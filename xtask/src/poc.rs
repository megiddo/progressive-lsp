//! `xtask poc`: build `progressive-lsp`, then run poc-ide against that artifact.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_root;

/// Value object. Argv after `xtask poc`. Split at `--`; the spawn shell is not unit-tested.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PocArgs {
    Help,
    Run { forwarded: Vec<String> },
}

#[cfg(test)]
pub fn run(args: &[String]) -> Result<(), String> {
    match parse_poc_args(args)? {
        PocArgs::Help => {
            print_poc_help();
            Ok(())
        }
        PocArgs::Run { forwarded } => spawn_poc(&forwarded),
    }
}

/// Split at the first `--`. Left side is xtask flags (`-h` / `--help` only).
/// Right side is forwarded to `poc-ide`. Leftover flags without `--` error.
#[cfg(test)]
pub fn parse_poc_args(args: &[String]) -> Result<PocArgs, String> {
    let dash = args.iter().position(|a| a == "--");
    let (own, forwarded) = match dash {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (args, Vec::new()),
    };
    for flag in own {
        match flag.as_str() {
            "-h" | "--help" => return Ok(PocArgs::Help),
            other => {
                return Err(format!(
                    "unknown poc flag: {other} (forward poc-ide args after --)"
                ));
            }
        }
    }
    Ok(PocArgs::Run { forwarded })
}

#[cfg(test)]
pub fn print_poc_help() {
    crate::cli::print_help(crate::cli::HelpTopic::Run);
}

fn cargo_bin() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Nested cargo under `cargo run -p xtask` inherits the jobserver and can
/// wait forever with no `Compiling` lines. Drop those env vars.
const NESTED_CARGO_DROP_ENV: &[&str] = &["MAKEFLAGS", "MFLAGS", "CARGO_MAKEFLAGS"];

fn cargo_cmd() -> Command {
    let mut cmd = Command::new(cargo_bin());
    for key in NESTED_CARGO_DROP_ENV {
        cmd.env_remove(*key);
    }
    // TTY progress bar suppresses `Compiling` lines. Stream them instead.
    cmd.env("CARGO_TERM_PROGRESS_WHEN", "never");
    cmd.env("CARGO_TERM_COLOR", "always");
    cmd
}

/// Native `progressive-lsp` for this machine (what poc-ide spawns without `--container`).
pub(crate) fn build_native_controller() -> Result<(), String> {
    let root = workspace_root();
    eprintln!("xtask: cargo build --bin progressive-lsp");
    let status = cargo_cmd()
        .args(["build", "--bin", "progressive-lsp"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo build --bin progressive-lsp: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build --bin progressive-lsp failed ({status})"
        ));
    }
    Ok(())
}

/// POC IDE binary only. Does not start the window.
pub(crate) fn build_poc_bin() -> Result<(), String> {
    let root = workspace_root();
    eprintln!("xtask: cargo build -p poc-ide");
    let status = cargo_cmd()
        .args(["build", "-p", "poc-ide"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo build -p poc-ide: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p poc-ide failed ({status})"));
    }
    Ok(())
}

pub(crate) fn run_poc_ide(forwarded: &[String]) -> Result<(), String> {
    spawn_poc(forwarded)
}

fn spawn_poc(forwarded: &[String]) -> Result<(), String> {
    let root = workspace_root();
    build_native_controller()?;
    let artifact = serve_artifact(&root)?;
    eprintln!(
        "xtask run: PROGRESSIVE_LSP={} cargo run -p poc-ide",
        artifact.display()
    );
    let mut cmd = cargo_cmd();
    cmd.args(["run", "-p", "poc-ide"])
        .current_dir(&root)
        .env("PROGRESSIVE_LSP", &artifact);
    if !forwarded.is_empty() {
        cmd.arg("--");
        cmd.args(forwarded);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("cargo run -p poc-ide: {e}"))?;
    if !status.success() {
        return Err(format!("cargo run -p poc-ide failed ({status})"));
    }
    Ok(())
}

fn serve_artifact(root: &Path) -> Result<PathBuf, String> {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let mut path = target_dir.join("debug").join("progressive-lsp");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    if !path.is_file() {
        return Err(format!("missing {}", path.display()));
    }
    path.canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<PocArgs, String> {
        parse_poc_args(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn poc_args_value_object_empty_is_run_with_no_forward() {
        assert_eq!(parse(&[]).unwrap(), PocArgs::Run { forwarded: vec![] });
    }

    #[test]
    fn parse_bare_dash_dash_forwards_nothing() {
        assert_eq!(parse(&["--"]).unwrap(), PocArgs::Run { forwarded: vec![] });
    }

    #[test]
    fn parse_forwards_after_dash_dash() {
        assert_eq!(
            parse(&["--", "--folder", "fixtures/java-multi"]).unwrap(),
            PocArgs::Run {
                forwarded: vec!["--folder".into(), "fixtures/java-multi".into()]
            }
        );
    }

    #[test]
    fn parse_forwards_help_after_dash_dash() {
        assert_eq!(
            parse(&["--", "--help"]).unwrap(),
            PocArgs::Run {
                forwarded: vec!["--help".into()]
            }
        );
    }

    #[test]
    fn parse_help_before_dash_dash() {
        assert_eq!(parse(&["--help"]).unwrap(), PocArgs::Help);
        assert_eq!(parse(&["-h"]).unwrap(), PocArgs::Help);
        assert_eq!(
            parse(&["--help", "--", "--folder", "x"]).unwrap(),
            PocArgs::Help
        );
    }

    #[test]
    fn poc_args_value_object_leftover_without_dash_dash_errors() {
        let err = parse(&["--folder", "DIR"]).unwrap_err();
        assert!(err.contains("unknown poc flag: --folder"), "{err}");
        assert!(err.contains("after --"), "{err}");
    }

    #[test]
    fn parse_unknown_flag_before_dash_dash_errors() {
        let err = parse(&["--release", "--", "--folder", "DIR"]).unwrap_err();
        assert!(err.contains("unknown poc flag: --release"), "{err}");
    }

    #[test]
    fn run_help_does_not_spawn() {
        run(&["--help".into()]).unwrap();
        print_poc_help();
    }

    #[test]
    fn nested_cargo_drops_jobserver_env() {
        assert_eq!(
            NESTED_CARGO_DROP_ENV,
            &["MAKEFLAGS", "MFLAGS", "CARGO_MAKEFLAGS"]
        );
        let _ = cargo_cmd();
    }
}
