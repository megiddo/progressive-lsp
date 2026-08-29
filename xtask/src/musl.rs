//! Hermetic musl builds via Docker. Both Linux triples.

use std::process::Command;

use crate::workspace_root;

pub const X86_64_MUSL: &str = "x86_64-unknown-linux-musl";
pub const AARCH64_MUSL: &str = "aarch64-unknown-linux-musl";

pub fn triples() -> &'static [&'static str] {
    &[X86_64_MUSL, AARCH64_MUSL]
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut targets = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                i += 1;
                let t = args.get(i).ok_or("--target requires a triple")?;
                if !triples().contains(&t.as_str()) {
                    return Err(format!(
                        "unsupported musl triple {t}; expected {X86_64_MUSL} or {AARCH64_MUSL}"
                    ));
                }
                targets.push(t.clone());
            }
            "--both" => {
                targets.extend(triples().iter().map(|s| (*s).to_string()));
            }
            other => return Err(format!("unknown musl flag: {other}")),
        }
        i += 1;
    }
    if targets.is_empty() {
        targets.extend(triples().iter().map(|s| (*s).to_string()));
    }
    targets.sort();
    targets.dedup();
    for triple in &targets {
        build_one(triple)?;
    }
    Ok(())
}

fn build_one(triple: &str) -> Result<(), String> {
    let root = workspace_root();
    let dockerfile = root.join("docker/rust-musl.Dockerfile");
    if !dockerfile.is_file() {
        return Err(format!("missing {}", dockerfile.display()));
    }
    let platform = match triple {
        X86_64_MUSL => "linux/amd64",
        AARCH64_MUSL => "linux/arm64",
        _ => return Err(format!("unknown triple {triple}")),
    };
    eprintln!("xtask musl: docker build --platform {platform} --build-arg RUST_TARGET={triple}");
    let status = Command::new("docker")
        .args([
            "build",
            "--platform",
            platform,
            "--build-arg",
            &format!("RUST_TARGET={triple}"),
            "-f",
            dockerfile.to_str().unwrap(),
            ".",
        ])
        .current_dir(&root)
        .status()
        .map_err(|e| {
            format!(
                "docker not available ({e}); musl ELFs require Linux CI or a working Docker. \
                 See docs/milestones.md M0 notes."
            )
        })?;
    if !status.success() {
        return Err(format!(
            "docker build failed for {triple} (exit {status}). CI Linux must produce both musl triples."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_triples_are_named() {
        assert_eq!(triples().len(), 2);
        assert!(triples().contains(&X86_64_MUSL));
        assert!(triples().contains(&AARCH64_MUSL));
    }

    #[test]
    fn rejects_unknown_triple_and_flag() {
        assert!(run(&["--target".into(), "x86_64-unknown-linux-gnu".into()]).is_err());
        assert!(run(&["--nope".into()]).is_err());
        assert!(run(&["--target".into()]).is_err());
    }

    #[test]
    fn dockerfile_exists() {
        assert!(workspace_root().join("docker/rust-musl.Dockerfile").is_file());
        assert!(workspace_root()
            .join("docker/engine-pack.Dockerfile")
            .is_file());
    }
}
