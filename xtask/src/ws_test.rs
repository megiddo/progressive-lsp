//! `./build test` — workspace unit tests (fixed CARGO_TARGET_DIR, harness included).

use std::path::PathBuf;
use std::process::Command;

use crate::result_tree::{tail_output, Outcome, ResultTree};
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

fn run_cargo_test(root: &PathBuf, args: &[&str]) -> (Outcome, Option<String>) {
    let output = match cargo_cmd(root).args(args).output() {
        Ok(o) => o,
        Err(e) => return (Outcome::Fail, Some(format!("spawn: {e}"))),
    };
    if output.status.success() {
        (Outcome::Pass, None)
    } else {
        let detail = tail_output(&output.stdout, &output.stderr, 10);
        let detail = if detail.is_empty() {
            Some(format!("exit {}", output.status))
        } else {
            Some(detail)
        };
        (Outcome::Fail, detail)
    }
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let mut tree = ResultTree::new("test");

    let groups: &[(&str, &[(&str, &[&str])])] = &[
        (
            "core crates",
            &[
                (
                    "progressive-lsp-resolve",
                    &["test", "-p", "progressive-lsp-resolve"],
                ),
                (
                    "progressive-lsp-protocol",
                    &["test", "-p", "progressive-lsp-protocol"],
                ),
                (
                    "progressive-lsp-control",
                    &["test", "-p", "progressive-lsp-control"],
                ),
                (
                    "progressive-lsp-types-cache",
                    &["test", "-p", "progressive-lsp-types-cache"],
                ),
            ],
        ),
        (
            "seams_extended",
            &[(
                "seams_extended",
                &["test", "-p", "progressive-lsp", "--test", "seams_extended"],
            )],
        ),
        (
            "integration harness",
            &[(
                "plsp-it1 unit tests",
                &["test", "--manifest-path", "integration/harness/Cargo.toml"],
            )],
        ),
    ];

    for (group, leaves) in groups {
        let mut group_outcome = Outcome::Pass;
        let mut leaf_results = Vec::new();
        for (leaf_name, args) in *leaves {
            let (outcome, detail) = run_cargo_test(&root, args);
            group_outcome = group_outcome.merge(outcome);
            leaf_results.push((*leaf_name, outcome, detail));
        }
        tree.push(1, *group, group_outcome);
        for (name, outcome, detail) in leaf_results {
            tree.push_detail(2, name, outcome, detail);
        }
    }

    tree.print();
    if tree.failed() {
        return Err("unit tests failed (see tree above)".into());
    }
    Ok(())
}
