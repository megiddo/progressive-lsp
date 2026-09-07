//! Hermetic musl **core** ELFs via Docker. Both Linux triples.
//!
//! `docker build --output` extracts `progressive-lsp` to
//! `target/musl/<triple>/progressive-lsp`. Pack jobs use the same
//! [`DockerPort`] (`xtask pack` → `target/musl/<triple>/engines/<pack>/<binary>`).
//! Tests inject [`RecordingDockerPort`] and never start a daemon.
//! [`MuslBuildPlan`] is the Value object.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(test)]
use std::sync::Mutex;

use crate::check_static;
use crate::workspace_root;

pub const X86_64_MUSL: &str = "x86_64-unknown-linux-musl";
pub const AARCH64_MUSL: &str = "aarch64-unknown-linux-musl";
pub const CORE_ELF_NAME: &str = "progressive-lsp";

pub fn triples() -> &'static [&'static str] {
    &[X86_64_MUSL, AARCH64_MUSL]
}

/// Value object. Triple, docker platform, dockerfile, dest ELF, `RUST_TARGET`.
/// Darwin unit tests assert the plan without invoking docker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuslBuildPlan {
    triple: String,
    docker_platform: String,
    dockerfile: PathBuf,
    dest: PathBuf,
    rust_target: String,
    context: PathBuf,
}

impl MuslBuildPlan {
    /// Fail closed: unknown triple and missing dockerfile error.
    pub fn for_triple(root: &Path, triple: &str) -> Result<Self, String> {
        let docker_platform = match triple {
            X86_64_MUSL => "linux/amd64",
            AARCH64_MUSL => "linux/arm64",
            _ => {
                return Err(format!(
                    "unknown triple {triple}; expected {X86_64_MUSL} or {AARCH64_MUSL}"
                ))
            }
        };
        let dockerfile = root.join("docker/rust-musl.Dockerfile");
        if !dockerfile.is_file() {
            return Err(format!("missing {}", dockerfile.display()));
        }
        let dest = root
            .join("target")
            .join("musl")
            .join(triple)
            .join(CORE_ELF_NAME);
        Ok(Self {
            triple: triple.to_string(),
            docker_platform: docker_platform.to_string(),
            dockerfile,
            dest,
            rust_target: triple.to_string(),
            context: root.to_path_buf(),
        })
    }

    pub fn triple(&self) -> &str {
        &self.triple
    }

    #[cfg(test)]
    pub fn docker_platform(&self) -> &str {
        &self.docker_platform
    }

    #[cfg(test)]
    pub fn dockerfile(&self) -> &Path {
        &self.dockerfile
    }

    pub fn dest(&self) -> &Path {
        &self.dest
    }

    #[cfg(test)]
    pub fn rust_target(&self) -> &str {
        &self.rust_target
    }

    pub fn context(&self) -> &Path {
        &self.context
    }

    /// Directory passed to `docker build --output type=local,dest=…`.
    pub fn output_dir(&self) -> &Path {
        self.dest
            .parent()
            .expect("dest is target/musl/<triple>/name")
    }

    /// `docker` argv (without the program name). Tests assert extract flags.
    pub fn docker_build_args(&self) -> Vec<String> {
        vec![
            "build".into(),
            "--platform".into(),
            self.docker_platform.clone(),
            "--build-arg".into(),
            format!("RUST_TARGET={}", self.rust_target),
            "-f".into(),
            self.dockerfile.display().to_string(),
            "--output".into(),
            format!("type=local,dest={}", self.output_dir().display()),
            ".".into(),
        ]
    }
}

/// Port. Production is `docker` CLI; tests inject a recording double.
/// Core musl, pack extract (slim + full), and runtime-image tag share this Port.
pub trait DockerPort {
    /// `docker build --output` extract. `dest` is the named ELF path.
    fn extract(&self, dest: &Path, context: &Path, args: &[String]) -> Result<(), String>;

    /// `docker build -t` (runtime image). Tests record args; no daemon.
    fn tag_image(&self, context: &Path, args: &[String]) -> Result<(), String>;

    fn build_and_export(&self, plan: &MuslBuildPlan) -> Result<(), String> {
        self.extract(plan.dest(), plan.context(), &plan.docker_build_args())
    }
}

/// Production Adapter. `std::process::Command` docker; never used from crate tests.
pub struct CommandDockerPort;

impl DockerPort for CommandDockerPort {
    fn extract(&self, dest: &Path, context: &Path, args: &[String]) -> Result<(), String> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        eprintln!(
            "xtask docker: {} --output dest={}",
            args.join(" "),
            dest.display()
        );
        let status = Command::new("docker")
            .args(args)
            .env("DOCKER_BUILDKIT", "1")
            .current_dir(context)
            .status()
            .map_err(|e| {
                format!(
                    "docker not available ({e}); musl ELFs require Linux CI or a working Docker. \
                     See docs/milestones.md HOST-2 / HOST-3 notes."
                )
            })?;
        if !status.success() {
            return Err(format!(
                "docker build failed for {} (exit {status}). CI Linux must produce both musl triples.",
                dest.display()
            ));
        }
        if !dest.is_file() {
            return Err(format!(
                "extract missing {} after docker build --output",
                dest.display()
            ));
        }
        Ok(())
    }

    fn tag_image(&self, context: &Path, args: &[String]) -> Result<(), String> {
        eprintln!(
            "xtask docker: {} (tag, context={})",
            args.join(" "),
            context.display()
        );
        let status = Command::new("docker")
            .args(args)
            .env("DOCKER_BUILDKIT", "1")
            .current_dir(context)
            .status()
            .map_err(|e| {
                format!(
                    "docker not available ({e}); runtime image requires Linux CI or a working Docker. \
                     See docs/milestones.md HOST-4 notes."
                )
            })?;
        if !status.success() {
            return Err(format!(
                "docker build -t failed (exit {status}). Context must be a staging dir of prebuilt ELFs."
            ));
        }
        Ok(())
    }
}

/// Test double. Records dest/args and writes a fixture ELF — never a Docker daemon.
#[cfg(test)]
pub struct RecordingDockerPort {
    dests: Mutex<Vec<PathBuf>>,
    args: Mutex<Vec<Vec<String>>>,
}

#[cfg(test)]
impl RecordingDockerPort {
    pub fn new() -> Self {
        Self {
            dests: Mutex::new(Vec::new()),
            args: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded_dests(&self) -> Vec<PathBuf> {
        self.dests.lock().expect("RecordingDockerPort").clone()
    }

    pub fn recorded_args(&self) -> Vec<Vec<String>> {
        self.args.lock().expect("RecordingDockerPort").clone()
    }
}

#[cfg(test)]
impl Default for RecordingDockerPort {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl DockerPort for RecordingDockerPort {
    fn extract(&self, dest: &Path, _context: &Path, args: &[String]) -> Result<(), String> {
        self.dests
            .lock()
            .expect("RecordingDockerPort")
            .push(dest.to_path_buf());
        self.args
            .lock()
            .expect("RecordingDockerPort")
            .push(args.to_vec());
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        // Fixture static ELF so dest exists for check-static. Not a musl green.
        fs::write(dest, check_static::fixture_static_elf64())
            .map_err(|e| format!("write fixture {}: {e}", dest.display()))?;
        Ok(())
    }

    fn tag_image(&self, context: &Path, args: &[String]) -> Result<(), String> {
        self.dests
            .lock()
            .expect("RecordingDockerPort")
            .push(context.to_path_buf());
        self.args
            .lock()
            .expect("RecordingDockerPort")
            .push(args.to_vec());
        Ok(())
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    run_at(&workspace_root(), args, &CommandDockerPort)
}

pub fn run_at(root: &Path, args: &[String], docker: &dyn DockerPort) -> Result<(), String> {
    let targets = parse_targets(args)?;
    for triple in &targets {
        let plan = MuslBuildPlan::for_triple(root, triple)?;
        docker.build_and_export(&plan)?;
        check_static::check_path(plan.dest()).map_err(|e| {
            format!(
                "{}: {e} (refusing to pass a non-static extract; Mach-O is not a musl green)",
                plan.dest().display()
            )
        })?;
        eprintln!(
            "xtask musl: check-static PASS {} ({})",
            plan.dest().display(),
            plan.triple()
        );
    }
    Ok(())
}

fn parse_targets(args: &[String]) -> Result<Vec<String>, String> {
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
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_static::{check_bytes, fixture_macho};

    fn fixture_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let docker = dir.path().join("docker");
        fs::create_dir_all(&docker).unwrap();
        fs::write(
            docker.join("rust-musl.Dockerfile"),
            "# test dockerfile\nFROM scratch\n",
        )
        .unwrap();
        dir
    }

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
        assert!(workspace_root()
            .join("docker/rust-musl.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack.Dockerfile")
            .is_file());
    }

    #[test]
    fn musl_build_plan_is_value_object_for_both_triples_without_docker() {
        let root = fixture_root();
        let amd = MuslBuildPlan::for_triple(root.path(), X86_64_MUSL).unwrap();
        assert_eq!(amd.triple(), X86_64_MUSL);
        assert_eq!(amd.docker_platform(), "linux/amd64");
        assert_eq!(amd.rust_target(), X86_64_MUSL);
        assert_eq!(
            amd.dest(),
            root.path()
                .join("target/musl")
                .join(X86_64_MUSL)
                .join(CORE_ELF_NAME)
        );
        assert_eq!(
            amd.dockerfile(),
            root.path().join("docker/rust-musl.Dockerfile")
        );
        assert_eq!(amd.context(), root.path());
        assert_eq!(amd, amd.clone());

        let arm = MuslBuildPlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        assert_eq!(arm.docker_platform(), "linux/arm64");
        assert_eq!(arm.rust_target(), AARCH64_MUSL);
        assert_eq!(
            arm.dest(),
            root.path()
                .join("target/musl")
                .join(AARCH64_MUSL)
                .join(CORE_ELF_NAME)
        );
        assert_ne!(amd, arm);
    }

    #[test]
    fn musl_build_plan_rejects_unknown_triple_and_missing_dockerfile() {
        let root = fixture_root();
        let err = MuslBuildPlan::for_triple(root.path(), "x86_64-unknown-linux-gnu").unwrap_err();
        assert!(err.contains("unknown triple"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let missing = MuslBuildPlan::for_triple(empty.path(), X86_64_MUSL).unwrap_err();
        assert!(missing.contains("missing"), "{missing}");
    }

    #[test]
    fn musl_build_plan_docker_args_export_named_elf() {
        let root = fixture_root();
        let plan = MuslBuildPlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let args = plan.docker_build_args();
        assert_eq!(args[0], "build");
        assert!(args.contains(&"--platform".to_string()));
        assert!(args.contains(&"linux/arm64".to_string()));
        assert!(args.contains(&"--output".to_string()));
        let dest_flag = args
            .iter()
            .find(|a| a.starts_with("type=local,dest="))
            .expect("local export dest");
        assert!(
            dest_flag.ends_with(&format!("target/musl/{AARCH64_MUSL}")),
            "{dest_flag}"
        );
        assert!(args.iter().any(|a| a.contains("RUST_TARGET=")));
        assert_eq!(args.last().map(String::as_str), Some("."));
    }

    #[test]
    fn recording_docker_port_is_would_have_built_without_daemon() {
        let root = fixture_root();
        let plan = MuslBuildPlan::for_triple(root.path(), X86_64_MUSL).unwrap();
        let docker = RecordingDockerPort::new();
        docker.build_and_export(&plan).unwrap();
        let recorded = docker.recorded_dests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], plan.dest());
        assert!(plan.dest().is_file(), "fixture dest for extract pipeline");
        check_static::check_path(plan.dest()).unwrap();
        let bytes = fs::read(plan.dest()).unwrap();
        assert!(
            check_bytes(&fixture_macho()).is_err(),
            "Mach-O still refused"
        );
        assert_ne!(bytes, fixture_macho());
        let args = &docker.recorded_args()[0];
        assert!(args.iter().any(|a| a.contains("RUST_TARGET=")));
    }

    #[test]
    fn run_at_extracts_fixture_then_check_static_not_a_musl_green() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &["--target".into(), X86_64_MUSL.into()],
            &docker,
        )
        .unwrap();
        run_at(root.path(), &["--both".into()], &docker).unwrap();
        let recorded = docker.recorded_dests();
        assert_eq!(recorded.len(), 3);
        assert!(recorded[0].ends_with(CORE_ELF_NAME));
        assert!(recorded[0].to_string_lossy().contains(X86_64_MUSL));
        let triples: Vec<_> = recorded[1..]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(triples.iter().any(|p| p.contains(X86_64_MUSL)));
        assert!(triples.iter().any(|p| p.contains(AARCH64_MUSL)));
        for dest in &recorded {
            assert_eq!(dest.file_name().unwrap(), CORE_ELF_NAME);
            check_static::check_path(dest).unwrap();
        }
    }

    #[test]
    fn parse_targets_default_both_and_dedup() {
        let both = parse_targets(&[]).unwrap();
        assert_eq!(both.len(), 2);
        let dup = parse_targets(&["--both".into(), "--target".into(), X86_64_MUSL.into()]).unwrap();
        assert_eq!(dup.len(), 2);
    }

    #[test]
    fn command_docker_port_exists_for_production() {
        let _ = CommandDockerPort;
        let _ = RecordingDockerPort::default();
    }
}
