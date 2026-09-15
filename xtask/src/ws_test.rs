//! `./build test` — workspace unit tests (fixed CARGO_TARGET_DIR, harness included).

use std::path::PathBuf;
use std::process::Command;

use crate::result_tree::{parse_cargo_test_totals, tail_output, Outcome, ResultTree};
use crate::workspace_root;

struct TestRun {
    outcome: Outcome,
    passed: u32,
    failed: u32,
    detail: Option<String>,
}

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

fn run_cargo_test(root: &PathBuf, args: &[&str]) -> TestRun {
    let output = match cargo_cmd(root).args(args).output() {
        Ok(o) => o,
        Err(e) => {
            return TestRun {
                outcome: Outcome::Fail,
                passed: 0,
                failed: 0,
                detail: Some(format!("spawn: {e}")),
            };
        }
    };
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (passed, failed) = parse_cargo_test_totals(&combined);
    if output.status.success() && failed == 0 {
        TestRun {
            outcome: Outcome::Pass,
            passed,
            failed,
            detail: None,
        }
    } else {
        let detail = tail_output(&output.stdout, &output.stderr, 10);
        let detail = if detail.is_empty() {
            Some(format!("exit {}", output.status))
        } else {
            Some(detail)
        };
        TestRun {
            outcome: Outcome::Fail,
            passed,
            failed,
            detail,
        }
    }
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let mut tree = ResultTree::new("test");

    // One `cargo test` batch per group; leaves are workspace packages (not individual #[test] names).
    let groups: &[(&str, &[(&str, &[&str])])] = &[
        (
            "core & infra",
            &[(
                "core, workspace, watch, index, resolve, script, plugin, install, log",
                &[
                    "test",
                    "-p",
                    "progressive-lsp-core",
                    "-p",
                    "progressive-lsp-workspace",
                    "-p",
                    "progressive-lsp-watch",
                    "-p",
                    "progressive-lsp-index",
                    "-p",
                    "progressive-lsp-resolve",
                    "-p",
                    "progressive-lsp-script",
                    "-p",
                    "progressive-lsp-plugin",
                    "-p",
                    "progressive-lsp-install",
                    "-p",
                    "progressive-lsp-log",
                ],
            )],
        ),
        (
            "protocol & control",
            &[(
                "protocol, control, types-cache",
                &[
                    "test",
                    "-p",
                    "progressive-lsp-protocol",
                    "-p",
                    "progressive-lsp-control",
                    "-p",
                    "progressive-lsp-types-cache",
                ],
            )],
        ),
        (
            "engine",
            &[("progressive-lsp-engine", &["test", "-p", "progressive-lsp-engine"])],
        ),
        (
            "language factories",
            &[(
                "lang-* (C, C++, C#, Go, Java, …)",
                &[
                    "test",
                    "-p",
                    "progressive-lsp-lang-java",
                    "-p",
                    "progressive-lsp-lang-php",
                    "-p",
                    "progressive-lsp-lang-html",
                    "-p",
                    "progressive-lsp-lang-css",
                    "-p",
                    "progressive-lsp-lang-javascript",
                    "-p",
                    "progressive-lsp-lang-go",
                    "-p",
                    "progressive-lsp-lang-zig",
                    "-p",
                    "progressive-lsp-lang-python",
                    "-p",
                    "progressive-lsp-lang-rust",
                    "-p",
                    "progressive-lsp-lang-c",
                    "-p",
                    "progressive-lsp-lang-cpp",
                    "-p",
                    "progressive-lsp-lang-csharp",
                ],
            )],
        ),
        (
            "progressive-lsp",
            &[(
                "lib, integration, seams_extended",
                &["test", "-p", "progressive-lsp"],
            )],
        ),
        (
            "poc-ide",
            &[("poc-ide library", &["test", "-p", "poc-ide"])],
        ),
        (
            "xtask",
            &[("xtask", &["test", "-p", "xtask"])],
        ),
        (
            "integration harness",
            &[(
                "plsp-it1",
                &["test", "--manifest-path", "integration/harness/Cargo.toml"],
            )],
        ),
    ];

    for (group, leaves) in groups {
        let mut group_outcome = Outcome::Pass;
        let mut group_passed = 0u32;
        let mut group_failed = 0u32;
        let mut leaf_results = Vec::new();
        for (leaf_name, args) in *leaves {
            let run = run_cargo_test(&root, args);
            group_outcome = group_outcome.merge(run.outcome);
            group_passed += run.passed;
            group_failed += run.failed;
            leaf_results.push((*leaf_name, run));
        }
        tree.push_counts(
            1,
            *group,
            group_outcome,
            group_passed,
            group_failed,
            None,
        );
        for (name, run) in leaf_results {
            tree.push_counts(
                2,
                name,
                run.outcome,
                run.passed,
                run.failed,
                run.detail,
            );
        }
    }

    tree.print();
    if tree.failed() {
        return Err("unit tests failed (see tree above)".into());
    }
    Ok(())
}
