//! `xtask poc`: build `progressive-lsp`, then run poc-ide against that artifact.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_root;

/// Value object. Argv after `xtask poc`. Split at `--`; the spawn shell is not unit-tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PocArgs {
    Help,
    Run { forwarded: Vec<String> },
}

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

pub fn print_poc_help() {
    eprintln!(
        "\
xtask poc [-- <poc-ide args>...]
  Build progressive-lsp, then cargo run -p poc-ide with PROGRESSIVE_LSP
  set to that artifact. Args after -- are forwarded to poc-ide.

  cargo xtask poc
  cargo xtask poc -- --folder DIR
  cargo xtask poc -- --file PATH

  This is the supported proof launch. Bare `cargo run -p poc-ide` does
  not rebuild progressive-lsp and may spawn a stale binary.
"
    );
}

fn spawn_poc(forwarded: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    eprintln!("xtask poc: cargo build --bin progressive-lsp");
    let status = Command::new(&cargo)
        .args(["build", "--bin", "progressive-lsp"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo build --bin progressive-lsp: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build --bin progressive-lsp failed ({status})"
        ));
    }
    let artifact = serve_artifact(&root)?;
    eprintln!(
        "xtask poc: PROGRESSIVE_LSP={} cargo run -p poc-ide",
        artifact.display()
    );
    let mut cmd = Command::new(&cargo);
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
}
