//! Make-style freshness for `./build lsp`.
//!
//! **Fresh** = dest exists and input stamp matches → skip (code has not changed).
//! **Stale** / **Missing** = dest missing or inputs changed → rebuild that artifact.
//! `--force` treats every artifact as stale and rebuilds everything.
//!
//! [`LspArtifactStamp`], [`Freshness`], and [`LspArtifact`] are Value objects.
//! Tests inject [`crate::musl::RecordingDockerPort`]; no Docker daemon.

use std::fs;
use std::path::{Path, PathBuf};

use progressive_lsp_engine::{binary_name_for_pack, full_pack_names, slim_pack_names, CLANGD_PACK};
use progressive_lsp_install::{hex_encode, sha256};

use crate::cli::{LspArch, LspFlags, LspFlavor};
use crate::musl::{
    triples, CommandDockerPort, DockerPort, AARCH64_MUSL, CORE_ELF_NAME, X86_64_MUSL,
};
use crate::pack::PINS_REL;
use crate::runtime_image::{runtime_dockerfile_rel, DOCKERFILE_REL, IMAGE_TAG};
use crate::{musl, pack, runtime_image, workspace_root};

/// SHA-256 hex of the inputs that produce one dest ELF / image. Value object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspArtifactStamp {
    hex: String,
}

impl LspArtifactStamp {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            hex: hex_encode(&sha256(bytes)),
        }
    }

    #[cfg(test)]
    pub fn hex(&self) -> &str {
        &self.hex
    }

    pub fn read(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }
        let text =
            fs::read_to_string(path).map_err(|e| format!("read stamp {}: {e}", path.display()))?;
        let hex = text.trim();
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(None);
        }
        Ok(Some(Self {
            hex: hex.to_ascii_lowercase(),
        }))
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        fs::write(path, format!("{}\n", self.hex))
            .map_err(|e| format!("write stamp {}: {e}", path.display()))
    }
}

/// Make-style dest state. Value object.
/// `Fresh` skips; `Stale` and `Missing` rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
    Missing,
}

impl Freshness {
    /// `--force` short-circuits to rebuild. Dest missing → `Missing`.
    /// Dest exists + matching stamp → `Fresh`. Dest exists + different stamp → `Stale`.
    pub fn decide(
        force: bool,
        dest_exists: bool,
        current: &LspArtifactStamp,
        stored: Option<&LspArtifactStamp>,
    ) -> Self {
        if force {
            return Self::Stale;
        }
        if !dest_exists {
            return Self::Missing;
        }
        match stored {
            Some(stored) if stored == current => Self::Fresh,
            Some(_) => Self::Stale,
            None => Self::Missing,
        }
    }

    pub fn skip(self) -> bool {
        matches!(self, Self::Fresh)
    }
}

/// One `./build lsp` dest (core ELF, pack ELF, or runtime image stamp). Value object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LspArtifact {
    Core { triple: String },
    Pack { name: String, triple: String },
    Image {
        triple: String,
        flavor: LspFlavor,
    },
}

impl LspArtifact {
    pub fn core(triple: impl Into<String>) -> Self {
        Self::Core {
            triple: triple.into(),
        }
    }

    pub fn pack(name: impl Into<String>, triple: impl Into<String>) -> Self {
        Self::Pack {
            name: name.into(),
            triple: triple.into(),
        }
    }

    pub fn image(triple: impl Into<String>, flavor: LspFlavor) -> Self {
        Self::Image {
            triple: triple.into(),
            flavor,
        }
    }

    pub fn dest(&self, root: &Path) -> PathBuf {
        match self {
            Self::Core { triple } => root
                .join("target")
                .join("musl")
                .join(triple)
                .join(CORE_ELF_NAME),
            Self::Pack { name, triple } => {
                let binary = binary_name_for_pack(name).unwrap_or(name.as_str());
                root.join("target")
                    .join("musl")
                    .join(triple)
                    .join("engines")
                    .join(name)
                    .join(binary)
            }
            Self::Image { triple, .. } => root
                .join("target")
                .join("runtime-image")
                .join(triple)
                .join("image.stamp"),
        }
    }

    pub fn stamp_path(&self, root: &Path) -> PathBuf {
        match self {
            Self::Image { .. } => self.dest(root),
            Self::Core { .. } | Self::Pack { .. } => {
                let dest = self.dest(root);
                dest.with_extension(match dest.extension() {
                    Some(ext) => format!("{}.stamp", ext.to_string_lossy()),
                    None => "stamp".into(),
                })
            }
        }
    }

    pub fn dest_exists(&self, root: &Path) -> bool {
        match self {
            Self::Image { .. } => self.stamp_path(root).is_file(),
            Self::Core { .. } | Self::Pack { .. } => self.dest(root).is_file(),
        }
    }

    pub fn current_stamp(&self, root: &Path) -> Result<LspArtifactStamp, String> {
        Ok(LspArtifactStamp::from_bytes(&self.payload(root)?))
    }

    pub fn stored_stamp(&self, root: &Path) -> Result<Option<LspArtifactStamp>, String> {
        LspArtifactStamp::read(&self.stamp_path(root))
    }

    pub fn freshness(&self, root: &Path, flags: &LspFlags) -> Result<Freshness, String> {
        let current = self.current_stamp(root)?;
        let stored = self.stored_stamp(root)?;
        Ok(Freshness::decide(
            flags.force,
            self.dest_exists(root),
            &current,
            stored.as_ref(),
        ))
    }

    pub fn write_stamp(&self, root: &Path) -> Result<(), String> {
        self.current_stamp(root)?.write(&self.stamp_path(root))
    }

    fn payload(&self, root: &Path) -> Result<Vec<u8>, String> {
        let mut buf = Vec::new();
        match self {
            Self::Core { triple } => {
                buf.extend(b"core\0");
                buf.extend(triple.as_bytes());
            }
            Self::Pack { name, triple } => {
                buf.extend(b"pack\0");
                buf.extend(name.as_bytes());
                buf.push(0);
                buf.extend(triple.as_bytes());
                buf.push(0);
                buf.extend(pack::pack_stamp_payload(root, name)?);
            }
            Self::Image { triple, flavor } => {
                buf.extend(b"image\0");
                buf.extend(triple.as_bytes());
                buf.push(0);
                buf.extend(flavor.as_str().as_bytes());
            }
        }
        buf.push(0);
        for path in self.input_files(root)? {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            buf.extend(rel.to_string_lossy().as_bytes());
            buf.push(0);
            if path.is_file() {
                let bytes =
                    fs::read(&path).map_err(|e| format!("read input {}: {e}", path.display()))?;
                buf.extend(&bytes);
            } else {
                buf.extend(b"missing");
            }
            buf.push(0);
        }
        Ok(buf)
    }

    fn input_files(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        match self {
            Self::Core { .. } => {
                for rel in [
                    "docker/rust-musl.Dockerfile",
                    "Cargo.lock",
                    "Cargo.toml",
                    "rust-toolchain.toml",
                ] {
                    push_if_file(root.join(rel), &mut files);
                }
                collect_source_files(&root.join("src"), &mut files);
                collect_workspace_crates(root, &mut files)?;
            }
            Self::Pack { name, .. } => {
                if let Some(df) = dockerfile_for_pack(root, name) {
                    push_if_file(df, &mut files);
                }
            }
            Self::Image {
                triple,
                flavor,
            } => {
                if let Ok(rel) = runtime_dockerfile_rel(triple) {
                    push_if_file(root.join(rel), &mut files);
                }
                files.push(
                    root.join("target")
                        .join("musl")
                        .join(triple)
                        .join(CORE_ELF_NAME),
                );
                for pack in pack_names_for_flavor(*flavor) {
                    let binary = binary_name_for_pack(pack).unwrap_or(pack);
                    files.push(
                        root.join("target")
                            .join("musl")
                            .join(triple)
                            .join("engines")
                            .join(pack)
                            .join(binary),
                    );
                }
            }
        }
        files.sort();
        files.dedup();
        Ok(files)
    }
}

fn push_if_file(path: PathBuf, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path);
    }
}

fn collect_workspace_crates(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => return Err(format!("read {}: {e}", root.display())),
    };
    for ent in entries {
        let ent = ent.map_err(|e| format!("read {}: {e}", root.display()))?;
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("progressive-lsp-") && ent.path().is_dir() {
            collect_source_files(&ent.path(), files);
        }
    }
    Ok(())
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == "tests" || name == "benches" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if is_input_file(&path) {
            out.push(path);
        }
    }
}

fn is_input_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "toml" | "proto")
    )
}

fn dockerfile_for_pack(root: &Path, pack: &str) -> Option<PathBuf> {
    let text = fs::read_to_string(root.join(PINS_REL)).ok()?;
    let value: toml::Value = text.parse().ok()?;
    let packs = value.get("pack")?.as_array()?;
    for entry in packs {
        if entry.get("name")?.as_str()? == pack {
            let rel = entry.get("dockerfile")?.as_str()?;
            return Some(root.join(rel));
        }
    }
    None
}

fn triples_for(arch: LspArch) -> Vec<&'static str> {
    match arch {
        LspArch::All => triples().to_vec(),
        LspArch::X86_64 => vec![X86_64_MUSL],
        LspArch::Aarch64 => vec![AARCH64_MUSL],
    }
}

fn docker_target_args(want: &[&str]) -> Vec<String> {
    if want.len() == triples().len() && want.iter().all(|t| triples().contains(t)) {
        return vec!["--both".into()];
    }
    let mut out = Vec::new();
    for t in want {
        out.push("--target".into());
        out.push((*t).to_string());
    }
    out
}

pub fn pack_names_for_flavor(flavor: LspFlavor) -> &'static [&'static str] {
    match flavor {
        LspFlavor::Slim => slim_pack_names(),
        LspFlavor::Dogfood => full_pack_names(),
    }
}

pub fn execute_lsp(arch: LspArch, flags: LspFlags) -> Result<(), String> {
    execute_lsp_at(&workspace_root(), arch, flags, &CommandDockerPort)
}

pub fn execute_lsp_at(
    root: &Path,
    arch: LspArch,
    flags: LspFlags,
    docker: &dyn DockerPort,
) -> Result<(), String> {
    let triples = triples_for(arch);
    rebuild_cores(root, arch, &flags, &triples, docker)?;
    rebuild_packs(root, arch, &flags, &triples, docker)?;
    if flags.flavor == LspFlavor::Dogfood {
        verify_dogfood_pack_dests(root, &triples)?;
    }
    rebuild_images(root, arch, &flags, &triples, docker)?;
    Ok(())
}

fn verify_dogfood_pack_dests(root: &Path, triples: &[&str]) -> Result<(), String> {
    let mut missing = Vec::new();
    for triple in triples {
        for name in full_pack_names() {
            let artifact = LspArtifact::pack(*name, *triple);
            if !artifact.dest_exists(root) {
                missing.push(format!("{} ({})", *name, *triple));
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let mut msg = format!(
        "dogfood flavor: missing pack dest(s): {}",
        missing.join(", ")
    );
    if missing.iter().any(|m| m.starts_with(CLANGD_PACK)) {
        msg.push_str(
            "; clangd needs target/pack-cache/clangd/<sha>/<triple>/clangd \
             (xtask pack --pack clangd --cache-fill or POST-ART cache pull)",
        );
    }
    Err(msg)
}

fn rebuild_cores(
    root: &Path,
    arch: LspArch,
    flags: &LspFlags,
    triples: &[&str],
    docker: &dyn DockerPort,
) -> Result<(), String> {
    let mut stale = Vec::new();
    for triple in triples {
        let artifact = LspArtifact::core(*triple);
        let freshness = artifact.freshness(root, flags)?;
        if freshness.skip() {
            eprintln!(
                "./build lsp {}: Linux musl controller ({}) is fresh",
                arch.as_str(),
                triple
            );
        } else {
            stale.push(*triple);
        }
    }
    if stale.is_empty() {
        return Ok(());
    }
    eprintln!("./build lsp {}: (1/3) Linux musl controller", arch.as_str());
    musl::run_at(root, &docker_target_args(&stale), docker)?;
    for triple in stale {
        LspArtifact::core(triple).write_stamp(root)?;
    }
    Ok(())
}

fn rebuild_packs(
    root: &Path,
    arch: LspArch,
    flags: &LspFlags,
    triples: &[&str],
    docker: &dyn DockerPort,
) -> Result<(), String> {
    for triple in triples {
        let mut stale = Vec::new();
        for name in pack_names_for_flavor(flags.flavor) {
            if let Some(note) = pack::documented_miss_for_pack(root, name, triple)? {
                eprintln!(
                    "./build lsp {}: backends {} ({}) {note}",
                    arch.as_str(),
                    name,
                    triple
                );
                continue;
            }
            let artifact = LspArtifact::pack(*name, *triple);
            let freshness = artifact.freshness(root, flags)?;
            if freshness.skip() {
                eprintln!(
                    "./build lsp {}: backends {} ({}) is fresh",
                    arch.as_str(),
                    name,
                    triple
                );
            } else {
                stale.push(*name);
            }
        }
        if stale.is_empty() {
            continue;
        }
        eprintln!("./build lsp {}: (2/3) backends", arch.as_str());
        let mut args = vec!["--pack".into(), stale.join(",")];
        args.push("--target".into());
        args.push((*triple).to_string());
        pack::run_at(root, &args, docker)?;
        for name in stale {
            let artifact = LspArtifact::pack(name, *triple);
            if artifact.dest_exists(root) {
                artifact.write_stamp(root)?;
            }
        }
    }
    Ok(())
}

fn rebuild_images(
    root: &Path,
    arch: LspArch,
    flags: &LspFlags,
    triples: &[&str],
    docker: &dyn DockerPort,
) -> Result<(), String> {
    let mut stale = Vec::new();
    for triple in triples {
        let artifact = LspArtifact::image(*triple, flags.flavor);
        let freshness = artifact.freshness(root, flags)?;
        if freshness.skip() {
            eprintln!(
                "./build lsp {}: runtime image {IMAGE_TAG} ({}, flavor {}) is fresh",
                arch.as_str(),
                triple,
                flags.flavor.as_str()
            );
        } else {
            stale.push(*triple);
        }
    }
    if stale.is_empty() {
        return Ok(());
    }
    eprintln!(
        "./build lsp {}: (3/3) runtime image {IMAGE_TAG} (flavor {})",
        arch.as_str(),
        flags.flavor.as_str()
    );
    runtime_image::run_at(root, &docker_target_args(&stale), docker)?;
    for triple in stale {
        LspArtifact::image(triple, flags.flavor).write_stamp(root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_static::fixture_static_elf64;
    use crate::musl::RecordingDockerPort;
    use progressive_lsp_engine::{CLANGD_PACK, GOPLS_PACK, TSGO_PACK, ZLS_PACK};

    const SAMPLE_PINS: &str = r#"
[toolchain.rust]
channel = "1.98.0"

[toolchain.zig]
version = "0.15.1"
tarball_x86_64 = "https://example.test/zig-x86_64.tar.xz"
sha256_x86_64 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
tarball_aarch64 = "https://example.test/zig-aarch64.tar.xz"
sha256_aarch64 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[toolchain.go]
version = "1.26"

[[pack]]
name = "python"
binary = "ty"
repo = "https://github.com/astral-sh/ty.git"
sha = "dca9f9873896bc3bfdb7db6459fd54f80fac3fa6"
kind = "rust"
cargo_bin = "ty"
source_subdir = "ruff"
dockerfile = "docker/engine-pack.Dockerfile"

[[pack]]
name = "rust"
binary = "rust-analyzer"
repo = "https://github.com/rust-lang/rust-analyzer.git"
sha = "f8996691e991a4dc3c6f135e0fc04fc5561e4e9a"
kind = "rust"
cargo_bin = "rust-analyzer"
dockerfile = "docker/engine-pack.Dockerfile"

[[pack]]
name = "phpantom"
binary = "phpantom"
repo = "https://github.com/PHPantom-dev/phpantom_lsp.git"
sha = "533ef884630924b774a2d74ac3d6050f7b3fbce0"
kind = "rust"
cargo_bin = "phpantom_lsp"
dockerfile = "docker/engine-pack.Dockerfile"

[[pack]]
name = "biome"
binary = "biome"
repo = "https://github.com/biomejs/biome.git"
sha = "0a31d7c4e1f6cc1b3e08af4aef2ad4b97f6b4f1c"
kind = "rust"
cargo_bin = "biome"
dockerfile = "docker/engine-pack.Dockerfile"

[[pack]]
name = "superhtml"
binary = "superhtml"
repo = "https://github.com/kristoff-it/superhtml.git"
sha = "9b8a0ab0d20339fbe1c803a72090d7b575efdc18"
kind = "zig"
dockerfile = "docker/engine-pack-zig.Dockerfile"

[[pack]]
name = "gopls"
binary = "gopls"
repo = "https://github.com/golang/tools.git"
sha = "014f87ff5c01915bc90f4f11a6bb8aea3e0edbd7"
kind = "go"
source_subdir = "gopls"
go_package = "."
dockerfile = "docker/engine-pack-go.Dockerfile"

[[pack]]
name = "tsgo"
binary = "tsgo"
repo = "https://github.com/microsoft/typescript-go.git"
sha = "2bd066d87f5bafd315be9f40889d0a60b9e58e0b"
kind = "go"
go_package = "./cmd/tsgo"
dockerfile = "docker/engine-pack-go.Dockerfile"

[[pack]]
name = "zls"
binary = "zls"
repo = "https://github.com/zigtools/zls.git"
sha = "f91b2e1e305e5d5bd3725aea90f9f9bfb3dce055"
kind = "zig"
dockerfile = "docker/engine-pack-zig.Dockerfile"

[[pack]]
name = "clangd"
binary = "clangd"
repo = "https://github.com/llvm/llvm-project.git"
sha = "3623fe661ae35c6c80ac221f14d85be76aa870f1"
kind = "cached"
tag = "llvmorg-21.1.0"
dockerfile = "docker/engine-pack-clangd.Dockerfile"

[[pack]]
name = "java"
binary = "javacs"
repo = "https://example.test/java-language-server.git"
sha = "58daaa29a0e2fe22764283607da6801cf8b493b9"
kind = "graal"
dockerfile = "docker/engine-pack-graal.Dockerfile"
"#;

    fn fixture_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docker")).unwrap();
        fs::write(
            root.join("docker/rust-musl.Dockerfile"),
            "# core\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/runtime.Dockerfile"),
            "FROM scratch\nCOPY prefix /opt/plsp\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/runtime-aarch64.Dockerfile"),
            "FROM rockylinux:9-minimal\nCOPY prefix /opt/plsp\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/engine-pack.Dockerfile"),
            "# rust pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/engine-pack-zig.Dockerfile"),
            "# zig pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/engine-pack-go.Dockerfile"),
            "# go pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/engine-pack-graal.Dockerfile"),
            "# graal pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            root.join("docker/engine-pack-clangd.Dockerfile"),
            "# clangd\nFROM scratch\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), "lock-v1\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn core() {}\n").unwrap();
        fs::create_dir_all(root.join("xtask")).unwrap();
        fs::write(root.join(PINS_REL), SAMPLE_PINS).unwrap();
        dir
    }

    fn seed_dests(root: &Path, triple: &str) {
        let elf = fixture_static_elf64();
        let core = LspArtifact::core(triple);
        fs::create_dir_all(core.dest(root).parent().unwrap()).unwrap();
        fs::write(core.dest(root), &elf).unwrap();
        for name in slim_pack_names() {
            let pack = LspArtifact::pack(*name, triple);
            fs::create_dir_all(pack.dest(root).parent().unwrap()).unwrap();
            fs::write(pack.dest(root), &elf).unwrap();
        }
        for name in [CLANGD_PACK, GOPLS_PACK, TSGO_PACK, ZLS_PACK] {
            let pack = LspArtifact::pack(name, triple);
            fs::create_dir_all(pack.dest(root).parent().unwrap()).unwrap();
            fs::write(pack.dest(root), &elf).unwrap();
        }
    }

    fn write_matching_stamps(root: &Path, triple: &str) {
        LspArtifact::core(triple).write_stamp(root).unwrap();
        for name in slim_pack_names() {
            LspArtifact::pack(*name, triple).write_stamp(root).unwrap();
        }
        for name in [CLANGD_PACK, GOPLS_PACK, TSGO_PACK, ZLS_PACK] {
            LspArtifact::pack(name, triple).write_stamp(root).unwrap();
        }
        LspArtifact::image(triple, LspFlavor::Slim)
            .write_stamp(root)
            .unwrap();
    }

    #[test]
    fn lsp_artifact_stamp_is_value_object_hashed_inputs() {
        let a = LspArtifactStamp::from_bytes(b"pins+dockerfile");
        let b = LspArtifactStamp::from_bytes(b"pins+dockerfile");
        let c = LspArtifactStamp::from_bytes(b"pins+dockerfile-revised");
        assert_eq!(a, b);
        assert_eq!(a, a.clone());
        assert_ne!(a, c);
        assert_eq!(a.hex().len(), 64);
        assert!(a.hex().chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(a.hex(), hex_encode(&sha256(b"pins+dockerfile")));
    }

    #[test]
    fn freshness_fresh_skips_stale_and_missing_rebuild() {
        let stamp = LspArtifactStamp::from_bytes(b"same");
        let other = LspArtifactStamp::from_bytes(b"revised");
        assert_eq!(
            Freshness::decide(false, true, &stamp, Some(&stamp)),
            Freshness::Fresh
        );
        assert!(Freshness::decide(false, true, &stamp, Some(&stamp)).skip());
        assert_eq!(
            Freshness::decide(false, true, &other, Some(&stamp)),
            Freshness::Stale
        );
        assert!(!Freshness::decide(false, true, &other, Some(&stamp)).skip());
        assert_eq!(
            Freshness::decide(false, false, &stamp, Some(&stamp)),
            Freshness::Missing
        );
        assert!(!Freshness::Missing.skip());
        assert_eq!(
            Freshness::decide(false, true, &stamp, None),
            Freshness::Missing
        );
        assert_ne!(Freshness::Fresh, Freshness::Stale);
        assert_ne!(Freshness::Stale, Freshness::Missing);
    }

    #[test]
    fn force_is_stale_even_when_stamp_matches() {
        let stamp = LspArtifactStamp::from_bytes(b"same");
        assert_eq!(
            Freshness::decide(true, true, &stamp, Some(&stamp)),
            Freshness::Stale
        );
        assert!(!Freshness::decide(true, true, &stamp, Some(&stamp)).skip());
        assert_eq!(
            Freshness::decide(true, false, &stamp, None),
            Freshness::Stale
        );
    }

    #[test]
    fn core_stamp_changes_with_cargo_lock_not_pack_pins() {
        let dir = fixture_root();
        let root = dir.path();
        let core = LspArtifact::core(X86_64_MUSL);
        let pack = LspArtifact::pack("python", X86_64_MUSL);
        let before_core = core.current_stamp(root).unwrap();
        let before_pack = pack.current_stamp(root).unwrap();
        fs::write(root.join("Cargo.lock"), "lock-v2\n").unwrap();
        let after_lock_core = core.current_stamp(root).unwrap();
        assert_ne!(after_lock_core, before_core);
        assert_eq!(pack.current_stamp(root).unwrap(), before_pack);
        fs::write(root.join(PINS_REL), format!("{SAMPLE_PINS}\n# bump\n")).unwrap();
        assert_eq!(core.current_stamp(root).unwrap(), after_lock_core);
        assert_eq!(
            pack.current_stamp(root).unwrap(),
            before_pack,
            "comment outside [[pack]] python must not stale that pack"
        );
        assert_ne!(core, pack);
        assert_eq!(core, core.clone());
    }

    #[test]
    fn dogfood_image_stamp_includes_full_pack_dest_bytes() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        let slim = LspArtifact::image(X86_64_MUSL, LspFlavor::Slim);
        let dogfood = LspArtifact::image(X86_64_MUSL, LspFlavor::Dogfood);
        assert_ne!(slim.current_stamp(root).unwrap(), dogfood.current_stamp(root).unwrap());
        let slim_before = slim.current_stamp(root).unwrap();
        let dogfood_before = dogfood.current_stamp(root).unwrap();
        fs::write(
            LspArtifact::pack(CLANGD_PACK, X86_64_MUSL).dest(root),
            b"clangd-revised",
        )
        .unwrap();
        assert_eq!(slim.current_stamp(root).unwrap(), slim_before);
        assert_ne!(dogfood.current_stamp(root).unwrap(), dogfood_before);
    }

    #[test]
    fn execute_lsp_dogfood_fails_closed_when_clangd_dest_missing() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        fs::remove_file(LspArtifact::pack(CLANGD_PACK, X86_64_MUSL).dest(root)).unwrap();
        let docker = RecordingDockerPort::new();
        let err = execute_lsp_at(
            root,
            LspArch::X86_64,
            LspFlags {
                flavor: LspFlavor::Dogfood,
                ..LspFlags::default()
            },
            &docker,
        )
        .unwrap_err();
        assert!(err.contains("clangd"), "{err}");
        assert!(err.contains("pack-cache"), "{err}");
    }

    #[test]
    fn image_stamp_changes_when_copy_inputs_change() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        let image = LspArtifact::image(X86_64_MUSL, LspFlavor::Dogfood);
        let before = image.current_stamp(root).unwrap();
        fs::write(
            LspArtifact::core(X86_64_MUSL).dest(root),
            b"revised-core-elf",
        )
        .unwrap();
        assert_ne!(image.current_stamp(root).unwrap(), before);
        fs::write(
            LspArtifact::core(X86_64_MUSL).dest(root),
            fixture_static_elf64(),
        )
        .unwrap();
        assert_eq!(image.current_stamp(root).unwrap(), before);
        fs::write(root.join(DOCKERFILE_REL), "FROM scratch\n# revised\n").unwrap();
        assert_ne!(image.current_stamp(root).unwrap(), before);
    }

    #[test]
    fn execute_lsp_skips_when_dest_and_stamp_match() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        write_matching_stamps(root, X86_64_MUSL);
        let docker = RecordingDockerPort::new();
        execute_lsp_at(root, LspArch::X86_64, LspFlags::default(), &docker).unwrap();
        assert!(
            docker.recorded_dests().is_empty(),
            "fresh dest+stamp must skip docker: {:?}",
            docker.recorded_dests()
        );
        assert!(docker.recorded_args().is_empty());
    }

    #[test]
    fn execute_lsp_rebuilds_only_artifacts_with_revised_inputs() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        write_matching_stamps(root, X86_64_MUSL);
        fs::write(root.join("Cargo.lock"), "lock-revised\n").unwrap();
        let docker = RecordingDockerPort::new();
        execute_lsp_at(root, LspArch::X86_64, LspFlags::default(), &docker).unwrap();
        let dests = docker.recorded_dests();
        assert!(
            dests.iter().any(|p| p.ends_with(CORE_ELF_NAME)),
            "revised Cargo.lock rebuilds core: {dests:?}"
        );
        assert!(
            dests
                .iter()
                .all(|p| !p.to_string_lossy().contains("/engines/")),
            "unchanged pack pins skip backends: {dests:?}"
        );
        assert_eq!(
            dests.iter().filter(|p| p.ends_with(CORE_ELF_NAME)).count(),
            1
        );

        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        write_matching_stamps(root, X86_64_MUSL);
        fs::write(
            root.join("docker/engine-pack.Dockerfile"),
            "# rust pack revised\nFROM scratch\n",
        )
        .unwrap();
        let docker = RecordingDockerPort::new();
        execute_lsp_at(root, LspArch::X86_64, LspFlags::default(), &docker).unwrap();
        let dests = docker.recorded_dests();
        assert!(
            dests.iter().all(|p| !p.ends_with(CORE_ELF_NAME)),
            "unchanged core inputs skip controller: {dests:?}"
        );
        assert!(
            dests
                .iter()
                .any(|p| p.to_string_lossy().contains("/engines/python/")),
            "revised pack dockerfile rebuilds rust-kind packs: {dests:?}"
        );
        assert!(
            dests
                .iter()
                .all(|p| !p.to_string_lossy().contains("/engines/superhtml/")),
            "zig dockerfile unchanged skips superhtml: {dests:?}"
        );
        assert!(
            dests
                .iter()
                .all(|p| !p.to_string_lossy().contains("/engines/java/")),
            "graal dockerfile unchanged skips java: {dests:?}"
        );
    }

    #[test]
    fn java_pin_bump_stales_only_java_other_slim_packs_stay_fresh() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        write_matching_stamps(root, X86_64_MUSL);
        let pins = fs::read_to_string(root.join(PINS_REL)).unwrap();
        let revised = pins.replace(
            "58daaa29a0e2fe22764283607da6801cf8b493b9",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert_ne!(revised, pins);
        fs::write(root.join(PINS_REL), revised).unwrap();
        let flags = LspFlags::default();
        assert_eq!(
            LspArtifact::pack("java", X86_64_MUSL)
                .freshness(root, &flags)
                .unwrap(),
            Freshness::Stale
        );
        for name in ["python", "rust", "phpantom", "biome", "superhtml"] {
            assert_eq!(
                LspArtifact::pack(name, X86_64_MUSL)
                    .freshness(root, &flags)
                    .unwrap(),
                Freshness::Fresh,
                "{name} must stay Fresh when only [[pack]] java changes"
            );
        }
        let docker = RecordingDockerPort::new();
        execute_lsp_at(root, LspArch::X86_64, flags, &docker).unwrap();
        let dests = docker.recorded_dests();
        assert!(
            dests
                .iter()
                .any(|p| p.to_string_lossy().contains("/engines/java/")),
            "java pin bump rebuilds java: {dests:?}"
        );
        for name in ["python", "rust", "phpantom", "biome", "superhtml"] {
            assert!(
                dests
                    .iter()
                    .all(|p| !p.to_string_lossy().contains(&format!("/engines/{name}/"))),
                "java pin bump must not rebuild {name}: {dests:?}"
            );
        }
    }

    #[test]
    fn execute_lsp_force_rebuilds_even_when_stamp_matches() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, X86_64_MUSL);
        write_matching_stamps(root, X86_64_MUSL);
        let docker = RecordingDockerPort::new();
        execute_lsp_at(
            root,
            LspArch::X86_64,
            LspFlags {
                force: true,
                ..LspFlags::default()
            },
            &docker,
        )
        .unwrap();
        let dests = docker.recorded_dests();
        assert!(
            dests.iter().any(|p| p.ends_with(CORE_ELF_NAME)),
            "force rebuilds core: {dests:?}"
        );
        assert!(
            dests
                .iter()
                .any(|p| p.to_string_lossy().contains("/engines/")),
            "force rebuilds packs: {dests:?}"
        );
        assert!(
            docker
                .recorded_args()
                .iter()
                .any(|args| args.iter().any(|a| a == IMAGE_TAG || a.contains(IMAGE_TAG))),
            "force rebuilds runtime image: {:?}",
            docker.recorded_args()
        );
    }

    #[test]
    fn missing_aarch64_java_dest_rebuilds_via_recording_docker() {
        let dir = fixture_root();
        let root = dir.path();
        seed_dests(root, AARCH64_MUSL);
        write_matching_stamps(root, AARCH64_MUSL);
        fs::remove_file(LspArtifact::pack("java", AARCH64_MUSL).dest(root)).unwrap();
        let docker = RecordingDockerPort::new();
        execute_lsp_at(root, LspArch::Aarch64, LspFlags::default(), &docker).unwrap();
        assert!(
            docker
                .recorded_dests()
                .iter()
                .any(|p| p.to_string_lossy().contains("/engines/java/")),
            "missing aarch64 java dest must rebuild: {:?}",
            docker.recorded_dests()
        );
        assert!(LspArtifact::pack("java", AARCH64_MUSL).dest_exists(root));
    }

    #[test]
    fn missing_dest_rebuilds_without_stamp_match() {
        let dir = fixture_root();
        let root = dir.path();
        let core = LspArtifact::core(AARCH64_MUSL);
        let flags = LspFlags::default();
        assert_eq!(core.freshness(root, &flags).unwrap(), Freshness::Missing);
        assert!(!core.freshness(root, &flags).unwrap().skip());
        seed_dests(root, AARCH64_MUSL);
        core.write_stamp(root).unwrap();
        assert_eq!(core.freshness(root, &flags).unwrap(), Freshness::Fresh);
        fs::write(root.join("src/lib.rs"), "pub fn core() { /* revised */ }\n").unwrap();
        assert_eq!(core.freshness(root, &flags).unwrap(), Freshness::Stale);
    }
}
