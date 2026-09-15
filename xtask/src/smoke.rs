//! PROD-5 smoke tier 0 — maintainer gate without `--cache-fill`.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a workspace member")
        .to_path_buf()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut skip_docker = false;
    let mut skip_clangd = false;
    for a in args {
        match a.as_str() {
            "--skip-docker" => skip_docker = true,
            "--skip-clangd-pull" => skip_clangd = true,
            "--help" | "-h" => {
                eprintln!(
                    "usage: cargo xtask smoke [--skip-docker] [--skip-clangd-pull]\n\
                     tier 0: docker (optional skip), local-artifacts manifest, xtask tests via caller"
                );
                return Ok(());
            }
            other => return Err(format!("unknown smoke argument: {other}")),
        }
    }

    let root = workspace_root();

    if !skip_docker {
        let out = Command::new("docker")
            .arg("info")
            .output()
            .map_err(|e| format!("docker info: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "docker info failed (exit {}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        eprintln!("xtask smoke: docker info OK");
    }

    let manifest = root.join("target/local-artifacts/manifest.json");
    if manifest.is_file() {
        eprintln!("xtask smoke: found {}", manifest.display());
    } else {
        eprintln!(
            "xtask smoke: warn: no {} (optional until PROD-2 push)",
            manifest.display()
        );
    }

    if !skip_clangd {
        let script = root.join("scripts/verify-clangd-cache-pull.sh");
        if script.is_file() {
            let status = Command::new("sh")
                .arg(&script)
                .current_dir(&root)
                .env(
                    "CARGO_TARGET_DIR",
                    std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| {
                        root.join("target").display().to_string()
                    }),
                )
                .status()
                .map_err(|e| format!("verify-clangd-cache-pull: {e}"))?;
            if !status.success() {
                return Err("clangd cache pull verify failed (run cache push first)".into());
            }
        } else {
            return Err(format!("missing {}", script.display()));
        }
    }

    eprintln!("xtask smoke: tier 0 OK (run `cargo test -p xtask` / `-p poc-ide` in CI)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_help() {
        run(&["--help".into()]).unwrap();
    }
}
