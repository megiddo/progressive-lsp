//! Runtime image: copy prebuilt core + slim + optional full packs into `/opt/plsp`.
//!
//! Context is a staging dir of already-extracted ELFs under
//! `target/runtime-image/<triple>/` — not the git tree, and never a
//! cargo/LLVM/clang/zig/go compile. Tests inject [`RecordingDockerPort`]
//! and never start a daemon. [`RuntimeImagePlan`] is the Value object.

use std::fs;
use std::path::{Path, PathBuf};

use progressive_lsp_core::PrefixLayout;
use progressive_lsp_engine::{binary_name_for_pack, full_pack_names, slim_pack_names};

use crate::musl::{
    triples, CommandDockerPort, DockerPort, AARCH64_MUSL, CORE_ELF_NAME, X86_64_MUSL,
};
use crate::workspace_root;

/// Locked in poc-ide `RUNTIME_IMAGE`. Do not invent a second name.
pub const IMAGE_TAG: &str = "progressive-lsp-runtime:local";

/// Prefix inside the image — not Mac `~/.progressivelsp`.
pub const IMAGE_PREFIX: &str = "/opt/plsp";

pub const DOCKERFILE_REL: &str = "docker/runtime.Dockerfile";

const STAGING_KEEP: &str = ".keep";

/// One pack ELF to copy (slim required, full optional). Value object (field of [`RuntimeImagePlan`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackImageCopy {
    pack: String,
    binary: String,
    src: PathBuf,
    required: bool,
}

impl PackImageCopy {
    #[cfg(test)]
    pub fn pack(&self) -> &str {
        &self.pack
    }

    #[cfg(test)]
    pub fn binary(&self) -> &str {
        &self.binary
    }

    #[cfg(test)]
    pub fn src(&self) -> &Path {
        &self.src
    }

    #[cfg(test)]
    pub fn required(&self) -> bool {
        self.required
    }

    pub fn image_rel(&self) -> PathBuf {
        PathBuf::from("engines").join(&self.pack).join(&self.binary)
    }
}

fn pack_required_on_triple(pack: &str, _triple: &str) -> bool {
    slim_pack_names().contains(&pack)
}

/// Value object. Platform, triple, dockerfile, core dest, pack dests, image tag.
/// Darwin unit tests assert the plan without invoking docker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeImagePlan {
    triple: String,
    docker_platform: String,
    dockerfile: PathBuf,
    core_dest: PathBuf,
    pack_dests: Vec<PackImageCopy>,
    image_tag: String,
    /// Image prefix. Tests read via [`Self::prefix`]; production stages under `staging/prefix`.
    #[cfg_attr(not(test), allow(dead_code))]
    prefix: String,
    staging: PathBuf,
}

impl RuntimeImagePlan {
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
        let dockerfile = root.join(DOCKERFILE_REL);
        if !dockerfile.is_file() {
            return Err(format!("missing {}", dockerfile.display()));
        }
        let musl_root = root.join("target").join("musl").join(triple);
        let core_dest = musl_root.join(CORE_ELF_NAME);
        let pack_dests = full_pack_names()
            .iter()
            .map(|pack| {
                let binary = binary_name_for_pack(pack)
                    .ok_or_else(|| format!("unknown pack {pack}"))?
                    .to_string();
                let required = pack_required_on_triple(pack, triple);
                Ok(PackImageCopy {
                    pack: (*pack).to_string(),
                    binary: binary.clone(),
                    src: musl_root.join("engines").join(pack).join(&binary),
                    required,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            triple: triple.to_string(),
            docker_platform: docker_platform.to_string(),
            dockerfile,
            core_dest,
            pack_dests,
            image_tag: IMAGE_TAG.to_string(),
            prefix: IMAGE_PREFIX.to_string(),
            staging: root.join("target").join("runtime-image").join(triple),
        })
    }

    #[cfg(test)]
    pub fn triple(&self) -> &str {
        &self.triple
    }

    pub fn docker_platform(&self) -> &str {
        &self.docker_platform
    }

    #[cfg(test)]
    pub fn dockerfile(&self) -> &Path {
        &self.dockerfile
    }

    #[cfg(test)]
    pub fn core_dest(&self) -> &Path {
        &self.core_dest
    }

    #[cfg(test)]
    pub fn pack_dests(&self) -> &[PackImageCopy] {
        &self.pack_dests
    }

    pub fn image_tag(&self) -> &str {
        &self.image_tag
    }

    #[cfg(test)]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    #[cfg(test)]
    pub fn staging(&self) -> &Path {
        &self.staging
    }

    pub fn context(&self) -> &Path {
        &self.staging
    }

    /// `docker` argv (without the program name). Tests assert `-t` and platform.
    pub fn docker_build_args(&self) -> Vec<String> {
        vec![
            "build".into(),
            "--platform".into(),
            self.docker_platform.clone(),
            "-f".into(),
            self.dockerfile.display().to_string(),
            "-t".into(),
            self.image_tag.clone(),
            ".".into(),
        ]
    }

    /// Copy prebuilt ELFs into a PrefixLayout-shaped staging tree.
    /// Fail closed if the core ELF or a required slim pack is missing.
    /// Slim packs (including superhtml and javacs) are required on both triples.
    /// aarch64 javacs may need libc (not a Miss). Full packs (clangd/tsgo/gopls/zls)
    /// are optional (HOST-7 miss / clangd cache miss).
    pub fn stage(&self) -> Result<Vec<String>, String> {
        if !self.core_dest.is_file() {
            return Err(format!(
                "missing required core ELF {}; run `cargo xtask musl --target {}` first",
                self.core_dest.display(),
                self.triple
            ));
        }
        if let Some(parent) = self.staging.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        if self.staging.exists() {
            fs::remove_dir_all(&self.staging)
                .map_err(|e| format!("rm staging {}: {e}", self.staging.display()))?;
        }
        let prefix_root = self.staging.join("prefix");
        let layout = PrefixLayout::from_path(&prefix_root);
        for dir in [
            layout.bin_dir(),
            layout.engines_dir(),
            layout.cache_dir(),
            layout.log_dir(),
            layout.run_dir(),
            layout.scripts_dir(),
        ] {
            fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
            touch_keep(&dir)?;
        }
        let core_out = layout.bin_dir().join(CORE_ELF_NAME);
        copy_file(&self.core_dest, &core_out)?;
        let mut omitted = Vec::new();
        for pack in &self.pack_dests {
            if !pack.src.is_file() {
                if pack.required {
                    return Err(format!(
                        "missing required slim pack {} ({}); run `cargo xtask pack --pack {} --target {}` first",
                        pack.pack,
                        pack.src.display(),
                        pack.pack,
                        self.triple
                    ));
                }
                let why = "HOST-7 miss; not a Mach-O green";
                omitted.push(format!("{}:{} omitted ({why})", pack.pack, self.triple));
                continue;
            }
            let dest = prefix_root.join(pack.image_rel());
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            copy_file(&pack.src, &dest)?;
        }
        let tmp = self.staging.join("tmp");
        fs::create_dir_all(&tmp).map_err(|e| format!("mkdir {}: {e}", tmp.display()))?;
        touch_keep(&tmp)?;
        Ok(omitted)
    }
}

fn touch_keep(dir: &Path) -> Result<(), String> {
    fs::write(dir.join(STAGING_KEEP), b"").map_err(|e| format!("write keep {}: {e}", dir.display()))
}

fn copy_file(src: &Path, dest: &Path) -> Result<(), String> {
    fs::copy(src, dest)
        .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dest.display()))?;
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    run_at(&workspace_root(), args, &CommandDockerPort)
}

pub fn run_at(root: &Path, args: &[String], docker: &dyn DockerPort) -> Result<(), String> {
    let targets = parse_targets(args)?;
    for triple in &targets {
        let plan = RuntimeImagePlan::for_triple(root, triple)?;
        let omitted = plan.stage()?;
        for note in &omitted {
            eprintln!("xtask runtime-image: {note}");
        }
        docker.tag_image(plan.context(), &plan.docker_build_args())?;
        eprintln!(
            "xtask runtime-image: tagged {} ({})",
            plan.image_tag(),
            plan.docker_platform()
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
            other => return Err(format!("unknown runtime-image flag: {other}")),
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
    use crate::musl::RecordingDockerPort;
    use progressive_lsp_engine::{JAVA_PACK, PYTHON_PACK, SUPERHTML_PACK};

    fn fixture_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let docker = dir.path().join("docker");
        fs::create_dir_all(&docker).unwrap();
        fs::write(
            docker.join("runtime.Dockerfile"),
            "FROM scratch\nCOPY prefix /opt/plsp\nCOPY tmp /tmp\n\
             ENTRYPOINT [\"/opt/plsp/bin/progressive-lsp\"]\n\
             CMD [\"serve\", \"--prefix\", \"/opt/plsp\"]\n",
        )
        .unwrap();
        dir
    }

    fn write_elf(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"fixture-elf").unwrap();
    }

    fn seed_required_elfs(root: &Path, triple: &str, include_superhtml: bool) {
        let musl = root.join("target/musl").join(triple);
        write_elf(&musl.join(CORE_ELF_NAME));
        for pack in slim_pack_names() {
            if *pack == SUPERHTML_PACK && !include_superhtml {
                continue;
            }
            let binary = binary_name_for_pack(pack).unwrap();
            write_elf(&musl.join("engines").join(pack).join(binary));
        }
    }

    #[test]
    fn runtime_image_plan_is_value_object_for_both_triples_without_docker() {
        let root = fixture_root();
        let amd = RuntimeImagePlan::for_triple(root.path(), X86_64_MUSL).unwrap();
        assert_eq!(amd.triple(), X86_64_MUSL);
        assert_eq!(amd.docker_platform(), "linux/amd64");
        assert_eq!(amd.image_tag(), IMAGE_TAG);
        assert_eq!(amd.prefix(), IMAGE_PREFIX);
        assert_eq!(
            amd.core_dest(),
            root.path()
                .join("target/musl")
                .join(X86_64_MUSL)
                .join(CORE_ELF_NAME)
        );
        assert_eq!(
            amd.dockerfile(),
            root.path().join("docker/runtime.Dockerfile")
        );
        assert_eq!(
            amd.staging(),
            root.path().join("target/runtime-image").join(X86_64_MUSL)
        );
        assert_eq!(amd.context(), amd.staging());
        assert_eq!(amd, amd.clone());

        let packs: Vec<_> = amd.pack_dests().iter().map(|p| p.pack()).collect();
        assert_eq!(packs, full_pack_names());
        let clangd = amd
            .pack_dests()
            .iter()
            .find(|p| p.pack() == progressive_lsp_engine::CLANGD_PACK)
            .unwrap();
        assert!(!clangd.required(), "full packs are optional HOST-7 copies");
        let superhtml = amd
            .pack_dests()
            .iter()
            .find(|p| p.pack() == SUPERHTML_PACK)
            .unwrap();
        assert!(
            superhtml.required(),
            "superhtml is required on both triples (HOST-3 miss closed)"
        );
        assert_eq!(
            superhtml.image_rel(),
            PathBuf::from("engines/superhtml/superhtml")
        );
        let ty = amd
            .pack_dests()
            .iter()
            .find(|p| p.pack() == PYTHON_PACK)
            .unwrap();
        assert!(ty.required());
        assert_eq!(ty.binary(), "ty");
        assert_eq!(
            ty.src(),
            root.path()
                .join("target/musl")
                .join(X86_64_MUSL)
                .join("engines/python/ty")
        );

        let arm = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        assert_eq!(arm.docker_platform(), "linux/arm64");
        let arm_superhtml = arm
            .pack_dests()
            .iter()
            .find(|p| p.pack() == SUPERHTML_PACK)
            .unwrap();
        assert!(arm_superhtml.required());
        let arm_java = arm
            .pack_dests()
            .iter()
            .find(|p| p.pack() == JAVA_PACK)
            .unwrap();
        assert!(
            arm_java.required(),
            "java aarch64 dest is required (libc exception, not a Miss)"
        );
        let amd_java = amd
            .pack_dests()
            .iter()
            .find(|p| p.pack() == JAVA_PACK)
            .unwrap();
        assert!(
            amd_java.required(),
            "java x86_64 remains a required slim pack"
        );
        assert_ne!(amd, arm);
        let layout = PrefixLayout::from_path(IMAGE_PREFIX);
        assert_eq!(
            layout.bin_dir().join(CORE_ELF_NAME),
            Path::new("/opt/plsp/bin/progressive-lsp")
        );
        assert_eq!(
            layout.engines_dir().join("python").join("ty"),
            Path::new("/opt/plsp/engines/python/ty")
        );
    }

    #[test]
    fn runtime_image_plan_rejects_unknown_triple_and_missing_dockerfile() {
        let root = fixture_root();
        let err =
            RuntimeImagePlan::for_triple(root.path(), "x86_64-unknown-linux-gnu").unwrap_err();
        assert!(err.contains("unknown triple"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let missing = RuntimeImagePlan::for_triple(empty.path(), X86_64_MUSL).unwrap_err();
        assert!(missing.contains("missing"), "{missing}");
    }

    #[test]
    fn runtime_image_plan_docker_args_tag_locked_name() {
        let root = fixture_root();
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let args = plan.docker_build_args();
        assert_eq!(args[0], "build");
        assert!(args.contains(&"--platform".to_string()));
        assert!(args.contains(&"linux/arm64".to_string()));
        assert!(args.contains(&"-t".to_string()));
        assert!(args.contains(&IMAGE_TAG.to_string()));
        assert!(!args.iter().any(|a| a.contains("--output")));
        assert!(!args.iter().any(|a| a.contains("RUST_TARGET")));
        assert_eq!(args.last().map(String::as_str), Some("."));
        assert!(args.iter().any(|a| a.contains("runtime.Dockerfile")));
    }

    #[test]
    fn recording_docker_port_is_would_have_tagged_without_daemon() {
        let root = fixture_root();
        seed_required_elfs(root.path(), AARCH64_MUSL, true);
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let omitted = plan.stage().unwrap();
        assert!(
            omitted.iter().all(|n| n.contains("HOST-7 miss")),
            "{omitted:?}"
        );
        let docker = RecordingDockerPort::new();
        docker
            .tag_image(plan.context(), &plan.docker_build_args())
            .unwrap();
        let recorded = docker.recorded_dests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], plan.staging());
        let args = &docker.recorded_args()[0];
        assert!(args.contains(&IMAGE_TAG.to_string()));
        assert!(args.contains(&"linux/arm64".to_string()));
    }

    #[test]
    fn stage_copies_prefix_layout_and_tmp_without_docker() {
        let root = fixture_root();
        seed_required_elfs(root.path(), AARCH64_MUSL, true);
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        plan.stage().unwrap();
        let prefix = PrefixLayout::from_path(plan.staging().join("prefix"));
        assert!(prefix.bin_dir().join(CORE_ELF_NAME).is_file());
        assert!(prefix.engines_dir().join("python/ty").is_file());
        assert!(prefix.engines_dir().join("rust/rust-analyzer").is_file());
        assert!(prefix.engines_dir().join("phpantom/phpantom").is_file());
        assert!(prefix.engines_dir().join("biome/biome").is_file());
        assert!(prefix.engines_dir().join("java/javacs").is_file());
        assert!(prefix.engines_dir().join("superhtml/superhtml").is_file());
        for dir in [
            prefix.cache_dir(),
            prefix.log_dir(),
            prefix.run_dir(),
            prefix.scripts_dir(),
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        assert!(plan.staging().join("tmp").is_dir());
        assert_eq!(
            fs::read(prefix.bin_dir().join(CORE_ELF_NAME)).unwrap(),
            b"fixture-elf"
        );
    }

    #[test]
    fn stage_fails_closed_when_aarch64_javacs_dest_missing() {
        let root = fixture_root();
        seed_required_elfs(root.path(), AARCH64_MUSL, true);
        let java = root
            .path()
            .join("target/musl")
            .join(AARCH64_MUSL)
            .join("engines/java/javacs");
        fs::remove_file(&java).unwrap();
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let err = plan.stage().unwrap_err();
        assert!(err.contains("missing required slim pack"), "{err}");
        assert!(err.contains(JAVA_PACK), "{err}");
        assert!(err.contains(AARCH64_MUSL), "{err}");
    }

    #[test]
    fn stage_fails_closed_when_required_core_elf_missing() {
        let root = fixture_root();
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let err = plan.stage().unwrap_err();
        assert!(err.contains("missing required core ELF"), "{err}");
        assert!(err.contains(CORE_ELF_NAME), "{err}");
    }

    #[test]
    fn stage_fails_closed_when_required_slim_pack_missing() {
        let root = fixture_root();
        write_elf(
            &root
                .path()
                .join("target/musl")
                .join(AARCH64_MUSL)
                .join(CORE_ELF_NAME),
        );
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let err = plan.stage().unwrap_err();
        assert!(err.contains("missing required slim pack"), "{err}");
        assert!(err.contains(PYTHON_PACK), "{err}");
    }

    #[test]
    fn stage_fails_closed_when_superhtml_x86_64_dest_missing() {
        let root = fixture_root();
        seed_required_elfs(root.path(), X86_64_MUSL, false);
        let plan = RuntimeImagePlan::for_triple(root.path(), X86_64_MUSL).unwrap();
        let err = plan.stage().unwrap_err();
        assert!(err.contains("missing required slim pack"), "{err}");
        assert!(err.contains(SUPERHTML_PACK), "{err}");
        assert!(err.contains(X86_64_MUSL), "{err}");
    }

    #[test]
    fn stage_copies_required_superhtml_x86_64_when_seeded() {
        let root = fixture_root();
        seed_required_elfs(root.path(), X86_64_MUSL, true);
        let plan = RuntimeImagePlan::for_triple(root.path(), X86_64_MUSL).unwrap();
        let omitted = plan.stage().unwrap();
        assert!(
            !omitted.iter().any(|n| n.contains("superhtml")),
            "{omitted:?}"
        );
        assert!(
            omitted
                .iter()
                .any(|n| n.contains("clangd") && n.contains("HOST-7 miss")),
            "{omitted:?}"
        );
        assert!(
            omitted
                .iter()
                .any(|n| n.contains("gopls") && n.contains("HOST-7 miss")),
            "{omitted:?}"
        );
        let prefix = PrefixLayout::from_path(plan.staging().join("prefix"));
        assert!(prefix.engines_dir().join("superhtml/superhtml").is_file());
        assert!(prefix.engines_dir().join("python/ty").is_file());
        assert!(prefix.engines_dir().join("biome/biome").is_file());
        assert!(!prefix.engines_dir().join("clangd/clangd").exists());
    }

    #[test]
    fn stage_copies_optional_full_pack_when_present() {
        let root = fixture_root();
        seed_required_elfs(root.path(), AARCH64_MUSL, true);
        write_elf(
            &root
                .path()
                .join("target/musl")
                .join(AARCH64_MUSL)
                .join("engines/gopls/gopls"),
        );
        let plan = RuntimeImagePlan::for_triple(root.path(), AARCH64_MUSL).unwrap();
        let omitted = plan.stage().unwrap();
        assert!(
            omitted
                .iter()
                .any(|n| n.contains("clangd") && n.contains("HOST-7 miss")),
            "{omitted:?}"
        );
        assert!(!omitted.iter().any(|n| n.contains("gopls")), "{omitted:?}");
        let prefix = PrefixLayout::from_path(plan.staging().join("prefix"));
        assert!(prefix.engines_dir().join("gopls/gopls").is_file());
        assert!(!prefix.engines_dir().join("clangd/clangd").exists());
    }

    #[test]
    fn run_at_stages_then_tags_without_daemon() {
        let root = fixture_root();
        seed_required_elfs(root.path(), X86_64_MUSL, true);
        seed_required_elfs(root.path(), AARCH64_MUSL, true);
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
        assert!(recorded[0]
            .to_string_lossy()
            .contains(&format!("runtime-image/{X86_64_MUSL}")));
        let args = docker.recorded_args();
        assert!(args[0].contains(&IMAGE_TAG.to_string()));
        assert!(args[0].contains(&"linux/amd64".to_string()));
    }

    #[test]
    fn parse_targets_default_both_and_dedup() {
        let both = parse_targets(&[]).unwrap();
        assert_eq!(both.len(), 2);
        let dup = parse_targets(&["--both".into(), "--target".into(), X86_64_MUSL.into()]).unwrap();
        assert_eq!(dup.len(), 2);
        assert!(parse_targets(&["--nope".into()]).is_err());
        assert!(parse_targets(&["--target".into()]).is_err());
        assert!(parse_targets(&["--target".into(), "x86_64-unknown-linux-gnu".into()]).is_err());
    }

    #[test]
    fn dockerfile_is_scratch_copy_only() {
        let text = fs::read_to_string(workspace_root().join(DOCKERFILE_REL)).unwrap();
        assert!(text.contains("FROM scratch"), "{text}");
        assert!(text.contains("COPY prefix /opt/plsp"), "{text}");
        assert!(text.contains("COPY tmp /tmp"), "{text}");
        assert!(
            text.contains("ENTRYPOINT [\"/opt/plsp/bin/progressive-lsp\"]"),
            "{text}"
        );
        assert!(
            text.contains("CMD [\"serve\", \"--prefix\", \"/opt/plsp\"]"),
            "{text}"
        );
        let active: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect();
        for forbidden in ["rustc", "cargo", "clang", "llvm", "zig", "golang"] {
            assert!(
                !active.to_ascii_lowercase().contains(forbidden),
                "compiler {forbidden} in active dockerfile: {active}"
            );
        }
        assert!(!active.to_ascii_lowercase().contains("from rust"));
        assert!(!active.contains("RUN "));
    }

    #[test]
    fn image_tag_matches_poc_ide_runtime_image() {
        let src = fs::read_to_string(workspace_root().join("poc-ide/src/runtime.rs")).unwrap();
        assert!(
            src.contains(&format!("pub const RUNTIME_IMAGE: &str = \"{IMAGE_TAG}\"")),
            "xtask IMAGE_TAG must stay locked to poc-ide RUNTIME_IMAGE"
        );
    }

    #[test]
    fn run_rejects_unknown_flag() {
        assert!(run(&["--nope".into()]).is_err());
    }
}
