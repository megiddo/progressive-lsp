//! Composition root: parse CLI, wire libs, call serve/install.

use std::ffi::OsString;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use progressive_lsp_core::{apply_worktree_excludes, PrefixLayout};
use progressive_lsp_install::{
    sha256, sha256_file, ExplicitPacks, Installer, LocalFs, Manifest, PackSelector,
};
use progressive_lsp_plugin::{register_builtins, PluginRegistry};
use progressive_lsp_protocol::LspFacade;

pub const USAGE: &str = "\
progressive-lsp serve [--prefix DIR] [--control-socket PATH] [--control-fd N] [--mux]
progressive-lsp install --prefix DIR --packs python,rust,...
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Serve(ServeOpts),
    Install(InstallOpts),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOpts {
    pub prefix: Option<PathBuf>,
    pub control_socket: Option<PathBuf>,
    pub control_fd: Option<u32>,
    pub mux: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOpts {
    pub prefix: PathBuf,
    pub packs: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    #[error("{0}")]
    Usage(String),
}

pub fn parse_args<I, S>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args.is_empty() {
        return Err(CliError::Usage(USAGE.trim_end().into()));
    }
    args.remove(0);
    if args.is_empty() {
        return Err(CliError::Usage(USAGE.trim_end().into()));
    }
    let head = args[0].to_string_lossy().into_owned();
    match head.as_str() {
        "serve" => parse_serve(&args[1..]),
        "install" => parse_install(&args[1..]),
        "-h" | "--help" | "help" => Err(CliError::Usage(USAGE.trim_end().into())),
        other => Err(CliError::Usage(format!("unknown command: {other}\n{USAGE}"))),
    }
}

fn parse_serve(args: &[OsString]) -> Result<Command, CliError> {
    let mut opts = ServeOpts {
        prefix: None,
        control_socket: None,
        control_fd: None,
        mux: false,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        match arg.as_ref() {
            "--prefix" => {
                opts.prefix = Some(require_value("--prefix", args, &mut i)?);
            }
            "--control-socket" => {
                opts.control_socket = Some(require_value("--control-socket", args, &mut i)?);
            }
            "--control-fd" => {
                let raw = require_raw("--control-fd", args, &mut i)?;
                let n = raw.parse::<u32>().map_err(|_| {
                    CliError::Usage(format!("--control-fd must be an integer, got {raw}"))
                })?;
                opts.control_fd = Some(n);
            }
            "--mux" => opts.mux = true,
            other => return Err(CliError::Usage(format!("unknown serve flag: {other}"))),
        }
        i += 1;
    }
    Ok(Command::Serve(opts))
}

fn parse_install(args: &[OsString]) -> Result<Command, CliError> {
    let mut prefix = None;
    let mut packs = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        match arg.as_ref() {
            "--prefix" => prefix = Some(require_value("--prefix", args, &mut i)?),
            "--packs" => {
                let raw = require_raw("--packs", args, &mut i)?;
                packs = Some(
                    ExplicitPacks::parse_csv(&raw)
                        .select(&progressive_lsp_install::HostProbe::current(
                            progressive_lsp_install::BuildCensus::default(),
                        ))
                        .into_iter()
                        .map(|p| p.0)
                        .collect(),
                );
            }
            other => return Err(CliError::Usage(format!("unknown install flag: {other}"))),
        }
        i += 1;
    }
    let prefix = prefix.ok_or_else(|| CliError::Usage("install requires --prefix DIR".into()))?;
    let packs = packs.ok_or_else(|| CliError::Usage("install requires --packs LIST".into()))?;
    Ok(Command::Install(InstallOpts { prefix, packs }))
}

fn require_value(flag: &str, args: &[OsString], i: &mut usize) -> Result<PathBuf, CliError> {
    Ok(PathBuf::from(require_raw(flag, args, i)?))
}

fn require_raw(flag: &str, args: &[OsString], i: &mut usize) -> Result<String, CliError> {
    *i += 1;
    let value = args
        .get(*i)
        .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))?;
    if value.to_string_lossy().starts_with('-') {
        return Err(CliError::Usage(format!("{flag} requires a value")));
    }
    Ok(value.to_string_lossy().into_owned())
}

pub fn build_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    register_builtins(&mut registry);
    registry
}

pub fn run<I, S>(args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    match parse_args(args)? {
        Command::Serve(opts) => run_serve(opts),
        Command::Install(opts) => run_install(opts).map_err(|e| e.into()),
    }
}

pub fn run_serve(opts: ServeOpts) -> Result<(), Box<dyn std::error::Error>> {
    let layout = PrefixLayout::resolve(opts.prefix.as_deref())?;
    layout.ensure_dirs()?;
    let _registry = build_registry();
    let socket = opts
        .control_socket
        .as_ref()
        .map(|p| p.display().to_string());
    let _ = opts.control_fd;
    let facade = LspFacade::new(socket, opts.mux);
    let stdin = io::stdin();
    let stdout = io::stdout();
    facade.serve(BufReader::new(stdin.lock()), stdout.lock())?;
    Ok(())
}

pub fn run_install(opts: InstallOpts) -> Result<(), progressive_lsp_core::InstallError> {
    let layout = PrefixLayout::from_path(&opts.prefix);
    layout
        .ensure_dirs()
        .map_err(|e| progressive_lsp_core::InstallError::Io(e.to_string()))?;
    write_install_record(&layout, &opts.packs)?;
    Ok(())
}

fn write_install_record(
    layout: &PrefixLayout,
    packs: &[String],
) -> Result<(), progressive_lsp_core::InstallError> {
    let record = format!("packs = {packs:?}\n");
    let dest = layout.root().join("installed-packs.toml");
    let hash = sha256(record.as_bytes());
    let installer = Installer::new(LocalFs);
    let plan = installer.plan(dest, record.into_bytes(), hash, false)?;
    installer.apply(&plan)
}

/// Schema-only local place of a verified blob under prefix (no network).
pub fn install_local_blob(
    prefix: &Path,
    rel_path: &str,
    bytes: &[u8],
) -> Result<(), progressive_lsp_core::InstallError> {
    let layout = PrefixLayout::from_path(prefix);
    layout
        .ensure_dirs()
        .map_err(|e| progressive_lsp_core::InstallError::Io(e.to_string()))?;
    let dest = layout.root().join(rel_path);
    let expected = sha256(bytes);
    let installer = Installer::new(LocalFs);
    let plan = installer.plan(dest, bytes.to_vec(), expected, true)?;
    installer.apply(&plan)
}

pub fn verify_existing(path: &Path, expected_hex: &str) -> Result<(), progressive_lsp_core::InstallError> {
    let actual = sha256_file(path)?;
    progressive_lsp_install::verify_hash(&actual, expected_hex)
}

pub fn exclude_workspace(workspace: &Path) -> Result<(), progressive_lsp_core::ConfigError> {
    apply_worktree_excludes(workspace).map(|_| ())
}

pub fn parse_manifest(json: &str) -> Result<Manifest, progressive_lsp_core::InstallError> {
    Manifest::parse(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_plugin::KNOWN_LANGUAGE_SLOTS;
    use progressive_lsp_protocol::framing;

    #[test]
    fn parse_serve_defaults_and_flags() {
        let cmd = parse_args(["plsp", "serve"]).unwrap();
        assert_eq!(
            cmd,
            Command::Serve(ServeOpts {
                prefix: None,
                control_socket: None,
                control_fd: None,
                mux: false,
            })
        );
        let cmd = parse_args([
            "plsp",
            "serve",
            "--prefix",
            "/p",
            "--control-socket",
            "/s",
            "--control-fd",
            "3",
            "--mux",
        ])
        .unwrap();
        assert_eq!(
            cmd,
            Command::Serve(ServeOpts {
                prefix: Some(PathBuf::from("/p")),
                control_socket: Some(PathBuf::from("/s")),
                control_fd: Some(3),
                mux: true,
            })
        );
    }

    #[test]
    fn parse_install_requires_prefix_and_packs() {
        assert!(parse_args(["plsp", "install"]).is_err());
        assert!(parse_args(["plsp", "install", "--prefix", "/p"]).is_err());
        let cmd = parse_args(["plsp", "install", "--prefix", "/p", "--packs", "python,rust"]).unwrap();
        assert_eq!(
            cmd,
            Command::Install(InstallOpts {
                prefix: PathBuf::from("/p"),
                packs: vec!["python".into(), "rust".into()],
            })
        );
    }

    #[test]
    fn parse_errors() {
        assert!(parse_args(Vec::<OsString>::new()).is_err());
        assert!(parse_args(["plsp"]).is_err());
        assert!(parse_args(["plsp", "help"]).is_err());
        assert!(parse_args(["plsp", "--help"]).is_err());
        assert!(parse_args(["plsp", "wat"]).is_err());
        assert!(parse_args(["plsp", "serve", "--nope"]).is_err());
        assert!(parse_args(["plsp", "serve", "--prefix"]).is_err());
        assert!(parse_args(["plsp", "serve", "--control-fd", "x"]).is_err());
        assert!(parse_args(["plsp", "serve", "--prefix", "--mux"]).is_err());
        assert!(parse_args(["plsp", "install", "--packs", "x"]).is_err());
        assert!(parse_args(["plsp", "install", "--nope"]).is_err());
        assert!(USAGE.contains("serve"));
        assert!(USAGE.contains("install"));
    }

    #[test]
    fn registry_is_injected_not_global() {
        let a = build_registry();
        let b = build_registry();
        assert!(a.is_empty());
        assert!(b.is_empty());
        for slot in KNOWN_LANGUAGE_SLOTS {
            assert!(a.get(&progressive_lsp_core::LanguageId::new(*slot)).is_err());
        }
    }

    #[test]
    fn install_writes_layout_under_prefix() {
        let dir = tempfile::tempdir().unwrap();
        run_install(InstallOpts {
            prefix: dir.path().to_path_buf(),
            packs: vec!["python".into()],
        })
        .unwrap();
        assert!(dir.path().join("bin").is_dir());
        assert!(dir.path().join("config.toml").is_file());
        let rec = std::fs::read_to_string(dir.path().join("installed-packs.toml")).unwrap();
        assert!(rec.contains("python"));
    }

    #[test]
    fn install_local_blob_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"elf-stub";
        install_local_blob(dir.path(), "bin/progressive-lsp", bytes).unwrap();
        let path = dir.path().join("bin/progressive-lsp");
        let hex = progressive_lsp_install::hex_encode(&sha256(bytes));
        verify_existing(&path, &hex).unwrap();
        assert!(verify_existing(&path, &progressive_lsp_install::hex_encode(&sha256(b"no"))).is_err());
    }

    #[test]
    fn serve_round_trip_via_facade_not_process_stdin() {
        let facade = LspFacade::new(None, false);
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let framed = framing::encode_message(serde_json::to_vec(&body).unwrap());
        let mut exit = framing::encode_message(
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","method":"exit"})).unwrap(),
        );
        let mut stdin = framed;
        stdin.append(&mut exit);
        let mut out = Vec::new();
        facade
            .serve(std::io::Cursor::new(stdin), &mut out)
            .unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn exclude_workspace_helper() {
        let dir = tempfile::tempdir().unwrap();
        exclude_workspace(dir.path()).unwrap();
        assert!(dir.path().join(".progressivelsp/.gitignore").is_file());
    }

    #[test]
    fn parse_manifest_helper() {
        let json = progressive_lsp_install::manifest::example_manifest_json(&sha256(b"z"));
        let m = parse_manifest(&json).unwrap();
        assert_eq!(m.version, "1");
    }

    #[test]
    fn run_dispatches_install() {
        let dir = tempfile::tempdir().unwrap();
        run([
            "plsp",
            "install",
            "--prefix",
            dir.path().to_str().unwrap(),
            "--packs",
            "rust",
        ])
        .unwrap();
        assert!(dir.path().join("installed-packs.toml").is_file());
    }

    #[test]
    fn run_rejects_bad_cli() {
        assert!(run(["plsp", "nope"]).is_err());
    }
}
