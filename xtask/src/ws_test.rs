//! `./build test` — workspace unit tests (fixed CARGO_TARGET_DIR, harness included).

use std::path::PathBuf;
use std::process::Command;

use crate::result_tree::{
    cargo_failure_snippet, failed_crates_from_cargo_output, parse_cargo_test_by_package,
    parse_cargo_test_totals, Outcome, ResultTree,
};
use crate::workspace_root;

struct TestRun {
    outcome: Outcome,
    passed: u32,
    failed: u32,
    detail: Option<String>,
    combined: String,
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
                combined: String::new(),
            };
        }
    };
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (passed, failed) = parse_cargo_test_totals(&combined);
    let ok = output.status.success() && failed == 0;
    let detail = if ok {
        None
    } else {
        Some(cargo_failure_snippet(
            &output.stdout,
            &output.stderr,
            12,
        ))
    };
    TestRun {
        outcome: if ok { Outcome::Pass } else { Outcome::Fail },
        passed,
        failed,
        detail,
        combined,
    }
}

/// Workspace library crates for one `cargo test` (xtask excluded — run `cargo test -p xtask` separately).
const WORKSPACE_TEST_ARGS: &[&str] = &[
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
    "-p",
    "progressive-lsp-protocol",
    "-p",
    "progressive-lsp-control",
    "-p",
    "progressive-lsp-types-cache",
    "-p",
    "progressive-lsp-engine",
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
    "-p",
    "progressive-lsp",
    "-p",
    "poc-ide",
];

fn workspace_package_order() -> Vec<&'static str> {
    WORKSPACE_TEST_ARGS
        .windows(2)
        .filter_map(|w| (w[0] == "-p").then_some(w[1]))
        .collect()
}

fn push_workspace_breakdown(tree: &mut ResultTree, ws: &TestRun) {
    tree.push_counts(
        1,
        "workspace",
        ws.outcome,
        ws.passed,
        ws.failed,
        ws.detail.clone(),
    );
    let by_pkg = parse_cargo_test_by_package(&ws.combined);
    for pkg in workspace_package_order() {
        let Some((p, f)) = by_pkg.get(pkg) else {
            continue;
        };
        if *p == 0 && *f == 0 {
            continue;
        }
        let outcome = if *f > 0 {
            Outcome::Fail
        } else {
            Outcome::Pass
        };
        tree.push_counts(2, pkg, outcome, *p, *f, None);
    }
    for (pkg, (p, f)) in &by_pkg {
        if workspace_package_order().contains(&pkg.as_str()) {
            continue;
        }
        if *p == 0 && *f == 0 {
            continue;
        }
        let outcome = if *f > 0 {
            Outcome::Fail
        } else {
            Outcome::Pass
        };
        tree.push_counts(2, pkg.clone(), outcome, *p, *f, None);
    }
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let mut tree = ResultTree::new("test");

    let ws = run_cargo_test(&root, WORKSPACE_TEST_ARGS);
    push_workspace_breakdown(&mut tree, &ws);
    if ws.outcome == Outcome::Fail {
        for crate_name in failed_crates_from_cargo_output(&ws.combined) {
            let args = ["test", "-p", crate_name.as_str()];
            let sub = run_cargo_test(&root, &args);
            tree.push_detail(
                2,
                format!("{crate_name} (detail)"),
                sub.outcome,
                sub.detail,
            );
        }
    }

    let harness = run_cargo_test(
        &root,
        &["test", "--manifest-path", "integration/harness/Cargo.toml"],
    );
    tree.push_counts(
        1,
        "integration harness",
        harness.outcome,
        harness.passed,
        harness.failed,
        harness.detail.clone(),
    );
    let harness_pkgs = parse_cargo_test_by_package(&harness.combined);
    if let Some((p, f)) = harness_pkgs.get("plsp-it1") {
        let outcome = if *f > 0 {
            Outcome::Fail
        } else {
            Outcome::Pass
        };
        tree.push_counts(2, "plsp-it1", outcome, *p, *f, None);
    }

    tree.print();
    if tree.failed() {
        return Err("unit tests failed (see tree above)".into());
    }
    Ok(())
}
