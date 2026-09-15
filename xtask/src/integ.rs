//! `./build integ` — integration smoke (bootstrap + tam + discover-container).

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::result_tree::{Outcome, ResultTree};
use crate::workspace_root;

/// Docker runtime image for poc-ide container / integ (dogfood packs).
pub fn ensure_dogfood_runtime_image() -> Result<(), String> {
    let inspect = Command::new("docker")
        .args(["image", "inspect", "progressive-lsp-runtime:local"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("docker image inspect: {e}"))?;
    if inspect.success() {
        return Ok(());
    }
    let root = workspace_root();
    let build = root.join("build");
    if !build.is_file() {
        return Err(format!("missing {}", build.display()));
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        return Err(format!(
            "no default lsp arch on this host; build image manually: ./build lsp aarch64 --flavor dogfood"
        ));
    };
    eprintln!(
        "xtask: progressive-lsp-runtime:local missing — running ./build lsp {arch} --flavor dogfood"
    );
    let status = Command::new(&build)
        .args(["lsp", arch, "--flavor", "dogfood"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("./build lsp: {e}"))?;
    if !status.success() {
        return Err(format!("./build lsp {arch} --flavor dogfood failed ({status})"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct TamReport {
    result: String,
    #[serde(default)]
    suite_id: Option<String>,
    #[serde(default)]
    rows: Vec<TamRow>,
}

#[derive(Deserialize)]
struct TamRow {
    method: String,
    result: String,
}

#[derive(Deserialize)]
struct DiscoverReport {
    result: String,
    #[serde(default)]
    corpus: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

fn outcome_from_report(s: &str, exit_failed: bool) -> Outcome {
    match s {
        "pass" if !exit_failed => Outcome::Pass,
        "fail" => Outcome::Fail,
        "skip" => Outcome::Skip,
        _ if exit_failed => Outcome::Fail,
        _ => Outcome::Skip,
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn run_bootstrap(root: &Path) -> Result<(), String> {
    let bootstrap = root.join("integration/harness/bootstrap.sh");
    let script = format!(
        ". \"{bootstrap}\"\n\
         plsp_export_cargo_env\n\
         if plsp_docker_ok; then\n\
           plsp_ensure_runtime_image || true\n\
           plsp_ensure_musl_controller || true\n\
         fi\n",
        bootstrap = bootstrap.display()
    );
    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(root)
        .env("PLSP_REPO_ROOT", root)
        .status()
        .map_err(|e| format!("integ bootstrap: {e}"))?;
    if !status.success() {
        return Err(format!("integ bootstrap failed ({status})"));
    }
    Ok(())
}

fn run_harness_script(root: &Path, script: &Path, args: &[&str]) -> (Outcome, Option<String>) {
    let output = match Command::new("sh")
        .arg(script)
        .args(args)
        .current_dir(root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return (Outcome::Fail, Some(format!("spawn: {e}"))),
    };
    let exit_fail = !output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = if exit_fail && !stderr.trim().is_empty() {
        Some(stderr.trim().to_string())
    } else {
        None
    };
    if exit_fail {
        (Outcome::Fail, detail)
    } else {
        (Outcome::Pass, None)
    }
}

fn append_tam_tree(tree: &mut ResultTree, root: &Path, suite_id: &str, script_outcome: Outcome) {
    let out_dir = root.join("integration/out");
    let report_path = out_dir.join(format!("tam-{suite_id}-report.json"));
    let report: TamReport = match read_json(&report_path) {
        Some(r) => r,
        None => {
            let outcome = script_outcome.merge(Outcome::Fail);
            tree.push_detail(
                1,
                "tam-run",
                outcome,
                Some(format!("missing or invalid {}", report_path.display())),
            );
            return;
        }
    };
    let suite_label = report
        .suite_id
        .as_deref()
        .unwrap_or(suite_id)
        .to_string();
    let mut group = script_outcome.merge(outcome_from_report(
        &report.result,
        script_outcome == Outcome::Fail,
    ));
    for row in &report.rows {
        group = group.merge(outcome_from_report(&row.result, false));
    }
    tree.push(1, "tam-run", group);
    tree.push(2, suite_label, group);
    for row in &report.rows {
        let row_out = outcome_from_report(&row.result, false);
        tree.push(2, row.method.clone(), row_out);
    }
    if group == Outcome::Fail {
        let trace = out_dir.join(format!("tam-{suite_id}-trace.log"));
        tree.push_detail(
            2,
            "trace",
            Outcome::Fail,
            Some(format!("see {}", trace.display())),
        );
    }
}

fn append_discover_tree(tree: &mut ResultTree, root: &Path, script_outcome: Outcome) {
    let report_path = root.join("integration/out/discover-container-report.json");
    let report: DiscoverReport = match read_json(&report_path) {
        Some(r) => r,
        None => {
            tree.push_detail(
                1,
                "discover-container",
                Outcome::Fail,
                Some(format!("missing or invalid {}", report_path.display())),
            );
            return;
        }
    };
    let label = report
        .corpus
        .clone()
        .unwrap_or_else(|| "discover-container".into());
    let mut group = outcome_from_report(&report.result, script_outcome == Outcome::Fail);
    group = group.merge(script_outcome);
    let detail = report.notes.filter(|_| group == Outcome::Fail);
    tree.push_detail(1, "discover-container", group, detail);
    tree.push(2, label, group);
    if group == Outcome::Fail {
        let trace = root.join("integration/out/discover-container-trace.log");
        tree.push_detail(
            2,
            "trace",
            Outcome::Fail,
            Some(format!("see {}", trace.display())),
        );
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let suite_id = args.first().map(String::as_str).unwrap_or("hermetic-discover-java");
    let harness = root.join("integration/harness");
    let tam_script = harness.join("run-tam.sh");
    let discover_script = harness.join("run-discover-container.sh");
    if !tam_script.is_file() || !discover_script.is_file() {
        return Err("missing integration harness scripts".into());
    }

    run_bootstrap(&root)?;

    let mut tree = ResultTree::new("integ");

    let (tam_out, _) = run_harness_script(&root, &tam_script, &[suite_id]);
    append_tam_tree(&mut tree, &root, suite_id, tam_out);

    let (disc_out, _) = run_harness_script(&root, &discover_script, &[]);
    append_discover_tree(&mut tree, &root, disc_out);

    tree.print();
    if tree.failed() {
        return Err("integration smoke failed (see tree above)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_script_paths_exist() {
        let root = workspace_root();
        assert!(root.join("integration/harness/run-tam.sh").is_file());
        assert!(root.join("integration/harness/run-discover-container.sh").is_file());
    }

    #[test]
    fn outcome_from_report_maps_fail() {
        assert_eq!(outcome_from_report("fail", false), Outcome::Fail);
        assert_eq!(outcome_from_report("pass", true), Outcome::Fail);
    }
}
