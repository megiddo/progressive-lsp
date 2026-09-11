//! Operator CLI behind `./build`. Spawn shells (`cargo`, `docker`) are N/A.
//!
//! [`XtaskCommand`], [`LspArch`], [`BuildTarget`], [`BuildFlags`], and
//! [`RunLaunch`] are Value objects.

use std::path::PathBuf;

use crate::freshness;
use crate::musl::{AARCH64_MUSL, X86_64_MUSL};
use crate::runtime_image::IMAGE_TAG;
use crate::{allocator, check_static, dist, musl, pack, perf, poc, runtime_image};

/// Which help screen to print. Value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Lsp,
    Ide,
    Run,
    Build,
}

impl HelpTopic {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lsp" => Some(Self::Lsp),
            "ide" => Some(Self::Ide),
            "run" | "poc" => Some(Self::Run),
            "build" => Some(Self::Build),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "help",
            Self::Lsp => "lsp",
            Self::Ide => "ide",
            Self::Run => "run",
            Self::Build => "build",
        }
    }
}

/// Linux musl architecture for `./build lsp`. Value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LspArch {
    All,
    X86_64,
    Aarch64,
}

impl LspArch {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "all" => Some(Self::All),
            "x86_64" | "amd64" | X86_64_MUSL => Some(Self::X86_64),
            "aarch64" | "arm64" | AARCH64_MUSL => Some(Self::Aarch64),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    #[cfg(test)]
    pub fn docker_args(self) -> Vec<String> {
        match self {
            Self::All => vec!["--both".into()],
            Self::X86_64 => vec!["--target".into(), X86_64_MUSL.into()],
            Self::Aarch64 => vec!["--target".into(), AARCH64_MUSL.into()],
        }
    }
}

/// Which engine packs `./build lsp` rebuilds before the runtime image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LspFlavor {
    /// Slim packs only (default). Runtime image step still requires dogfood dests if you tag `:local` manually.
    #[default]
    Slim,
    /// Slim + full packs (clangd, tsgo, gopls, zls) — matches dogfood `RuntimeImagePlan`.
    Dogfood,
}

impl LspFlavor {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "slim" => Some(Self::Slim),
            "dogfood" => Some(Self::Dogfood),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slim => "slim",
            Self::Dogfood => "dogfood",
        }
    }
}

/// Flags for `./build lsp {arch}`. Value object.
/// `--force` treats every artifact as stale (make: rebuild everything).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspFlags {
    pub force: bool,
    pub flavor: LspFlavor,
}

/// What `xtask build` produces. Value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTarget {
    /// Static language-server backends (engine packs).
    Backends,
    /// Native `progressive-lsp` for this machine.
    Controller,
    /// Linux musl controller + backends + `progressive-lsp-runtime:local`.
    Package,
    /// POC IDE (+ native controller). Does not start the window.
    Poc,
}

impl BuildTarget {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "backends" => Some(Self::Backends),
            "controller" => Some(Self::Controller),
            "package" => Some(Self::Package),
            "poc" => Some(Self::Poc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backends => "backends",
            Self::Controller => "controller",
            Self::Package => "package",
            Self::Poc => "poc",
        }
    }

    pub fn takes_docker_flags(self) -> bool {
        matches!(self, Self::Backends | Self::Package)
    }
}

/// Flags for `build backends` / `build package`. Value object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildFlags {
    pub full: bool,
    pub both: bool,
    pub target: Option<String>,
}

impl BuildFlags {
    pub fn validate(&self, target: BuildTarget) -> Result<(), String> {
        if target.takes_docker_flags() {
            if let Some(t) = &self.target {
                if t != X86_64_MUSL && t != AARCH64_MUSL {
                    return Err(format!(
                        "unsupported musl triple {t}; expected {X86_64_MUSL} or {AARCH64_MUSL}"
                    ));
                }
            }
            return Ok(());
        }
        if self.full || self.both || self.target.is_some() {
            return Err(format!(
                "build {} is a native cargo build; do not pass --full, --target, or --both \
                 (those flags are for `build backends` and `build package`)",
                target.as_str()
            ));
        }
        Ok(())
    }

    /// Args for `xtask musl` / `xtask runtime-image`. Always names a triple
    /// (this host's Linux arch) so we do not silently build both.
    pub fn docker_args(&self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        if self.both {
            out.push("--both".into());
        } else {
            out.push("--target".into());
            match &self.target {
                Some(t) => out.push(t.clone()),
                None => out.push(default_linux_triple()?.to_string()),
            }
        }
        Ok(out)
    }

    pub fn pack_args(&self) -> Result<Vec<String>, String> {
        let mut out = self.docker_args()?;
        if self.full {
            out.insert(0, "--full".into());
        }
        Ok(out)
    }
}

/// How to start poc-ide. Value object. Flags are parsed here so `run` does
/// not need a second `--` before `--folder` / `--container`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunLaunch {
    pub folder: Option<PathBuf>,
    pub file: Option<PathBuf>,
    pub container: bool,
    /// Explicit native open (T1/T2 on non-Linux). Opts out of folder container default.
    pub native: bool,
    pub control_socket: Option<PathBuf>,
    pub extra: Vec<String>,
}

impl RunLaunch {
    /// Non-Linux `--folder` opens in the container unless `--native` or `--container` was set.
    pub fn apply_host_defaults(&mut self) {
        if cfg!(target_os = "linux") {
            return;
        }
        if self.native || self.container {
            return;
        }
        if self.folder.is_some() {
            self.container = true;
        }
    }

    #[cfg(test)]
    pub fn apply_host_defaults_for_editor_linux(&mut self, editor_is_linux: bool) {
        if editor_is_linux {
            return;
        }
        if self.native || self.container {
            return;
        }
        if self.folder.is_some() {
            self.container = true;
        }
    }

    pub fn to_forwarded(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(folder) = &self.folder {
            out.push("--folder".into());
            out.push(folder.display().to_string());
        }
        if let Some(file) = &self.file {
            out.push("--file".into());
            out.push(file.display().to_string());
        }
        if self.container {
            out.push("--container".into());
        }
        if self.native {
            out.push("--native".into());
        }
        if let Some(sock) = &self.control_socket {
            out.push("--control-socket".into());
            out.push(sock.display().to_string());
        }
        out.extend(self.extra.iter().cloned());
        out
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(folder) = &self.folder {
            if folder.as_os_str().is_empty() {
                return Err("run --folder requires a directory".into());
            }
            if self.container && !folder.is_absolute() {
                return Err(
                    "container mode needs an absolute --folder (bind-mount identity: -v $WS:$WS)"
                        .into(),
                );
            }
        }
        if let Some(file) = &self.file {
            if file.as_os_str().is_empty() {
                return Err("run --file requires a path".into());
            }
        }
        Ok(())
    }
}

/// Parsed argv. Value object. Spawn is [`run`]; tests parse only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XtaskCommand {
    Help {
        topic: HelpTopic,
    },
    /// `./build lsp` with no arch: print the valid list (usage error).
    LspArches,
    Lsp {
        arch: LspArch,
        flags: LspFlags,
    },
    Ide,
    Build {
        target: BuildTarget,
        flags: BuildFlags,
    },
    Run {
        launch: RunLaunch,
    },
    Legacy {
        name: String,
        rest: Vec<String>,
    },
}

pub fn default_linux_triple() -> Result<&'static str, String> {
    if cfg!(target_arch = "aarch64") {
        Ok(AARCH64_MUSL)
    } else if cfg!(target_arch = "x86_64") {
        Ok(X86_64_MUSL)
    } else {
        Err(format!(
            "no default musl triple for this host; pass --target {X86_64_MUSL} or {AARCH64_MUSL}"
        ))
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    match parse(args)? {
        XtaskCommand::Help { topic } => {
            print_help(topic);
            Ok(())
        }
        XtaskCommand::LspArches => {
            print_help(HelpTopic::Lsp);
            Err("pass all, x86_64, or aarch64".into())
        }
        XtaskCommand::Lsp { arch, flags } => freshness::execute_lsp(arch, flags),
        XtaskCommand::Ide => {
            poc::build_native_controller()?;
            poc::build_poc_bin()
        }
        XtaskCommand::Build { target, flags } => execute_build(target, flags),
        XtaskCommand::Run { launch } => execute_run(launch),
        XtaskCommand::Legacy { name, rest } => execute_legacy(&name, &rest),
    }
}

pub fn parse(args: &[String]) -> Result<XtaskCommand, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "help" | "-h" | "--help" => parse_help(&args.get(1..).unwrap_or(&[])),
        "lsp" => parse_lsp(&args[1..]),
        "ide" => parse_ide(&args[1..]),
        "run" => parse_run_ide(&args[1..]),
        "poc" => parse_run_flags(&args[1..]),
        "build" => parse_build(&args[1..]),
        "musl" | "pack" | "runtime-image" | "check-static" | "bench-alloc" | "bench-perf"
        | "dist" => Ok(XtaskCommand::Legacy {
            name: cmd.to_string(),
            rest: args[1..].to_vec(),
        }),
        other => Err(format!("unknown command: {other}\nTry: ./build help")),
    }
}

fn parse_help(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        None | Some("help") | Some("-h") | Some("--help") => {
            Ok(XtaskCommand::Help {
                topic: HelpTopic::Root,
            })
        }
        Some(topic) => match HelpTopic::parse(topic) {
            Some(topic) => Ok(XtaskCommand::Help { topic }),
            None => Err(format!(
                "unknown help topic: {topic}\nTry: ./build help  |  ./build help lsp  |  ./build help ide  |  ./build help run"
            )),
        },
    }
}

fn parse_lsp(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        None => Ok(XtaskCommand::LspArches),
        Some("-h") | Some("--help") | Some("help") => Ok(XtaskCommand::Help {
            topic: HelpTopic::Lsp,
        }),
        Some(raw) => match LspArch::parse(raw) {
            Some(arch) => {
                let flags = parse_lsp_flags(&args[1..])?;
                Ok(XtaskCommand::Lsp { arch, flags })
            }
            None => Err(format!("unknown architecture: {raw}\n\n{LSP_ARCHES}")),
        },
    }
}

fn parse_lsp_flags(args: &[String]) -> Result<LspFlags, String> {
    let mut flags = LspFlags::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--force" => {
                if flags.force {
                    return Err(
                        "unexpected argument after architecture: --force\nTry: ./build help lsp"
                            .into(),
                    );
                }
                flags.force = true;
            }
            "--flavor" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "missing value for --flavor\nTry: ./build help lsp".to_string())?;
                flags.flavor = LspFlavor::parse(raw).ok_or_else(|| {
                    format!("unknown --flavor {raw} (expected slim or dogfood)\nTry: ./build help lsp")
                })?;
            }
            other if let Some(raw) = other.strip_prefix("--flavor=") => {
                flags.flavor = LspFlavor::parse(raw).ok_or_else(|| {
                    format!("unknown --flavor {raw} (expected slim or dogfood)\nTry: ./build help lsp")
                })?;
            }
            other => {
                return Err(format!(
                    "unexpected argument after architecture: {other}\nTry: ./build help lsp"
                ));
            }
        }
        i += 1;
    }
    Ok(flags)
}

fn parse_ide(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        None => Ok(XtaskCommand::Ide),
        Some("-h") | Some("--help") | Some("help") => Ok(XtaskCommand::Help {
            topic: HelpTopic::Ide,
        }),
        Some(other) => Err(format!(
            "unknown ide argument: {other}\nTry: ./build help ide"
        )),
    }
}

fn parse_run_ide(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => Ok(XtaskCommand::Help {
            topic: HelpTopic::Run,
        }),
        Some("ide") => parse_run_flags(&args[1..]),
        Some(other) => Err(format!(
            "unknown run target: {other}\nusage: ./build run ide [--folder DIR] [--container]"
        )),
    }
}

fn parse_build(args: &[String]) -> Result<XtaskCommand, String> {
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => Ok(XtaskCommand::Help {
            topic: HelpTopic::Build,
        }),
        Some(raw) => {
            let target = BuildTarget::parse(raw).ok_or_else(|| {
                format!(
                    "unknown build target: {raw}\nTry: cargo xtask help build\n\
                     Targets: backends, controller, package, poc"
                )
            })?;
            let flags = parse_build_flags(&args[1..])?;
            flags.validate(target)?;
            Ok(XtaskCommand::Build { target, flags })
        }
    }
}

fn parse_build_flags(args: &[String]) -> Result<BuildFlags, String> {
    let mut flags = BuildFlags::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                return Err("put --help after `build`, not after the target".into());
            }
            "--full" => flags.full = true,
            "--both" => flags.both = true,
            "--target" => {
                i += 1;
                let t = args.get(i).ok_or(
                    "--target requires x86_64-unknown-linux-musl or aarch64-unknown-linux-musl",
                )?;
                flags.target = Some(t.clone());
            }
            other => {
                return Err(format!(
                    "unknown build flag: {other}\nTry: cargo xtask help build"
                ));
            }
        }
        i += 1;
    }
    Ok(flags)
}

fn parse_run_flags(args: &[String]) -> Result<XtaskCommand, String> {
    if args
        .first()
        .is_some_and(|a| a == "-h" || a == "--help" || a == "help")
    {
        return Ok(XtaskCommand::Help {
            topic: HelpTopic::Run,
        });
    }
    let mut launch = RunLaunch::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            launch.extra.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(value) = arg.strip_prefix("--folder=") {
            take_path(&mut launch.folder, value, "run --folder")?;
        } else if let Some(value) = arg.strip_prefix("--file=") {
            take_path(&mut launch.file, value, "run --file")?;
        } else if let Some(value) = arg.strip_prefix("--control-socket=") {
            take_path(&mut launch.control_socket, value, "run --control-socket")?;
        } else if arg == "--folder" {
            let value = next_value(args, &mut i, "run --folder requires a directory")?;
            take_path(&mut launch.folder, value, "run --folder")?;
        } else if arg == "--file" {
            let value = next_value(args, &mut i, "run --file requires a path")?;
            take_path(&mut launch.file, value, "run --file")?;
        } else if arg == "--control-socket" {
            let value = next_value(args, &mut i, "run --control-socket requires a path")?;
            take_path(&mut launch.control_socket, value, "run --control-socket")?;
        } else if arg == "--container" {
            launch.container = true;
        } else if arg == "--native" {
            launch.native = true;
        } else if arg == "-h" || arg == "--help" {
            return Ok(XtaskCommand::Help {
                topic: HelpTopic::Run,
            });
        } else {
            return Err(format!("unknown run flag: {arg}\nTry: ./build help run"));
        }
        i += 1;
    }
    launch.apply_host_defaults();
    launch.validate()?;
    Ok(XtaskCommand::Run { launch })
}

fn next_value<'a>(args: &'a [String], i: &mut usize, err: &str) -> Result<&'a str, String> {
    let next = args.get(*i + 1).map(String::as_str);
    match next {
        Some(value) if !value.is_empty() && !value.starts_with('-') => {
            *i += 1;
            Ok(value)
        }
        _ => Err(err.into()),
    }
}

fn take_path(slot: &mut Option<PathBuf>, value: &str, flag: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{flag} requires a path"));
    }
    *slot = Some(PathBuf::from(value));
    Ok(())
}

pub fn print_help(topic: HelpTopic) {
    match topic {
        HelpTopic::Root => eprintln!("{}", ROOT_HELP),
        HelpTopic::Lsp => eprintln!("{}", LSP_ARCHES),
        HelpTopic::Ide => eprintln!("{}", IDE_HELP),
        HelpTopic::Run => eprintln!("{}", RUN_HELP),
        HelpTopic::Build => eprintln!("{}", BUILD_HELP),
    }
}

const ROOT_HELP: &str = "\
Usage:
  ./build lsp {all|x86_64|aarch64} [--force] [--flavor slim|dogfood]
  ./build ide
  ./build run ide
  ./build run ide --folder DIR
  ./build run ide --folder DIR --container

  ./build help
  ./build help lsp
  ./build help ide
  ./build help run

lsp     Linux static language server (musl controller + backends +
        container image progressive-lsp-runtime:local). Needs Docker.
        Architecture is required. Omit it to list valid arches.
        Rebuilds artifacts whose inputs changed; skips dests that are
        fresh. --force rebuilds controller, backends, and image.

ide     POC editor + native progressive-lsp. Does not start the window.

run ide Start the POC editor (rebuilds native progressive-lsp first).
        --folder must be absolute when using --container.
";

pub const LSP_ARCHES: &str = "\
usage: ./build lsp {all|x86_64|aarch64} [--force] [--flavor slim|dogfood]

Valid architectures:
  all       both Linux musl triples
  x86_64    x86_64-unknown-linux-musl
  aarch64   aarch64-unknown-linux-musl

  --force   rebuild controller, backends, and runtime image even when
            dest ELFs / image stamp are fresh
  --flavor  slim (default): slim engine packs only. dogfood: slim + full
            packs (clangd, tsgo, gopls, zls) required by the runtime image.
            clangd needs pack-cache (--cache pull or --cache-fill; see consumer.md).
";

const IDE_HELP: &str = "\
usage: ./build ide

Build the POC IDE and the native progressive-lsp it spawns.
Does not open the window. Start it with: ./build run ide
";

const RUN_HELP: &str = "\
usage: ./build run ide [--folder DIR] [--file PATH] [--container] [--native]

Start the POC IDE. Stays running until you quit the editor window.

  ./build run ide
  ./build run ide --folder DIR
  ./build run ide --folder DIR --native

On macOS/Windows, --folder DIR defaults to container mode (absolute path
required for bind-mount identity). Use --native for optional T1/T2-only
native serve without Docker.

Container mode needs Docker Desktop and ./build lsp <arch> first.
For full T3 engines in the runtime image: ./build lsp <arch> --flavor dogfood
(missing packs gate T3 per language only). First T3 proof is Python, PHP, or
Java source, not a Darwin Cargo tree.
";

const BUILD_HELP: &str = "\
Low-level cargo xtask build <target> [options]

Prefer: ./build lsp <arch>  and  ./build ide

Targets
  backends      Static language-server backends (deployment ELFs).
  controller    Native progressive-lsp for this machine.
  package       Linux musl controller + backends + runtime image.
  poc           POC IDE + native controller (same as ./build ide).

Options (backends and package only)
  --full        Also clangd, tsgo, gopls, zls
  --target T    x86_64-unknown-linux-musl  or  aarch64-unknown-linux-musl
  --both        Both triples
";

fn execute_build(target: BuildTarget, flags: BuildFlags) -> Result<(), String> {
    match target {
        BuildTarget::Backends => {
            eprintln!("xtask build backends: static language-server packs");
            pack::run(&flags.pack_args()?)
        }
        BuildTarget::Controller => poc::build_native_controller(),
        BuildTarget::Package => {
            let docker = flags.docker_args()?;
            eprintln!("xtask build package: (1/3) Linux musl controller");
            musl::run(&docker)?;
            eprintln!("xtask build package: (2/3) backends");
            pack::run(&flags.pack_args()?)?;
            eprintln!("xtask build package: (3/3) runtime image {IMAGE_TAG}");
            runtime_image::run(&docker)
        }
        BuildTarget::Poc => {
            poc::build_native_controller()?;
            poc::build_poc_bin()
        }
    }
}

fn execute_run(launch: RunLaunch) -> Result<(), String> {
    if launch.container {
        eprintln!(
            "./build run ide: container mode -> image {IMAGE_TAG} (Docker Desktop + `./build lsp <arch>`)"
        );
    }
    poc::run_poc_ide(&launch.to_forwarded())
}

fn execute_legacy(name: &str, rest: &[String]) -> Result<(), String> {
    match name {
        "musl" => musl::run(rest),
        "pack" => pack::run(rest),
        "runtime-image" => runtime_image::run(rest),
        "check-static" => check_static::run(rest),
        "bench-alloc" => allocator::run(rest),
        "bench-perf" => perf::run(rest),
        "dist" => dist::run(rest),
        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_line(args: &[&str]) -> Result<XtaskCommand, String> {
        parse(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn help_topic_lsp_arch_and_build_target_are_value_objects() {
        assert_eq!(HelpTopic::parse("lsp"), Some(HelpTopic::Lsp));
        assert_eq!(HelpTopic::parse("ide"), Some(HelpTopic::Ide));
        assert_eq!(HelpTopic::parse("run"), Some(HelpTopic::Run));
        assert_eq!(HelpTopic::parse("poc"), Some(HelpTopic::Run));
        assert_eq!(HelpTopic::parse("build"), Some(HelpTopic::Build));
        assert_eq!(HelpTopic::parse("nope"), None);
        assert_eq!(HelpTopic::Root.as_str(), "help");
        assert_eq!(HelpTopic::Lsp.as_str(), "lsp");
        assert_eq!(HelpTopic::Ide.as_str(), "ide");
        assert_eq!(HelpTopic::Run.as_str(), "run");
        assert_eq!(HelpTopic::Build.as_str(), "build");
        assert_ne!(HelpTopic::Root, HelpTopic::Lsp);

        assert_eq!(LspArch::parse("all"), Some(LspArch::All));
        assert_eq!(LspArch::parse("x86_64"), Some(LspArch::X86_64));
        assert_eq!(LspArch::parse("amd64"), Some(LspArch::X86_64));
        assert_eq!(LspArch::parse(X86_64_MUSL), Some(LspArch::X86_64));
        assert_eq!(LspArch::parse("aarch64"), Some(LspArch::Aarch64));
        assert_eq!(LspArch::parse("arm64"), Some(LspArch::Aarch64));
        assert_eq!(LspArch::parse(AARCH64_MUSL), Some(LspArch::Aarch64));
        assert_eq!(LspArch::parse("riscv64"), None);
        assert_eq!(LspArch::All.as_str(), "all");
        assert_eq!(LspArch::X86_64.as_str(), "x86_64");
        assert_eq!(LspArch::Aarch64.as_str(), "aarch64");
        assert_eq!(LspArch::All.docker_args(), vec!["--both".to_string()]);
        assert_eq!(
            LspArch::X86_64.docker_args(),
            vec!["--target".to_string(), X86_64_MUSL.into()]
        );
        assert_eq!(
            LspArch::Aarch64.docker_args(),
            vec!["--target".to_string(), AARCH64_MUSL.into()]
        );
        assert_ne!(LspArch::X86_64, LspArch::Aarch64);

        assert_eq!(BuildTarget::parse("backends"), Some(BuildTarget::Backends));
        assert_eq!(
            BuildTarget::parse("controller"),
            Some(BuildTarget::Controller)
        );
        assert_eq!(BuildTarget::parse("package"), Some(BuildTarget::Package));
        assert_eq!(BuildTarget::parse("poc"), Some(BuildTarget::Poc));
        assert_eq!(BuildTarget::parse("image"), None);
        assert_eq!(BuildTarget::Backends.as_str(), "backends");
        assert_eq!(BuildTarget::Controller.as_str(), "controller");
        assert_eq!(BuildTarget::Package.as_str(), "package");
        assert_eq!(BuildTarget::Poc.as_str(), "poc");
        assert!(BuildTarget::Backends.takes_docker_flags());
        assert!(BuildTarget::Package.takes_docker_flags());
        assert!(!BuildTarget::Controller.takes_docker_flags());
        assert!(!BuildTarget::Poc.takes_docker_flags());
        assert_ne!(BuildTarget::Backends, BuildTarget::Package);
    }

    #[test]
    fn empty_argv_and_help_print_root() {
        assert_eq!(
            parse_line(&[]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Root
            }
        );
        assert_eq!(
            parse_line(&["help"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Root
            }
        );
        assert_eq!(
            parse_line(&["--help"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Root
            }
        );
        assert_eq!(
            parse_line(&["help", "lsp"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Lsp
            }
        );
        assert_eq!(
            parse_line(&["help", "ide"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Ide
            }
        );
        assert_eq!(
            parse_line(&["help", "run"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Run
            }
        );
        let err = parse_line(&["help", "nope"]).unwrap_err();
        assert!(err.contains("unknown help topic"), "{err}");
    }

    #[test]
    fn lsp_without_arch_lists_valid_arches() {
        assert_eq!(parse_line(&["lsp"]).unwrap(), XtaskCommand::LspArches);
        assert_eq!(
            parse_line(&["lsp", "--help"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Lsp
            }
        );
        assert_eq!(
            parse_line(&["lsp", "all"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::All,
                flags: LspFlags::default(),
            }
        );
        assert_eq!(
            parse_line(&["lsp", "x86_64"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::X86_64,
                flags: LspFlags::default(),
            }
        );
        assert_eq!(
            parse_line(&["lsp", "aarch64"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::Aarch64,
                flags: LspFlags::default(),
            }
        );
        assert_eq!(
            parse_line(&["lsp", "x86_64", "--flavor", "dogfood"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::X86_64,
                flags: LspFlags {
                    flavor: LspFlavor::Dogfood,
                    ..LspFlags::default()
                },
            }
        );
        assert_eq!(
            parse_line(&["lsp", "all", "--flavor=dogfood", "--force"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::All,
                flags: LspFlags {
                    force: true,
                    flavor: LspFlavor::Dogfood,
                },
            }
        );
        let err = parse_line(&["lsp", "x86_64", "--flavor", "fat"]).unwrap_err();
        assert!(err.contains("unknown --flavor"), "{err}");
        let err = parse_line(&["lsp", "riscv64"]).unwrap_err();
        assert!(err.contains("unknown architecture: riscv64"), "{err}");
        assert!(err.contains("Valid architectures"), "{err}");
        assert!(err.contains("x86_64"), "{err}");
        assert!(err.contains("aarch64"), "{err}");
        assert!(err.contains("all"), "{err}");
        assert_eq!(parse_line(&["ide"]).unwrap(), XtaskCommand::Ide);
        let err = run(&["lsp".into()]).unwrap_err();
        assert!(err.contains("pass all, x86_64, or aarch64"), "{err}");
    }

    #[test]
    fn lsp_force_parses_and_unknown_extra_args_fail_closed() {
        assert_eq!(
            parse_line(&["lsp", "all", "--force"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::All,
                flags: LspFlags {
                    force: true,
                    ..LspFlags::default()
                },
            }
        );
        assert_eq!(
            parse_line(&["lsp", "aarch64", "--force"]).unwrap(),
            XtaskCommand::Lsp {
                arch: LspArch::Aarch64,
                flags: LspFlags {
                    force: true,
                    ..LspFlags::default()
                },
            }
        );
        let forced = LspFlags {
            force: true,
            ..LspFlags::default()
        };
        assert_eq!(forced, forced.clone());
        assert_ne!(LspFlags::default(), forced);
        let err = parse_line(&["lsp", "all", "--wat"]).unwrap_err();
        assert!(
            err.contains("unexpected argument after architecture: --wat"),
            "{err}"
        );
        let err = parse_line(&["lsp", "x86_64", "--force", "--wat"]).unwrap_err();
        assert!(
            err.contains("unexpected argument after architecture: --wat"),
            "{err}"
        );
        let err = parse_line(&["lsp", "all", "--force", "--force"]).unwrap_err();
        assert!(
            err.contains("unexpected argument after architecture: --force"),
            "{err}"
        );
    }

    #[test]
    fn parse_build_targets_and_flags() {
        assert_eq!(
            parse_line(&["build"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Build
            }
        );
        assert_eq!(
            parse_line(&["build", "backends"]).unwrap(),
            XtaskCommand::Build {
                target: BuildTarget::Backends,
                flags: BuildFlags::default(),
            }
        );
        assert_eq!(
            parse_line(&["build", "package", "--full", "--both"]).unwrap(),
            XtaskCommand::Build {
                target: BuildTarget::Package,
                flags: BuildFlags {
                    full: true,
                    both: true,
                    target: None,
                },
            }
        );
        assert_eq!(
            parse_line(&["build", "backends", "--target", AARCH64_MUSL,]).unwrap(),
            XtaskCommand::Build {
                target: BuildTarget::Backends,
                flags: BuildFlags {
                    full: false,
                    both: false,
                    target: Some(AARCH64_MUSL.into()),
                },
            }
        );
        assert_eq!(
            parse_line(&["build", "controller"]).unwrap(),
            XtaskCommand::Build {
                target: BuildTarget::Controller,
                flags: BuildFlags::default(),
            }
        );
        assert_eq!(
            parse_line(&["build", "poc"]).unwrap(),
            XtaskCommand::Build {
                target: BuildTarget::Poc,
                flags: BuildFlags::default(),
            }
        );
    }

    #[test]
    fn build_rejects_unknown_target_and_native_docker_flags() {
        let err = parse_line(&["build", "image"]).unwrap_err();
        assert!(err.contains("unknown build target"), "{err}");
        assert!(err.contains("backends"), "{err}");
        let err = parse_line(&["build", "controller", "--full"]).unwrap_err();
        assert!(err.contains("native cargo build"), "{err}");
        let err = parse_line(&["build", "poc", "--target", AARCH64_MUSL]).unwrap_err();
        assert!(err.contains("native cargo build"), "{err}");
        let err = parse_line(&["build", "backends", "--cache-fill"]).unwrap_err();
        assert!(err.contains("unknown build flag"), "{err}");
        let err = parse_line(&["build", "package", "--target"]).unwrap_err();
        assert!(err.contains("--target requires"), "{err}");
        let err = parse_line(&["build", "package", "--target", "not-a-triple"]).unwrap_err();
        assert!(err.contains("unsupported musl triple"), "{err}");
    }

    #[test]
    fn build_flags_name_one_triple_by_default() {
        let flags = BuildFlags::default();
        let docker = flags.docker_args().unwrap();
        assert_eq!(
            docker,
            vec![
                "--target".to_string(),
                default_linux_triple().unwrap().into()
            ]
        );
        let pack = flags.pack_args().unwrap();
        assert_eq!(pack, docker);
        let full = BuildFlags {
            full: true,
            both: false,
            target: Some(X86_64_MUSL.into()),
        };
        assert_eq!(
            full.pack_args().unwrap(),
            vec!["--full".to_string(), "--target".into(), X86_64_MUSL.into(),]
        );
        let both = BuildFlags {
            full: false,
            both: true,
            target: None,
        };
        assert_eq!(both.docker_args().unwrap(), vec!["--both".to_string()]);
        let triple = default_linux_triple().unwrap();
        assert!(triple == X86_64_MUSL || triple == AARCH64_MUSL);
    }

    #[test]
    fn parse_run_ide_folder_container_file_without_extra_dash_dash() {
        assert_eq!(
            parse_line(&["run"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Run
            }
        );
        assert_eq!(
            parse_line(&["run", "ide"]).unwrap(),
            XtaskCommand::Run {
                launch: RunLaunch::default()
            }
        );
        assert_eq!(
            parse_line(&["run", "--help"]).unwrap(),
            XtaskCommand::Help {
                topic: HelpTopic::Run
            }
        );
        let cmd = parse_line(&[
            "run",
            "ide",
            "--folder",
            "/abs/src",
            "--container",
            "--file",
            "/abs/src/a.py",
        ])
        .unwrap();
        match cmd {
            XtaskCommand::Run { launch } => {
                assert_eq!(
                    launch.folder.as_deref().map(|p| p.to_str().unwrap()),
                    Some("/abs/src")
                );
                assert_eq!(
                    launch.file.as_deref().map(|p| p.to_str().unwrap()),
                    Some("/abs/src/a.py")
                );
                assert!(launch.container);
                assert_eq!(
                    launch.to_forwarded(),
                    vec![
                        "--folder",
                        "/abs/src",
                        "--file",
                        "/abs/src/a.py",
                        "--container",
                    ]
                );
            }
            other => panic!("{other:?}"),
        }
        let eq = parse_line(&["run", "ide", "--folder=/ws"]).unwrap();
        match eq {
            XtaskCommand::Run { launch } => {
                assert_eq!(
                    launch.folder.as_deref().map(|p| p.to_str().unwrap()),
                    Some("/ws")
                );
                let mut native_only = RunLaunch {
                    folder: Some(PathBuf::from("/ws")),
                    native: true,
                    ..RunLaunch::default()
                };
                native_only.apply_host_defaults_for_editor_linux(false);
                assert!(!native_only.container);
                let mut default_container = RunLaunch {
                    folder: Some(PathBuf::from("/ws")),
                    ..RunLaunch::default()
                };
                default_container.apply_host_defaults_for_editor_linux(false);
                assert!(default_container.container);
            }
            other => panic!("{other:?}"),
        }
        let err = parse_line(&["run", "nope"]).unwrap_err();
        assert!(err.contains("unknown run target"), "{err}");
        assert!(err.contains("./build run ide"), "{err}");
    }

    #[test]
    fn poc_is_run_alias_and_still_forwards_after_dash_dash() {
        assert_eq!(
            parse_line(&["poc"]).unwrap(),
            parse_line(&["run", "ide"]).unwrap()
        );
        let cmd = parse_line(&["poc", "--", "--folder", "/ws"]).unwrap();
        match cmd {
            XtaskCommand::Run { launch } => {
                assert_eq!(launch.extra, vec!["--folder", "/ws"]);
                assert_eq!(launch.to_forwarded(), vec!["--folder", "/ws"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn run_rejects_relative_folder_in_container_and_missing_values() {
        let err = parse_line(&["run", "ide", "--folder", "rel", "--container"]).unwrap_err();
        assert!(err.contains("absolute"), "{err}");
        let err = parse_line(&["run", "ide", "--folder"]).unwrap_err();
        assert!(err.contains("--folder requires"), "{err}");
        let err = parse_line(&["run", "ide", "--folder", "--container"]).unwrap_err();
        assert!(err.contains("--folder requires"), "{err}");
        let err = parse_line(&["run", "ide", "--folder="]).unwrap_err();
        assert!(err.contains("--folder requires"), "{err}");
        let err = parse_line(&["run", "ide", "--file"]).unwrap_err();
        assert!(err.contains("--file requires"), "{err}");
        let err = parse_line(&["run", "ide", "--nope"]).unwrap_err();
        assert!(err.contains("unknown run flag"), "{err}");
        let err = parse_line(&["run", "ide", "--control-socket"]).unwrap_err();
        assert!(err.contains("--control-socket requires"), "{err}");
    }

    #[test]
    fn unknown_command_points_at_help() {
        let err = parse_line(&["nope"]).unwrap_err();
        assert!(err.contains("unknown command: nope"), "{err}");
        assert!(err.contains("./build help"), "{err}");
    }

    #[test]
    fn legacy_commands_are_still_named() {
        match parse_line(&["musl", "--both"]).unwrap() {
            XtaskCommand::Legacy { name, rest } => {
                assert_eq!(name, "musl");
                assert_eq!(rest, vec!["--both"]);
            }
            other => panic!("{other:?}"),
        }
        for cmd in [
            "pack",
            "runtime-image",
            "check-static",
            "bench-alloc",
            "bench-perf",
            "dist",
        ] {
            match parse_line(&[cmd]).unwrap() {
                XtaskCommand::Legacy { name, .. } => assert_eq!(name, cmd),
                other => panic!("{cmd}: {other:?}"),
            }
        }
    }

    #[test]
    fn help_screens_name_the_four_jobs() {
        print_help(HelpTopic::Root);
        print_help(HelpTopic::Lsp);
        print_help(HelpTopic::Ide);
        print_help(HelpTopic::Run);
        assert!(ROOT_HELP.contains("./build lsp"));
        assert!(ROOT_HELP.contains("./build ide"));
        assert!(ROOT_HELP.contains("./build run ide"));
        assert!(ROOT_HELP.contains("--container"));
        assert!(ROOT_HELP.contains("--force"));
        assert!(LSP_ARCHES.contains("Valid architectures"));
        assert!(LSP_ARCHES.contains("x86_64"));
        assert!(LSP_ARCHES.contains("aarch64"));
        assert!(LSP_ARCHES.contains("all"));
        assert!(LSP_ARCHES.contains("--force"));
        assert!(LSP_ARCHES.contains("[--force]"));
        assert!(IDE_HELP.contains("./build ide"));
        assert!(RUN_HELP.contains("--flavor dogfood"));
        assert!(RUN_HELP.contains("--native"));
        assert!(RUN_HELP.contains("absolute"));
        assert!(BUILD_HELP.contains(IMAGE_TAG) || BUILD_HELP.contains("runtime image"));
    }

    #[test]
    fn run_help_does_not_spawn() {
        run(&[]).unwrap();
        run(&["help".into()]).unwrap();
        run(&["help".into(), "lsp".into()]).unwrap();
        run(&["help".into(), "ide".into()]).unwrap();
        run(&["ide".into(), "--help".into()]).unwrap();
        run(&["run".into(), "--help".into()]).unwrap();
        run(&["poc".into(), "--help".into()]).unwrap();
        run(&["build".into()]).unwrap();
    }

    #[test]
    fn build_script_lists_arches_when_lsp_arch_missing() {
        let script = crate::workspace_root().join("build");
        assert!(script.is_file(), "{}", script.display());
        let output = std::process::Command::new("sh")
            .arg(&script)
            .arg("lsp")
            .output()
            .expect("spawn ./build lsp");
        assert!(!output.status.success(), "missing arch is a usage error");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains("Valid architectures"), "{text}");
        assert!(text.contains("all"), "{text}");
        assert!(text.contains("x86_64"), "{text}");
        assert!(text.contains("aarch64"), "{text}");

        let help = std::process::Command::new("sh")
            .arg(&script)
            .output()
            .expect("spawn ./build");
        assert!(help.status.success());
        let src = std::fs::read_to_string(&script).expect("read ./build");
        assert!(src.contains("cargo build -p xtask"), "{src}");
        assert!(src.contains("CARGO_TERM_PROGRESS_WHEN=never"), "{src}");
        assert!(src.contains("target/operator"), "{src}");
        assert!(src.contains("debug/xtask"), "{src}");
        assert!(!src.contains("cargo \"$@\" &"), "{src}");
        assert!(src.contains("reap_orphaned_xtask_cargo"), "{src}");
        assert!(src.contains("--force"), "{src}");
        assert!(src.contains("run_xtask lsp \"$arch\" \"$@\""), "{src}");
        assert!(
            !src.contains("exec cargo xtask"),
            "nested cargo run hangs the inner build: {src}"
        );
    }
}
