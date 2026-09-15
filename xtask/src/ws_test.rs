//! `./build test` — workspace unit tests (fixed CARGO_TARGET_DIR, harness included).

use std::path::PathBuf;
use std::process::Command;

use crate::workspace_root;

fn workspace_target(root: &PathBuf) -> PathBuf {
    root.join("target")
}

fn cargo_cmd(root: &PathBuf) -> Command {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root);
    cmd.env("CARGO_TARGET_DIR", workspace_target(root));
    cmd.env("CARGO_HOME", root.join(".cargo-home"));
    cmd.env("CARGO_TERM_PROGRESS_WHEN", "never");
    cmd
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let target = workspace_target(&root);
    eprintln!("xtask test: CARGO_TARGET_DIR={}", target.display());

    let steps: &[(&str, &[&str])] = &[
        (
            "core crates",
            &[
                "test",
                "-p",
                "progressive-lsp-resolve",
                "-p",
                "progressive-lsp-protocol",
                "-p",
                "progressive-lsp-control",
                "-p",
                "progressive-lsp-types-cache",
            ],
        ),
        (
            "progressive-lsp + seams_extended",
            &["test", "-p", "progressive-lsp", "--test", "seams_extended"],
        ),
        (
            "integration harness",
            &[
                "test",
                "--manifest-path",
                "integration/harness/Cargo.toml",
            ],
        ),
    ];

    for (label, args) in steps {
        eprintln!("xtask test: {label}");
        let status = cargo_cmd(&root)
            .args(*args)
            .status()
            .map_err(|e| format!("cargo {label}: {e}"))?;
        if !status.success() {
            return Err(format!("cargo test ({label}) failed ({status})"));
        }
    }
    eprintln!("xtask test: OK");
    Ok(())
}
