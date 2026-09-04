//! Slim engine pack musl jobs. Both Linux triples.
//!
//! Extract dest is `target/musl/<triple>/engines/<pack>/<binary>`. Tests inject
//! [`crate::musl::RecordingDockerPort`] and never start a daemon.
//! [`PackBuildPlan`] is the Value object. Pins live in `xtask/pack-pins.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use progressive_lsp_engine::{binary_name_for_pack, is_heavy_pack, slim_pack_names};

use crate::check_static;
use crate::musl::{triples, CommandDockerPort, DockerPort, AARCH64_MUSL, X86_64_MUSL};
use crate::workspace_root;

pub const PINS_REL: &str = "xtask/pack-pins.toml";

/// Value object. rust vs zig pack kind (toolchain lives in the build container).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackKind {
    Rust,
    Zig,
}

impl PackKind {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "rust" => Ok(Self::Rust),
            "zig" => Ok(Self::Zig),
            other => Err(format!(
                "unknown pack kind {other}; expected rust or zig (host php/Node/JVM/CPython forbidden)"
            )),
        }
    }
}

/// Value object. Pinned upstream repo + git SHA (not core crate semver).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackPin {
    name: String,
    binary: String,
    repo: String,
    sha: String,
    kind: PackKind,
    cargo_bin: String,
    source_subdir: String,
    dockerfile: String,
}

impl PackPin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }

    pub fn kind(&self) -> &PackKind {
        &self.kind
    }

    pub fn cargo_bin(&self) -> &str {
        &self.cargo_bin
    }

    pub fn source_subdir(&self) -> &str {
        &self.source_subdir
    }

    pub fn dockerfile_rel(&self) -> &str {
        &self.dockerfile
    }
}

/// Value object. Pack-build rustc channel when upstream has no rust-toolchain.toml.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustToolchainPin {
    channel: String,
}

impl RustToolchainPin {
    pub fn channel(&self) -> &str {
        &self.channel
    }
}

/// Value object. Zig compiler pin (container-only; not a shipped .so).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZigToolchainPin {
    version: String,
    tarball_x86_64: String,
    sha256_x86_64: String,
    tarball_aarch64: String,
    sha256_aarch64: String,
}

impl ZigToolchainPin {
    pub fn version(&self) -> &str {
        &self.version
    }

    fn for_triple(&self, triple: &str) -> Result<(&str, &str, &str), String> {
        match triple {
            X86_64_MUSL => Ok((
                "x86_64",
                self.tarball_x86_64.as_str(),
                self.sha256_x86_64.as_str(),
            )),
            AARCH64_MUSL => Ok((
                "aarch64",
                self.tarball_aarch64.as_str(),
                self.sha256_aarch64.as_str(),
            )),
            other => Err(format!("unknown triple {other} for zig toolchain")),
        }
    }
}

/// Value object. Pack name, binary, triple, platform, dockerfile, dest, pinned SHA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackBuildPlan {
    pack: String,
    binary: String,
    triple: String,
    docker_platform: String,
    dockerfile: PathBuf,
    dest: PathBuf,
    rust_target: String,
    pinned_sha: String,
    repo: String,
    kind: PackKind,
    cargo_bin: String,
    source_subdir: String,
    context: PathBuf,
    rust_channel: String,
    zig_version: Option<String>,
    zig_arch: Option<String>,
    zig_target: Option<String>,
    zig_sha256: Option<String>,
}

impl PackBuildPlan {
    pub fn for_pin(
        root: &Path,
        pin: &PackPin,
        triple: &str,
        rust: Option<&RustToolchainPin>,
        zig: Option<&ZigToolchainPin>,
    ) -> Result<Self, String> {
        let docker_platform = match triple {
            X86_64_MUSL => "linux/amd64",
            AARCH64_MUSL => "linux/arm64",
            _ => {
                return Err(format!(
                    "unknown triple {triple}; expected {X86_64_MUSL} or {AARCH64_MUSL}"
                ))
            }
        };
        let dockerfile = root.join(&pin.dockerfile);
        if !dockerfile.is_file() {
            return Err(format!("missing {}", dockerfile.display()));
        }
        let expected_bin =
            binary_name_for_pack(&pin.name).ok_or_else(|| format!("unknown pack {}", pin.name))?;
        if expected_bin != pin.binary {
            return Err(format!(
                "pin binary {} does not match discovery {} for pack {}",
                pin.binary, expected_bin, pin.name
            ));
        }
        let dest = root
            .join("target")
            .join("musl")
            .join(triple)
            .join("engines")
            .join(&pin.name)
            .join(&pin.binary);
        let (zig_version, zig_arch, zig_target, zig_sha256) = match pin.kind {
            PackKind::Zig => {
                let zig = zig.ok_or("zig pack requires [toolchain.zig] in pack-pins.toml")?;
                let (arch, _tarball, sha) = zig.for_triple(triple)?;
                let zt = match triple {
                    X86_64_MUSL => "x86_64-linux-musl",
                    AARCH64_MUSL => "aarch64-linux-musl",
                    _ => unreachable!(),
                };
                (
                    Some(zig.version.clone()),
                    Some(arch.to_string()),
                    Some(zt.to_string()),
                    Some(sha.to_string()),
                )
            }
            PackKind::Rust => (None, None, None, None),
        };
        Ok(Self {
            pack: pin.name.clone(),
            binary: pin.binary.clone(),
            triple: triple.to_string(),
            docker_platform: docker_platform.to_string(),
            dockerfile,
            dest,
            rust_target: triple.to_string(),
            pinned_sha: pin.sha.clone(),
            repo: pin.repo.clone(),
            kind: pin.kind.clone(),
            cargo_bin: pin.cargo_bin.clone(),
            source_subdir: pin.source_subdir.clone(),
            context: root.to_path_buf(),
            rust_channel: rust
                .map(|r| r.channel.clone())
                .unwrap_or_else(|| "1.98.0".into()),
            zig_version,
            zig_arch,
            zig_target,
            zig_sha256,
        })
    }

    pub fn pack(&self) -> &str {
        &self.pack
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    pub fn triple(&self) -> &str {
        &self.triple
    }

    pub fn docker_platform(&self) -> &str {
        &self.docker_platform
    }

    pub fn dockerfile(&self) -> &Path {
        &self.dockerfile
    }

    pub fn dest(&self) -> &Path {
        &self.dest
    }

    pub fn rust_target(&self) -> &str {
        &self.rust_target
    }

    pub fn pinned_sha(&self) -> &str {
        &self.pinned_sha
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn kind(&self) -> &PackKind {
        &self.kind
    }

    pub fn context(&self) -> &Path {
        &self.context
    }

    /// Directory passed to `docker build --output type=local,dest=…`.
    pub fn output_dir(&self) -> &Path {
        self.dest
            .parent()
            .expect("dest is target/musl/<triple>/engines/<pack>/<binary>")
    }

    /// `docker` argv (without the program name). Tests assert extract flags + SHA.
    pub fn docker_build_args(&self) -> Vec<String> {
        let mut args = vec![
            "build".into(),
            "--platform".into(),
            self.docker_platform.clone(),
            "--build-arg".into(),
            format!("PACK={}", self.pack),
            "--build-arg".into(),
            format!("UPSTREAM_REPO={}", self.repo),
            "--build-arg".into(),
            format!("UPSTREAM_SHA={}", self.pinned_sha),
            "--build-arg".into(),
            format!("BINARY={}", self.binary),
            "--build-arg".into(),
            format!("RUST_TARGET={}", self.rust_target),
            "--build-arg".into(),
            format!("CARGO_BIN={}", self.cargo_bin),
            "--build-arg".into(),
            format!("SOURCE_SUBDIR={}", self.source_subdir),
            "--build-arg".into(),
            format!("RUST_CHANNEL={}", self.rust_channel),
        ];
        if self.kind == PackKind::Zig {
            args.push("--build-arg".into());
            args.push(format!(
                "ZIG_VERSION={}",
                self.zig_version.as_deref().unwrap_or("")
            ));
            args.push("--build-arg".into());
            args.push(format!(
                "ZIG_ARCH={}",
                self.zig_arch.as_deref().unwrap_or("")
            ));
            args.push("--build-arg".into());
            args.push(format!(
                "ZIG_TARGET={}",
                self.zig_target.as_deref().unwrap_or("")
            ));
            args.push("--build-arg".into());
            args.push(format!(
                "ZIG_SHA256={}",
                self.zig_sha256.as_deref().unwrap_or("")
            ));
        }
        args.push("-f".into());
        args.push(self.dockerfile.display().to_string());
        args.push("--output".into());
        args.push(format!("type=local,dest={}", self.output_dir().display()));
        args.push(".".into());
        args
    }
}

pub fn load_pins(
    root: &Path,
) -> Result<
    (
        Vec<PackPin>,
        Option<RustToolchainPin>,
        Option<ZigToolchainPin>,
    ),
    String,
> {
    let path = root.join(PINS_REL);
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_pins(&text)
}

fn parse_pins(
    text: &str,
) -> Result<
    (
        Vec<PackPin>,
        Option<RustToolchainPin>,
        Option<ZigToolchainPin>,
    ),
    String,
> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("parse pack-pins.toml: {e}"))?;
    let rust = table
        .get("toolchain")
        .and_then(|t| t.get("rust"))
        .map(parse_rust)
        .transpose()?;
    let zig = table
        .get("toolchain")
        .and_then(|t| t.get("zig"))
        .map(parse_zig)
        .transpose()?;
    let packs = table
        .get("pack")
        .and_then(|v| v.as_array())
        .ok_or("pack-pins.toml must have [[pack]] entries")?;
    if packs.is_empty() {
        return Err("pack-pins.toml has no packs".into());
    }
    let mut out = Vec::new();
    for p in packs {
        out.push(parse_pack(p)?);
    }
    Ok((out, rust, zig))
}

fn parse_rust(v: &toml::Value) -> Result<RustToolchainPin, String> {
    Ok(RustToolchainPin {
        channel: req_str(v, "channel")?,
    })
}

fn parse_zig(v: &toml::Value) -> Result<ZigToolchainPin, String> {
    Ok(ZigToolchainPin {
        version: req_str(v, "version")?,
        tarball_x86_64: req_str(v, "tarball_x86_64")?,
        sha256_x86_64: req_hex(v, "sha256_x86_64", 64)?,
        tarball_aarch64: req_str(v, "tarball_aarch64")?,
        sha256_aarch64: req_hex(v, "sha256_aarch64", 64)?,
    })
}

fn parse_pack(v: &toml::Value) -> Result<PackPin, String> {
    let name = req_str(v, "name")?;
    if is_heavy_pack(&name) {
        return Err(format!(
            "heavy pack {name} is HOST-7; HOST-3 builds slim only (ty, rust-analyzer, phpantom, biome, superhtml)"
        ));
    }
    if binary_name_for_pack(&name).is_none() {
        return Err(format!("unknown pack {name}"));
    }
    let sha = req_hex(v, "sha", 40)?;
    let kind = PackKind::parse(&req_str(v, "kind")?)?;
    let binary = req_str(v, "binary")?;
    let cargo_bin = v
        .get("cargo_bin")
        .and_then(|x| x.as_str())
        .unwrap_or(&binary)
        .to_string();
    let source_subdir = v
        .get("source_subdir")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let dockerfile = v
        .get("dockerfile")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| match kind {
            PackKind::Rust => "docker/engine-pack.Dockerfile".into(),
            PackKind::Zig => "docker/engine-pack-zig.Dockerfile".into(),
        });
    Ok(PackPin {
        name,
        binary,
        repo: req_str(v, "repo")?,
        sha,
        kind,
        cargo_bin,
        source_subdir,
        dockerfile,
    })
}

fn req_str(v: &toml::Value, key: &str) -> Result<String, String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("pack pin missing {key}"))
}

fn req_hex(v: &toml::Value, key: &str, len: usize) -> Result<String, String> {
    let s = req_str(v, key)?;
    if s == "latest" || s == "unknown" {
        return Err(format!("{key} must be a git SHA, not {s}"));
    }
    if s.len() != len || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{key} must be {len} hex chars, got {s}"));
    }
    Ok(s.to_ascii_lowercase())
}

pub fn run(args: &[String]) -> Result<(), String> {
    run_at(&workspace_root(), args, &CommandDockerPort)
}

pub fn run_at(root: &Path, args: &[String], docker: &dyn DockerPort) -> Result<(), String> {
    let (want_packs, targets) = parse_pack_args(args)?;
    let (pins, rust, zig) = load_pins(root)?;
    for pack in &want_packs {
        refuse_heavy_or_unknown(pack)?;
        let pin = pins
            .iter()
            .find(|p| p.name == *pack)
            .ok_or_else(|| format!("pack {pack} is not pinned in {PINS_REL}"))?;
        for triple in &targets {
            let plan = PackBuildPlan::for_pin(root, pin, triple, rust.as_ref(), zig.as_ref())?;
            docker.extract(plan.dest(), plan.context(), &plan.docker_build_args())?;
            check_static::check_path(plan.dest()).map_err(|e| {
                format!(
                    "{}: {e} (refusing to pass a non-static extract; Mach-O is not a musl green)",
                    plan.dest().display()
                )
            })?;
            eprintln!(
                "xtask pack: check-static PASS {} ({} {})",
                plan.dest().display(),
                plan.pack(),
                plan.triple()
            );
        }
    }
    Ok(())
}

fn refuse_heavy_or_unknown(pack: &str) -> Result<(), String> {
    if is_heavy_pack(pack) {
        return Err(format!(
            "heavy pack {pack} is HOST-7; HOST-3 builds slim only"
        ));
    }
    if binary_name_for_pack(pack).is_none() {
        return Err(format!(
            "unknown pack {pack}; slim packs: {}",
            slim_pack_names().join(", ")
        ));
    }
    if !slim_pack_names().contains(&pack) {
        return Err(format!("pack {pack} is not a slim pack on HOST-3"));
    }
    Ok(())
}

fn parse_pack_args(args: &[String]) -> Result<(Vec<String>, Vec<String>), String> {
    let mut packs = Vec::new();
    let mut targets = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" | "--packs" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or("--pack requires slim or a CSV of slim packs")?;
                packs = expand_slim_list(raw)?;
            }
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
            "--slim" => {
                packs = slim_pack_names().iter().map(|s| (*s).to_string()).collect();
            }
            other => return Err(format!("unknown pack flag: {other}")),
        }
        i += 1;
    }
    if packs.is_empty() {
        packs = slim_pack_names().iter().map(|s| (*s).to_string()).collect();
    }
    if targets.is_empty() {
        targets.extend(triples().iter().map(|s| (*s).to_string()));
    }
    targets.sort();
    targets.dedup();
    Ok((packs, targets))
}

fn expand_slim_list(raw: &str) -> Result<Vec<String>, String> {
    match raw.trim() {
        "slim" => Ok(slim_pack_names().iter().map(|s| (*s).to_string()).collect()),
        "full" => Err("full flavor is HOST-7; HOST-3 is slim only".into()),
        other => {
            let mut out = Vec::new();
            for p in other.split(',') {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }
                refuse_heavy_or_unknown(p)?;
                out.push(p.to_string());
            }
            if out.is_empty() {
                return Err("--pack requires slim or a CSV of slim packs".into());
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musl::RecordingDockerPort;
    use progressive_lsp_engine::{
        BIOME_PACK, PHPANTOM_PACK, PYTHON_PACK, RUST_PACK, SUPERHTML_PACK,
    };

    fn fixture_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let docker = dir.path().join("docker");
        fs::create_dir_all(&docker).unwrap();
        fs::write(
            docker.join("engine-pack.Dockerfile"),
            "# test rust pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            docker.join("engine-pack-zig.Dockerfile"),
            "# test zig pack\nFROM scratch\n",
        )
        .unwrap();
        let xtask = dir.path().join("xtask");
        fs::create_dir_all(&xtask).unwrap();
        fs::write(xtask.join("pack-pins.toml"), SAMPLE_PINS).unwrap();
        dir
    }

    const SAMPLE_PINS: &str = r#"
[toolchain.rust]
channel = "1.98.0"

[toolchain.zig]
version = "0.15.1"
tarball_x86_64 = "https://example.test/zig-x86_64.tar.xz"
sha256_x86_64 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
tarball_aarch64 = "https://example.test/zig-aarch64.tar.xz"
sha256_aarch64 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

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
name = "phpantom"
binary = "phpantom"
repo = "https://github.com/PHPantom-dev/phpantom_lsp.git"
sha = "533ef884630924b774a2d74ac3d6050f7b3fbce0"
kind = "rust"
cargo_bin = "phpantom_lsp"
dockerfile = "docker/engine-pack.Dockerfile"

[[pack]]
name = "superhtml"
binary = "superhtml"
repo = "https://github.com/kristoff-it/superhtml.git"
sha = "9b8a0ab0d20339fbe1c803a72090d7b575efdc18"
kind = "zig"
dockerfile = "docker/engine-pack-zig.Dockerfile"
"#;

    #[test]
    fn pack_pin_is_value_object_content_addressed_by_sha() {
        let (pins, rust, zig) = parse_pins(SAMPLE_PINS).unwrap();
        assert_eq!(rust.expect("rust toolchain pin").channel(), "1.98.0");
        assert_eq!(pins.len(), 3);
        assert_eq!(pins[0].name(), PYTHON_PACK);
        assert_eq!(pins[0].binary(), "ty");
        assert_eq!(pins[0].sha().len(), 40);
        assert_ne!(pins[0].sha(), "latest");
        assert_eq!(pins[0].kind(), &PackKind::Rust);
        assert_eq!(pins[0].source_subdir(), "ruff");
        assert!(pins[0].repo().contains("astral-sh/ty"));
        assert_eq!(pins[0].dockerfile_rel(), "docker/engine-pack.Dockerfile");
        assert_eq!(pins[1].cargo_bin(), "phpantom_lsp");
        assert!(pins[1].repo().contains("phpantom_lsp"));
        assert_eq!(
            pins[2].dockerfile_rel(),
            "docker/engine-pack-zig.Dockerfile"
        );
        assert_eq!(pins[1].binary(), "phpantom");
        assert_eq!(pins[2].kind(), &PackKind::Zig);
        let zig = zig.expect("zig toolchain pin");
        assert_eq!(zig.version(), "0.15.1");
        assert_eq!(pins[0], pins[0].clone());
    }

    #[test]
    fn pack_pin_rejects_latest_and_heavy() {
        let latest = SAMPLE_PINS.replace("dca9f9873896bc3bfdb7db6459fd54f80fac3fa6", "latest");
        let err = parse_pins(&latest).unwrap_err();
        assert!(err.contains("SHA") || err.contains("latest"), "{err}");
        let heavy = SAMPLE_PINS.replace(r#"name = "python""#, r#"name = "clangd""#);
        let err = parse_pins(&heavy).unwrap_err();
        assert!(err.contains("HOST-7") || err.contains("heavy"), "{err}");
        assert!(PackKind::parse("python").is_err());
    }

    #[test]
    fn pack_build_plan_is_value_object_for_both_triples_without_docker() {
        let root = fixture_root();
        let (pins, rust, zig) = load_pins(root.path()).unwrap();
        let ty = pins.iter().find(|p| p.name() == PYTHON_PACK).unwrap();
        let amd = PackBuildPlan::for_pin(root.path(), ty, X86_64_MUSL, rust.as_ref(), zig.as_ref())
            .unwrap();
        assert_eq!(amd.pack(), PYTHON_PACK);
        assert_eq!(amd.binary(), "ty");
        assert_eq!(amd.triple(), X86_64_MUSL);
        assert_eq!(amd.docker_platform(), "linux/amd64");
        assert_eq!(amd.rust_target(), X86_64_MUSL);
        assert_eq!(
            amd.dest(),
            root.path()
                .join("target/musl")
                .join(X86_64_MUSL)
                .join("engines")
                .join(PYTHON_PACK)
                .join("ty")
        );
        assert_eq!(
            amd.dockerfile(),
            root.path().join("docker/engine-pack.Dockerfile")
        );
        assert_eq!(amd.pinned_sha(), ty.sha());
        assert_eq!(amd.repo(), ty.repo());
        assert_eq!(amd, amd.clone());

        let arm =
            PackBuildPlan::for_pin(root.path(), ty, AARCH64_MUSL, rust.as_ref(), zig.as_ref())
                .unwrap();
        assert_eq!(arm.docker_platform(), "linux/arm64");
        assert_ne!(amd, arm);

        let html = pins.iter().find(|p| p.name() == SUPERHTML_PACK).unwrap();
        let zig_plan =
            PackBuildPlan::for_pin(root.path(), html, AARCH64_MUSL, rust.as_ref(), zig.as_ref())
                .unwrap();
        assert_eq!(zig_plan.kind(), &PackKind::Zig);
        assert_eq!(zig_plan.dest().file_name().unwrap(), "superhtml");
        let zargs = zig_plan.docker_build_args();
        assert!(zargs
            .iter()
            .any(|a| a.contains("ZIG_TARGET=aarch64-linux-musl")));
        assert!(zargs.iter().any(|a| a.starts_with("UPSTREAM_SHA=")));
    }

    #[test]
    fn pack_build_plan_docker_args_export_named_elf() {
        let root = fixture_root();
        let (pins, rust, zig) = load_pins(root.path()).unwrap();
        let php = pins.iter().find(|p| p.name() == PHPANTOM_PACK).unwrap();
        let plan =
            PackBuildPlan::for_pin(root.path(), php, AARCH64_MUSL, rust.as_ref(), zig.as_ref())
                .unwrap();
        let args = plan.docker_build_args();
        assert_eq!(args[0], "build");
        assert!(args.contains(&"linux/arm64".to_string()));
        assert!(args
            .iter()
            .any(|a| a == "UPSTREAM_SHA=533ef884630924b774a2d74ac3d6050f7b3fbce0"));
        assert!(args.iter().any(|a| a == "CARGO_BIN=phpantom_lsp"));
        assert!(args.iter().any(|a| a == "BINARY=phpantom"));
        assert!(args.iter().any(|a| a == "PACK=phpantom"));
        assert!(args.iter().any(|a| a == "RUST_CHANNEL=1.98.0"));
        let dest_flag = args
            .iter()
            .find(|a| a.starts_with("type=local,dest="))
            .expect("local export dest");
        assert!(
            dest_flag.ends_with(&format!(
                "target/musl/{AARCH64_MUSL}/engines/{PHPANTOM_PACK}"
            )),
            "{dest_flag}"
        );
        assert_eq!(args.last().map(String::as_str), Some("."));
    }

    #[test]
    fn pack_build_plan_rejects_unknown_triple_and_missing_dockerfile() {
        let root = fixture_root();
        let (pins, rust, zig) = load_pins(root.path()).unwrap();
        let ty = &pins[0];
        let err = PackBuildPlan::for_pin(
            root.path(),
            ty,
            "x86_64-unknown-linux-gnu",
            rust.as_ref(),
            zig.as_ref(),
        )
        .unwrap_err();
        assert!(err.contains("unknown triple"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let missing =
            PackBuildPlan::for_pin(empty.path(), ty, X86_64_MUSL, rust.as_ref(), zig.as_ref())
                .unwrap_err();
        assert!(missing.contains("missing"), "{missing}");
    }

    #[test]
    fn run_at_extracts_fixture_then_check_static_not_a_musl_green() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &[
                "--pack".into(),
                "python".into(),
                "--target".into(),
                X86_64_MUSL.into(),
            ],
            &docker,
        )
        .unwrap();
        let dests = docker.recorded_dests();
        assert_eq!(dests.len(), 1);
        assert_eq!(dests[0].file_name().unwrap(), "ty");
        assert!(dests[0].to_string_lossy().contains("engines/python"));
        check_static::check_path(&dests[0]).unwrap();
        let args = &docker.recorded_args()[0];
        assert!(args.iter().any(|a| a.starts_with("UPSTREAM_SHA=")));
        assert!(args.iter().any(|a| a.contains("SOURCE_SUBDIR=ruff")));
    }

    #[test]
    fn recording_docker_port_covers_both_triples_for_phpantom() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &["--pack".into(), "phpantom".into(), "--both".into()],
            &docker,
        )
        .unwrap();
        let dests = docker.recorded_dests();
        assert_eq!(dests.len(), 2);
        for d in &dests {
            assert_eq!(d.file_name().unwrap(), "phpantom");
            check_static::check_path(d).unwrap();
        }
    }

    #[test]
    fn unknown_and_heavy_packs_fail_closed() {
        assert!(run(&["--pack".into(), "clangd".into()]).is_err());
        assert!(run(&["--pack".into(), "tsgo".into()]).is_err());
        assert!(run(&["--pack".into(), "gopls".into()]).is_err());
        assert!(run(&["--pack".into(), "zls".into()]).is_err());
        assert!(run(&["--pack".into(), "full".into()]).is_err());
        assert!(run(&["--pack".into(), "csharp-ls".into()]).is_err());
        assert!(run(&["--nope".into()]).is_err());
        assert!(run(&["--pack".into()]).is_err());
        assert!(run(&["--target".into()]).is_err());
        assert!(run(&["--target".into(), "x86_64-unknown-linux-gnu".into()]).is_err());
        let err = expand_slim_list("full").unwrap_err();
        assert!(err.contains("HOST-7"), "{err}");
    }

    #[test]
    fn default_is_slim_both_triples_and_workspace_pins_exist() {
        let (packs, targets) = parse_pack_args(&[]).unwrap();
        assert_eq!(packs, slim_pack_names());
        assert_eq!(targets.len(), 2);
        let slim = parse_pack_args(&["--slim".into(), "--both".into()]).unwrap();
        assert_eq!(slim.0.len(), 5);
        assert!(workspace_root().join(PINS_REL).is_file());
        assert!(workspace_root()
            .join("docker/engine-pack.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack-zig.Dockerfile")
            .is_file());
        let stub =
            fs::read_to_string(workspace_root().join("docker/engine-pack.Dockerfile")).unwrap();
        assert!(!stub.contains("cat /pack-id.txt"));
        assert!(stub.contains("UPSTREAM_SHA"));
        let (pins, rust, zig) = load_pins(&workspace_root()).unwrap();
        assert_eq!(pins.len(), 5);
        assert!(zig.is_some());
        assert_eq!(rust.expect("rust toolchain pin").channel(), "1.98.0");
        for name in [
            PYTHON_PACK,
            RUST_PACK,
            PHPANTOM_PACK,
            BIOME_PACK,
            SUPERHTML_PACK,
        ] {
            assert!(pins.iter().any(|p| p.name() == name));
        }
    }

    #[test]
    fn command_docker_port_exists_for_production() {
        let _ = CommandDockerPort;
        let _ = RecordingDockerPort::default();
    }
}
