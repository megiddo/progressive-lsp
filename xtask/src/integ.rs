//! `./build integ` — integration smoke (bootstrap + tam + discover-container).

use std::process::Command;

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

pub fn run(args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let script = root.join("integration/harness/run-integ-smoke.sh");
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }
    let mut cmd = Command::new("sh");
    cmd.arg(&script);
    cmd.args(args);
    cmd.current_dir(&root);
    eprintln!("xtask integ: {}", script.display());
    let status = cmd
        .status()
        .map_err(|e| format!("run-integ-smoke.sh: {e}"))?;
    if !status.success() {
        return Err(format!("integration smoke failed ({status})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_script_path_exists() {
        let root = workspace_root();
        assert!(root.join("integration/harness/run-integ-smoke.sh").is_file());
    }
}
