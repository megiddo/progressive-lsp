//! Composition root: parse CLI, wire libs, call serve/install.

use std::ffi::OsString;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use progressive_lsp_control::ControlServer;
use progressive_lsp_core::{
    apply_worktree_excludes, InstallError, LogPort, MemoryLog, PrefixLayout, SystemClock,
};
use progressive_lsp_engine::{
    binary_name_for_pack, stub_pack_bytes, EngineSupervisor, PackAdapter,
};
use progressive_lsp_install::{
    hex_encode, sha256, sha256_file, ExplicitPacks, Installer, LocalFs, Manifest, ManifestArtifact,
    PackSelector,
};
use progressive_lsp_log::{CliUsageAdapter, StderrEmitAdapter};
use progressive_lsp_plugin::PluginRegistry;
use progressive_lsp_protocol::LspFacade;
use progressive_lsp_script::ScriptHost;

mod control_socket;
mod serve_host;
mod session;
pub use serve_host::{root_from_params, ServeDiskWatch, ServeHost};
pub use session::{register_languages, WorkspaceSession};

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
        other => Err(CliError::Usage(format!(
            "unknown command: {other}\n{USAGE}"
        ))),
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
    register_languages(&mut registry);
    registry
}

pub fn run<I, S>(args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    run_with_log(args, Arc::new(MemoryLog::new()))
}

pub fn run_with_log<I, S>(args: I, log: Arc<dyn LogPort>) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    match parse_args(args) {
        Err(e) => {
            CliUsageAdapter::new(Arc::clone(&log)).emit_usage(&e.to_string());
            Err(e.into())
        }
        Ok(Command::Serve(opts)) => run_serve_with_log(opts, log),
        Ok(Command::Install(opts)) => run_install(opts).map_err(|e| {
            StderrEmitAdapter::new(Arc::clone(&log)).emit(&e.to_string());
            e.into()
        }),
    }
}

pub fn run_serve(opts: ServeOpts) -> Result<(), Box<dyn std::error::Error>> {
    run_serve_with_log(opts, Arc::new(MemoryLog::new()))
}

fn run_serve_with_log(
    opts: ServeOpts,
    log: Arc<dyn LogPort>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_with_io_and_log(opts, BufReader::new(stdin.lock()), stdout.lock(), log)
}

/// Same as [`run_serve`] but injectable stdio (IT-1 handshake + unit tests).
pub fn serve_with_io<R, W>(
    opts: ServeOpts,
    reader: R,
    writer: W,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: io::BufRead,
    W: io::Write,
{
    serve_with_io_and_log(opts, reader, writer, Arc::new(MemoryLog::new()))
}

pub fn serve_with_io_and_log<R, W>(
    opts: ServeOpts,
    reader: R,
    writer: W,
    log: Arc<dyn LogPort>,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: io::BufRead,
    W: io::Write,
{
    let layout = PrefixLayout::resolve(opts.prefix.as_deref())?;
    layout.ensure_dirs()?;
    let _registry = build_registry();
    let mut supervisor = EngineSupervisor::new(Arc::new(SystemClock), layout.clone());
    supervisor.register(Box::new(PackAdapter::python()));
    supervisor.register(Box::new(PackAdapter::rust()));
    supervisor.register(Box::new(PackAdapter::clangd()));
    supervisor.register(Box::new(PackAdapter::tsgo()));
    supervisor.register(Box::new(PackAdapter::phpantom()));
    supervisor.register(Box::new(PackAdapter::superhtml()));
    supervisor.register(Box::new(PackAdapter::biome()));
    supervisor.register(Box::new(PackAdapter::gopls()));
    supervisor.register(Box::new(PackAdapter::zls()));
    let _supervisor = supervisor;
    let host = Arc::new(ServeHost::new_with_log(layout, log)?);
    let advertised = opts.control_socket.as_ref().map(|p| {
        control_socket::advertised_socket_path(p)
            .display()
            .to_string()
    });
    if let Some(path) = &opts.control_socket {
        let abs = control_socket::advertised_socket_path(path);
        let listener = control_socket::bind_control_socket(&abs)?;
        let srv = ControlServer::new("")
            .with_plane(Arc::clone(&host) as Arc<dyn progressive_lsp_control::ControlPlane>)
            .with_progressive(true);
        control_socket::spawn_control_accept(listener, Arc::new(srv), Arc::clone(&host));
    }
    let _ = opts.control_fd;
    let facade = LspFacade::new(advertised, opts.mux).with_intelligence(Arc::clone(&host) as _);
    if opts.mux {
        let srv = ControlServer::new("").with_progressive(true);
        let mut reader = reader;
        let mut writer = writer;
        facade.serve_mux(
            &mut reader,
            &mut writer,
            Some(|payload: &[u8]| srv.handle_mux_payload(payload).ok()),
        )?;
    } else {
        facade.serve(reader, writer)?;
    }
    Ok(())
}

pub fn run_install(opts: InstallOpts) -> Result<(), InstallError> {
    run_install_with_scripts(opts, None)
}

/// Hash-gated prefix. `on_install_verify` Abort refuses the new binary (no rename).
pub fn run_install_with_scripts(
    opts: InstallOpts,
    mut scripts: Option<&mut ScriptHost>,
) -> Result<(), InstallError> {
    let layout = PrefixLayout::from_path(&opts.prefix);
    layout
        .ensure_dirs()
        .map_err(|e| InstallError::Io(e.to_string()))?;
    let installer = Installer::new(LocalFs);
    for pack in &opts.packs {
        install_verified_pack(&installer, &layout, pack, scripts.as_deref_mut())?;
    }
    write_install_record(&installer, &layout, &opts.packs, scripts.as_deref_mut())?;
    Ok(())
}

fn install_verified_pack(
    installer: &Installer<LocalFs>,
    layout: &PrefixLayout,
    pack: &str,
    scripts: Option<&mut ScriptHost>,
) -> Result<(), InstallError> {
    let binary = binary_name_for_pack(pack)
        .ok_or_else(|| InstallError::Manifest(format!("unknown pack {pack}")))?;
    let bytes = stub_pack_bytes(pack, binary);
    let expected = sha256(&bytes);
    let dest = layout.engines_dir().join(pack).join(binary);
    let plan = installer.plan(dest, bytes, expected, true)?;
    apply_verified(installer, &plan, pack, scripts)?;
    let manifest = Manifest {
        version: "1".into(),
        artifacts: vec![ManifestArtifact {
            name: binary.into(),
            rel_path: binary.into(),
            sha256: hex_encode(&expected),
            executable: true,
        }],
    };
    let man_bytes = manifest.to_json()?.into_bytes();
    let man_hash = sha256(&man_bytes);
    let man_dest = layout.engines_dir().join(pack).join("manifest.json");
    let man_plan = installer.plan(man_dest, man_bytes, man_hash, false)?;
    apply_verified(installer, &man_plan, pack, None)?;
    Ok(())
}

fn apply_verified(
    installer: &Installer<LocalFs>,
    plan: &progressive_lsp_install::InstallPlan,
    pack: &str,
    scripts: Option<&mut ScriptHost>,
) -> Result<(), InstallError> {
    installer.apply_with_verify(plan, |_| {
        if let Some(host) = scripts {
            host.on_install_verify(&plan.dest.to_string_lossy(), pack)
                .map_err(|e| InstallError::Refused(e.0))?;
        }
        Ok(())
    })
}

fn write_install_record(
    installer: &Installer<LocalFs>,
    layout: &PrefixLayout,
    packs: &[String],
    scripts: Option<&mut ScriptHost>,
) -> Result<(), InstallError> {
    let record = format!("packs = {packs:?}\n");
    let dest = layout.root().join("installed-packs.toml");
    let hash = sha256(record.as_bytes());
    let plan = installer.plan(dest, record.into_bytes(), hash, false)?;
    apply_verified(installer, &plan, "core", scripts)
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

pub fn verify_existing(
    path: &Path,
    expected_hex: &str,
) -> Result<(), progressive_lsp_core::InstallError> {
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
    use std::path::Path;

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
        let cmd = parse_args([
            "plsp",
            "install",
            "--prefix",
            "/p",
            "--packs",
            "python,rust",
        ])
        .unwrap();
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
        #[cfg(feature = "lang-java")]
        {
            assert!(a
                .get(&progressive_lsp_core::LanguageId::new("java"))
                .is_ok());
            assert_eq!(
                a.get(&progressive_lsp_core::LanguageId::new("java"))
                    .unwrap()
                    .grammar_id(),
                "tree-sitter-java"
            );
        }
        #[cfg(feature = "lang-php")]
        {
            assert!(a.get(&progressive_lsp_core::LanguageId::new("php")).is_ok());
        }
        for slot in KNOWN_LANGUAGE_SLOTS {
            if matches!(
                *slot,
                "java"
                    | "php"
                    | "html"
                    | "css"
                    | "javascript"
                    | "typescript"
                    | "go"
                    | "zig"
                    | "python"
                    | "rust"
                    | "c"
                    | "cpp"
                    | "csharp"
            ) {
                continue;
            }
            assert!(
                a.get(&progressive_lsp_core::LanguageId::new(*slot))
                    .is_err(),
                "unregistered {slot} must stay UnsupportedLanguage"
            );
        }
        assert_eq!(
            a.contains(&progressive_lsp_core::LanguageId::new("java")),
            cfg!(feature = "lang-java")
        );
        #[cfg(feature = "lang-python")]
        {
            assert!(a
                .get(&progressive_lsp_core::LanguageId::new("python"))
                .is_ok());
        }
        #[cfg(not(feature = "lang-python"))]
        {
            assert!(a
                .get(&progressive_lsp_core::LanguageId::new("python"))
                .is_err());
        }
        #[cfg(feature = "lang-rust")]
        {
            assert!(a
                .get(&progressive_lsp_core::LanguageId::new("rust"))
                .is_ok());
        }
        #[cfg(feature = "lang-c")]
        {
            assert!(a.get(&progressive_lsp_core::LanguageId::new("c")).is_ok());
        }
        #[cfg(feature = "lang-cpp")]
        {
            assert!(a.get(&progressive_lsp_core::LanguageId::new("cpp")).is_ok());
        }
        #[cfg(feature = "lang-csharp")]
        {
            assert!(a
                .get(&progressive_lsp_core::LanguageId::new("csharp"))
                .is_ok());
        }
        let _ = b.registered_ids();
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
        let ty = dir.path().join("engines/python/ty");
        assert!(ty.is_file());
        let man = dir.path().join("engines/python/manifest.json");
        assert!(man.is_file());
        let found =
            progressive_lsp_engine::discover_pack(&PrefixLayout::from_path(dir.path()), "python")
                .unwrap();
        assert_eq!(found.path, ty);
        assert!(progressive_lsp_engine::is_pack_stub(
            &std::fs::read(&ty).unwrap()
        ));
    }

    #[test]
    fn install_unknown_pack_is_manifest_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_install(InstallOpts {
            prefix: dir.path().to_path_buf(),
            packs: vec!["csharp-ls".into()],
        })
        .unwrap_err();
        assert!(matches!(err, InstallError::Manifest(_)));
    }

    #[test]
    fn on_install_verify_abort_refuses_new_binary() {
        use progressive_lsp_core::FakeClock;
        use progressive_lsp_script::{FakeEngineFactory, ScriptDecision, ScriptHost};
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptHost::new(
            Box::new(FakeEngineFactory {
                decision: ScriptDecision::Abort("refuse-ty".into()),
                fail_create: None,
            }),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        host.load("ok", "fake").unwrap();
        let err = run_install_with_scripts(
            InstallOpts {
                prefix: dir.path().to_path_buf(),
                packs: vec!["python".into()],
            },
            Some(&mut host),
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Refused(msg) if msg.contains("refuse-ty")));
        assert!(!dir.path().join("engines/python/ty").exists());
    }

    #[test]
    fn install_local_blob_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"elf-stub";
        install_local_blob(dir.path(), "bin/progressive-lsp", bytes).unwrap();
        let path = dir.path().join("bin/progressive-lsp");
        let hex = progressive_lsp_install::hex_encode(&sha256(bytes));
        verify_existing(&path, &hex).unwrap();
        assert!(
            verify_existing(&path, &progressive_lsp_install::hex_encode(&sha256(b"no"))).is_err()
        );
    }

    fn handshake_bytes(root: Option<&Path>) -> Vec<u8> {
        let params = match root {
            Some(p) => serde_json::json!({
                "capabilities": {},
                "rootUri": format!("file://{}", p.display())
            }),
            None => serde_json::json!({"capabilities": {}}),
        };
        let mut stdin = framing::encode_message(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": params
            }))
            .unwrap(),
        );
        stdin.extend_from_slice(&framing::encode_message(
            serde_json::to_vec(
                &serde_json::json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            )
            .unwrap(),
        ));
        stdin.extend_from_slice(&framing::encode_message(
            serde_json::to_vec(
                &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
            )
            .unwrap(),
        ));
        stdin.extend_from_slice(&framing::encode_message(
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","method":"exit"})).unwrap(),
        ));
        stdin
    }

    #[test]
    fn serve_round_trip_via_facade_not_process_stdin() {
        let facade = LspFacade::new(None, false);
        let mut out = Vec::new();
        facade
            .serve(std::io::Cursor::new(handshake_bytes(None)), &mut out)
            .unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn serve_with_io_control_socket_is_advertised() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let sock = prefix.path().join("run/control.sock");
        let mut out = Vec::new();
        serve_with_io(
            ServeOpts {
                prefix: Some(prefix.path().to_path_buf()),
                control_socket: Some(sock.clone()),
                control_fd: None,
                mux: false,
            },
            std::io::Cursor::new(handshake_bytes(Some(workspace.path()))),
            &mut out,
        )
        .unwrap();
        let texts = framing::decode_all(&out).unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&texts[0]).unwrap();
        let cap = &resp["result"]["capabilities"]["experimental"]["progressiveLsp"];
        assert_eq!(cap["version"], "v1");
        assert_eq!(cap["mux"], false);
        let advertised = cap["socket"].as_str().unwrap();
        assert!(advertised.ends_with("control.sock"), "{advertised}");
        assert!(!advertised.is_empty());
    }

    #[test]
    fn serve_with_io_stock_initialize_control_off() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        serve_with_io(
            ServeOpts {
                prefix: Some(prefix.path().to_path_buf()),
                control_socket: None,
                control_fd: None,
                mux: false,
            },
            std::io::Cursor::new(handshake_bytes(Some(workspace.path()))),
            &mut out,
        )
        .unwrap();
        let texts = framing::decode_all(&out).unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&texts[0]).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "progressive-lsp");
        let cap = &resp["result"]["capabilities"]["experimental"]["progressiveLsp"];
        assert_eq!(cap["version"], "v1");
        assert!(cap["socket"].is_null());
        assert_eq!(cap["mux"], false);
        assert!(prefix.path().join("config.toml").is_file());
        assert!(prefix.path().join("cache").is_dir());
        assert!(!workspace.path().join(".progressivelsp/cache").exists());
        assert!(workspace
            .path()
            .join(".progressivelsp/.gitignore")
            .is_file());
    }

    #[test]
    fn serve_with_io_cli_prefix_creates_cliprefix_not_env() {
        let clip = tempfile::tempdir().unwrap();
        let env_home = tempfile::tempdir().unwrap();
        let old = std::env::var("PROGRESSIVE_LSP_HOME").ok();
        std::env::set_var("PROGRESSIVE_LSP_HOME", env_home.path());
        let result = serve_with_io(
            ServeOpts {
                prefix: Some(clip.path().to_path_buf()),
                control_socket: None,
                control_fd: None,
                mux: false,
            },
            std::io::Cursor::new(handshake_bytes(None)),
            Vec::new(),
        );
        match old {
            Some(v) => std::env::set_var("PROGRESSIVE_LSP_HOME", v),
            None => std::env::remove_var("PROGRESSIVE_LSP_HOME"),
        }
        result.unwrap();
        assert!(clip.path().join("config.toml").is_file());
        assert!(!env_home.path().join("config.toml").exists());
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

    #[test]
    fn run_with_log_emits_cli_usage_on_bad_args() {
        let log = progressive_lsp_core::FakeLog::new();
        assert!(run_with_log(["plsp", "nope"], Arc::new(log.clone())).is_err());
        assert!(
            log.records()
                .iter()
                .any(|r| r.operation.as_deref() == Some("cli")),
            "{:?}",
            log.records()
        );
    }

    #[test]
    fn product_crates_have_no_diagnostic_eprintln() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut hits = Vec::new();
        walk_product_rs(&root.join("src"), &mut hits);
        if let Ok(rd) = std::fs::read_dir(&root) {
            for entry in rd.flatten() {
                let p = entry.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("progressive-lsp-") && p.is_dir() {
                    walk_product_rs(&p, &mut hits);
                }
            }
        }
        assert!(
            hits.is_empty(),
            "diagnostic eprintln leftover (CLI usage is cli_usage.rs only): {hits:?}"
        );
    }

    fn walk_product_rs(dir: &Path, hits: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                    continue;
                }
                walk_product_rs(&path, hits);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("bakeoff.rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut in_tests = false;
            for (i, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("#[cfg(test)]") {
                    in_tests = true;
                }
                if in_tests {
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if trimmed.contains("eprintln!") {
                    if path.file_name().and_then(|n| n.to_str()) == Some("cli_usage.rs") {
                        continue;
                    }
                    hits.push(format!("{}:{}:{line}", path.display(), i + 1));
                }
            }
        }
    }
}
