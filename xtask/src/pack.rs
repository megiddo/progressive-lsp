//! Slim and full engine pack musl jobs. Both Linux triples.
//!
//! Extract dest is `target/musl/<triple>/engines/<pack>/<binary>`. clangd cache
//! dest is `target/pack-cache/clangd/<sha>/<triple>/clangd` (COPY hit; miss is
//! documented, never cmake in the default job). Tests inject
//! [`crate::musl::RecordingDockerPort`] and never start a daemon.
//! [`PackBuildPlan`] is the Value object. Pins live in `xtask/pack-pins.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use progressive_lsp_engine::{binary_name_for_pack, full_pack_names, slim_pack_names, CLANGD_PACK};

use crate::check_static;
use crate::musl::{triples, CommandDockerPort, DockerPort, AARCH64_MUSL, X86_64_MUSL};
use crate::workspace_root;

pub const PINS_REL: &str = "xtask/pack-pins.toml";

/// Value object. rust / zig / go / cached / cmake (toolchain lives in the container).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackKind {
    Rust,
    Zig,
    Go,
    /// clangd default: content-addressed cache COPY. Never cmake.
    Cached,
    /// clangd cache-fill only (`--cache-fill`). Not the default pack job.
    Cmake,
}

impl PackKind {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "rust" => Ok(Self::Rust),
            "zig" => Ok(Self::Zig),
            "go" => Ok(Self::Go),
            "cached" => Ok(Self::Cached),
            "cmake" => Ok(Self::Cmake),
            other => Err(format!(
                "unknown pack kind {other}; expected rust, zig, go, cached, or cmake \
                 (host php/Node/JVM/CPython forbidden)"
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
    go_package: String,
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

    pub fn go_package(&self) -> &str {
        &self.go_package
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

    /// Tarball matches the **container** ISA (`--platform`), not the output triple.
    /// Zig then cross-compiles with `-Dtarget`.
    pub fn for_docker_platform(&self, platform: &str) -> Result<(&str, &str, &str), String> {
        match platform {
            "linux/amd64" => Ok((
                "x86_64",
                self.tarball_x86_64.as_str(),
                self.sha256_x86_64.as_str(),
            )),
            "linux/arm64" => Ok((
                "aarch64",
                self.tarball_aarch64.as_str(),
                self.sha256_aarch64.as_str(),
            )),
            other => Err(format!("unknown docker platform {other} for zig toolchain")),
        }
    }
}

/// Native container ISA for Zig packs. qemu `linux/amd64` on Darwin is ENOSYS
/// (`faccessat`) in Zig 0.15.1's Options step. Rust/Go packs still follow the triple.
pub fn host_native_docker_platform() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("linux/arm64"),
        "x86_64" => Ok("linux/amd64"),
        other => Err(format!(
            "unsupported host arch {other} for zig pack docker platform; expected aarch64 or x86_64"
        )),
    }
}

fn zig_target_for_triple(triple: &str) -> Result<&'static str, String> {
    match triple {
        X86_64_MUSL => Ok("x86_64-linux-musl"),
        AARCH64_MUSL => Ok("aarch64-linux-musl"),
        other => Err(format!("unknown triple {other} for zig target")),
    }
}

/// Value object. Go compiler pin (container-only; CGO_ENABLED=0; not a shipped SDK).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoToolchainPin {
    version: String,
}

impl GoToolchainPin {
    pub fn version(&self) -> &str {
        &self.version
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
    go_version: Option<String>,
    go_os: Option<String>,
    go_arch: Option<String>,
    go_package: Option<String>,
}

impl PackBuildPlan {
    pub fn for_pin(
        root: &Path,
        pin: &PackPin,
        triple: &str,
        rust: Option<&RustToolchainPin>,
        zig: Option<&ZigToolchainPin>,
        go: Option<&GoToolchainPin>,
    ) -> Result<Self, String> {
        let triple_platform = match triple {
            X86_64_MUSL => "linux/amd64",
            AARCH64_MUSL => "linux/arm64",
            _ => {
                return Err(format!(
                    "unknown triple {triple}; expected {X86_64_MUSL} or {AARCH64_MUSL}"
                ))
            }
        };
        // Zig cross-compiles (`-Dtarget`); do not qemu the compiler. Rust/Go stay
        // on the triple platform (those already PASS under qemu).
        let docker_platform = match pin.kind {
            PackKind::Zig => host_native_docker_platform()?,
            PackKind::Rust | PackKind::Go | PackKind::Cached | PackKind::Cmake => triple_platform,
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
                let (arch, _tarball, sha) = zig.for_docker_platform(docker_platform)?;
                let zt = zig_target_for_triple(triple)?;
                (
                    Some(zig.version.clone()),
                    Some(arch.to_string()),
                    Some(zt.to_string()),
                    Some(sha.to_string()),
                )
            }
            PackKind::Rust | PackKind::Go | PackKind::Cached | PackKind::Cmake => {
                (None, None, None, None)
            }
        };
        let (go_version, go_os, go_arch, go_package) = match pin.kind {
            PackKind::Go => {
                let go = go.ok_or("go pack requires [toolchain.go] in pack-pins.toml")?;
                let arch = match triple {
                    X86_64_MUSL => "amd64",
                    AARCH64_MUSL => "arm64",
                    _ => unreachable!(),
                };
                (
                    Some(go.version.clone()),
                    Some("linux".into()),
                    Some(arch.to_string()),
                    Some(if pin.go_package.is_empty() {
                        ".".into()
                    } else {
                        pin.go_package.clone()
                    }),
                )
            }
            PackKind::Rust | PackKind::Zig | PackKind::Cached | PackKind::Cmake => {
                (None, None, None, None)
            }
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
            go_version,
            go_os,
            go_arch,
            go_package,
        })
    }

    /// clangd cache-fill plan. Dest is the cache ELF, dockerfile is cmake-only.
    /// Default `xtask pack` never constructs this.
    pub fn cache_fill_for_pin(
        root: &Path,
        pin: &PackPin,
        triple: &str,
        rust: Option<&RustToolchainPin>,
        zig: Option<&ZigToolchainPin>,
        go: Option<&GoToolchainPin>,
    ) -> Result<Self, String> {
        if pin.name != CLANGD_PACK {
            return Err(format!(
                "cache-fill is clangd only (LLVM); refusing {0}",
                pin.name
            ));
        }
        let mut plan = Self::for_pin(root, pin, triple, rust, zig, go)?;
        let dockerfile = root.join("docker/engine-pack-clangd-cache-fill.Dockerfile");
        if !dockerfile.is_file() {
            return Err(format!("missing {}", dockerfile.display()));
        }
        plan.kind = PackKind::Cmake;
        plan.dockerfile = dockerfile;
        plan.dest = plan.cache_src();
        Ok(plan)
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

    pub fn go_package(&self) -> Option<&str> {
        self.go_package.as_deref()
    }

    /// Content-addressed cache key: upstream SHA + triple.
    pub fn cache_key(&self) -> String {
        format!("{}:{}", self.pinned_sha, self.triple)
    }

    /// `target/pack-cache/<pack>/<sha>/<triple>/<binary>`.
    pub fn cache_src(&self) -> PathBuf {
        self.context
            .join("target")
            .join("pack-cache")
            .join(&self.pack)
            .join(&self.pinned_sha)
            .join(&self.triple)
            .join(&self.binary)
    }

    /// Directory passed to `docker build --output type=local,dest=…`.
    pub fn output_dir(&self) -> &Path {
        self.dest
            .parent()
            .expect("dest is target/musl/<triple>/engines/<pack>/<binary> or pack-cache")
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
        if self.kind == PackKind::Go {
            args.push("--build-arg".into());
            args.push(format!(
                "GO_VERSION={}",
                self.go_version.as_deref().unwrap_or("")
            ));
            args.push("--build-arg".into());
            args.push(format!("GOOS={}", self.go_os.as_deref().unwrap_or("linux")));
            args.push("--build-arg".into());
            args.push(format!("GOARCH={}", self.go_arch.as_deref().unwrap_or("")));
            args.push("--build-arg".into());
            args.push(format!(
                "GO_PACKAGE={}",
                self.go_package.as_deref().unwrap_or(".")
            ));
            args.push("--build-arg".into());
            args.push("CGO_ENABLED=0".into());
        }
        if self.kind == PackKind::Cmake {
            args.push("--build-arg".into());
            args.push(format!("CACHE_KEY={}", self.cache_key()));
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
        Option<GoToolchainPin>,
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
        Option<GoToolchainPin>,
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
    let go = table
        .get("toolchain")
        .and_then(|t| t.get("go"))
        .map(parse_go)
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
    Ok((out, rust, zig, go))
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

fn parse_go(v: &toml::Value) -> Result<GoToolchainPin, String> {
    Ok(GoToolchainPin {
        version: req_str(v, "version")?,
    })
}

fn parse_pack(v: &toml::Value) -> Result<PackPin, String> {
    let name = req_str(v, "name")?;
    if binary_name_for_pack(&name).is_none() {
        return Err(format!("unknown pack {name}"));
    }
    let sha = req_hex(v, "sha", 40)?;
    let kind = PackKind::parse(&req_str(v, "kind")?)?;
    if kind == PackKind::Cmake {
        return Err(
            "kind=cmake is cache-fill only; pin clangd as cached (default pack never cmake)".into(),
        );
    }
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
    let go_package = v
        .get("go_package")
        .and_then(|x| x.as_str())
        .unwrap_or(".")
        .to_string();
    let dockerfile = v
        .get("dockerfile")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| match kind {
            PackKind::Rust => "docker/engine-pack.Dockerfile".into(),
            PackKind::Zig => "docker/engine-pack-zig.Dockerfile".into(),
            PackKind::Go => "docker/engine-pack-go.Dockerfile".into(),
            PackKind::Cached => "docker/engine-pack-clangd.Dockerfile".into(),
            PackKind::Cmake => "docker/engine-pack-clangd-cache-fill.Dockerfile".into(),
        });
    Ok(PackPin {
        name,
        binary,
        repo: req_str(v, "repo")?,
        sha,
        kind,
        cargo_bin,
        source_subdir,
        go_package,
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
    let (want_packs, targets, cache_fill) = parse_pack_args(args)?;
    let (pins, rust, zig, go) = load_pins(root)?;
    let mut failures = Vec::new();
    for pack in &want_packs {
        refuse_unknown(pack)?;
        if cache_fill && pack != CLANGD_PACK {
            return Err(format!("cache-fill is clangd only (LLVM); refusing {pack}"));
        }
        let pin = pins
            .iter()
            .find(|p| p.name == *pack)
            .ok_or_else(|| format!("pack {pack} is not pinned in {PINS_REL}"))?;
        for triple in &targets {
            let plan = if cache_fill {
                PackBuildPlan::cache_fill_for_pin(
                    root,
                    pin,
                    triple,
                    rust.as_ref(),
                    zig.as_ref(),
                    go.as_ref(),
                )?
            } else {
                PackBuildPlan::for_pin(root, pin, triple, rust.as_ref(), zig.as_ref(), go.as_ref())?
            };
            match extract_plan(&plan, docker) {
                Ok(PackOutcome::Pass) => {
                    eprintln!(
                        "xtask pack: check-static PASS {} ({} {})",
                        plan.dest().display(),
                        plan.pack(),
                        plan.triple()
                    );
                }
                Ok(PackOutcome::Miss(note)) => {
                    eprintln!("xtask pack: {note}");
                }
                Err(e) => {
                    eprintln!("xtask pack: FAIL {} {}: {e}", plan.pack(), plan.triple());
                    failures.push(format!("{} {}: {e}", plan.pack(), plan.triple()));
                }
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

enum PackOutcome {
    Pass,
    Miss(String),
}

fn extract_plan(plan: &PackBuildPlan, docker: &dyn DockerPort) -> Result<PackOutcome, String> {
    if plan.kind() == &PackKind::Cached {
        let cache = plan.cache_src();
        if !cache.is_file() {
            return Ok(PackOutcome::Miss(format!(
                "{}:{} cache miss (key {}); documented gap, not cmake / not a Mach-O green",
                plan.pack(),
                plan.triple(),
                plan.cache_key()
            )));
        }
        if let Some(parent) = plan.dest().parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        fs::copy(&cache, plan.dest()).map_err(|e| {
            format!(
                "cache COPY {} -> {}: {e}",
                cache.display(),
                plan.dest().display()
            )
        })?;
        check_static_dest(plan)?;
        return Ok(PackOutcome::Pass);
    }
    docker.extract(plan.dest(), plan.context(), &plan.docker_build_args())?;
    check_static_dest(plan)?;
    Ok(PackOutcome::Pass)
}

fn check_static_dest(plan: &PackBuildPlan) -> Result<(), String> {
    check_static::check_path(plan.dest()).map_err(|e| {
        format!(
            "{}: {e} (refusing to pass a non-static extract; Mach-O is not a musl green; \
             unclosable clangd .so is a miss, do not ship dynamic)",
            plan.dest().display()
        )
    })
}

fn refuse_unknown(pack: &str) -> Result<(), String> {
    if binary_name_for_pack(pack).is_none() {
        return Err(format!(
            "unknown pack {pack}; known: slim, full, or {}",
            full_pack_names().join(", ")
        ));
    }
    Ok(())
}

fn parse_pack_args(args: &[String]) -> Result<(Vec<String>, Vec<String>, bool), String> {
    let mut packs = Vec::new();
    let mut targets = Vec::new();
    let mut cache_fill = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" | "--packs" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or("--pack requires slim, full, or a CSV of known packs")?;
                packs = expand_pack_list(raw)?;
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
            "--full" => {
                packs = full_pack_names().iter().map(|s| (*s).to_string()).collect();
            }
            "--cache-fill" => {
                cache_fill = true;
            }
            other => return Err(format!("unknown pack flag: {other}")),
        }
        i += 1;
    }
    if packs.is_empty() {
        packs = if cache_fill {
            vec![CLANGD_PACK.to_string()]
        } else {
            slim_pack_names().iter().map(|s| (*s).to_string()).collect()
        };
    }
    if cache_fill {
        for p in &packs {
            if p != CLANGD_PACK {
                return Err(format!("cache-fill is clangd only; refusing {p}"));
            }
        }
    }
    if targets.is_empty() {
        targets.extend(triples().iter().map(|s| (*s).to_string()));
    }
    targets.sort();
    targets.dedup();
    Ok((packs, targets, cache_fill))
}

fn expand_pack_list(raw: &str) -> Result<Vec<String>, String> {
    match raw.trim() {
        "slim" => Ok(slim_pack_names().iter().map(|s| (*s).to_string()).collect()),
        "full" => Ok(full_pack_names().iter().map(|s| (*s).to_string()).collect()),
        other => {
            let mut out = Vec::new();
            for p in other.split(',') {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }
                refuse_unknown(p)?;
                out.push(p.to_string());
            }
            if out.is_empty() {
                return Err("--pack requires slim, full, or a CSV of known packs".into());
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
        full_pack_names, is_heavy_pack, slim_pack_names, BIOME_PACK, CLANGD_PACK, GOPLS_PACK,
        PHPANTOM_PACK, PYTHON_PACK, RUST_PACK, SUPERHTML_PACK, TSGO_PACK, ZLS_PACK,
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
        fs::write(
            docker.join("engine-pack-go.Dockerfile"),
            "# test go pack\nFROM scratch\n",
        )
        .unwrap();
        fs::write(
            docker.join("engine-pack-clangd.Dockerfile"),
            "# test clangd cache COPY\nFROM scratch\nCOPY cache/clangd /clangd\n",
        )
        .unwrap();
        fs::write(
            docker.join("engine-pack-clangd-cache-fill.Dockerfile"),
            "# test clangd cache-fill\nFROM scratch\n# cmake LLVM cache-fill only\n",
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
dockerfile = "docker/engine-pack-clangd.Dockerfile"
"#;

    #[test]
    fn go_toolchain_pin_is_value_object_container_only() {
        let (_, _, _, go) = parse_pins(SAMPLE_PINS).unwrap();
        let go = go.expect("go toolchain pin");
        assert_eq!(go.version(), "1.26");
        assert_eq!(go, go.clone());
    }

    #[test]
    fn pack_kind_is_value_object_go_cached_cmake() {
        assert_eq!(PackKind::parse("go").unwrap(), PackKind::Go);
        assert_eq!(PackKind::parse("cached").unwrap(), PackKind::Cached);
        assert_eq!(PackKind::parse("cmake").unwrap(), PackKind::Cmake);
        assert!(PackKind::parse("python").is_err());
        assert_eq!(PackKind::Go, PackKind::Go.clone());
    }

    #[test]
    fn pack_pin_is_value_object_content_addressed_by_sha() {
        let (pins, rust, zig, go) = parse_pins(SAMPLE_PINS).unwrap();
        assert_eq!(rust.expect("rust toolchain pin").channel(), "1.98.0");
        assert_eq!(pins.len(), 7);
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
        let go = go.expect("go toolchain pin");
        assert_eq!(go.version(), "1.26");
        let gopls = pins.iter().find(|p| p.name() == GOPLS_PACK).unwrap();
        assert_eq!(gopls.kind(), &PackKind::Go);
        assert_eq!(gopls.go_package(), ".");
        assert_eq!(gopls.sha().len(), 40);
        let tsgo = pins.iter().find(|p| p.name() == TSGO_PACK).unwrap();
        assert_eq!(tsgo.go_package(), "./cmd/tsgo");
        let clangd = pins.iter().find(|p| p.name() == CLANGD_PACK).unwrap();
        assert_eq!(clangd.kind(), &PackKind::Cached);
        assert_eq!(pins[0], pins[0].clone());
    }

    #[test]
    fn pack_pin_rejects_latest_and_unknown_kind() {
        let latest = SAMPLE_PINS.replace("dca9f9873896bc3bfdb7db6459fd54f80fac3fa6", "latest");
        let err = parse_pins(&latest).unwrap_err();
        assert!(err.contains("SHA") || err.contains("latest"), "{err}");
        assert!(PackKind::parse("python").is_err());
        assert!(PackKind::parse("go").is_ok());
        assert!(PackKind::parse("cached").is_ok());
        assert!(PackKind::parse("cmake").is_ok());
        let cmake_pin = SAMPLE_PINS.replace(r#"kind = "cached""#, r#"kind = "cmake""#);
        let err = parse_pins(&cmake_pin).unwrap_err();
        assert!(err.contains("cache-fill") || err.contains("cmake"), "{err}");
        let unknown = SAMPLE_PINS.replace(r#"name = "python""#, r#"name = "csharp-ls""#);
        let err = parse_pins(&unknown).unwrap_err();
        assert!(err.contains("unknown pack"), "{err}");
    }

    #[test]
    fn pack_build_plan_is_value_object_for_both_triples_without_docker() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let ty = pins.iter().find(|p| p.name() == PYTHON_PACK).unwrap();
        let amd = PackBuildPlan::for_pin(
            root.path(),
            ty,
            X86_64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
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

        let arm = PackBuildPlan::for_pin(
            root.path(),
            ty,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(arm.docker_platform(), "linux/arm64");
        assert_ne!(amd, arm);

        let html = pins.iter().find(|p| p.name() == SUPERHTML_PACK).unwrap();
        let zig_plan = PackBuildPlan::for_pin(
            root.path(),
            html,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(zig_plan.kind(), &PackKind::Zig);
        assert_eq!(zig_plan.dest().file_name().unwrap(), "superhtml");
        let zargs = zig_plan.docker_build_args();
        assert!(zargs
            .iter()
            .any(|a| a.contains("ZIG_TARGET=aarch64-linux-musl")));
        assert!(zargs.iter().any(|a| a.starts_with("UPSTREAM_SHA=")));
        assert_eq!(
            zig_plan.docker_platform(),
            host_native_docker_platform().unwrap()
        );
    }

    #[test]
    fn zig_packs_use_host_native_platform_and_zig_target_for_both_triples() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let host_platform = host_native_docker_platform().unwrap();
        let host_zig_arch = match host_platform {
            "linux/amd64" => "x86_64",
            "linux/arm64" => "aarch64",
            other => panic!("unexpected host docker platform {other}"),
        };
        let host_sha = zig
            .as_ref()
            .unwrap()
            .for_docker_platform(host_platform)
            .unwrap()
            .2;
        for (name, triple, zig_target) in [
            (SUPERHTML_PACK, X86_64_MUSL, "x86_64-linux-musl"),
            (SUPERHTML_PACK, AARCH64_MUSL, "aarch64-linux-musl"),
            (ZLS_PACK, X86_64_MUSL, "x86_64-linux-musl"),
            (ZLS_PACK, AARCH64_MUSL, "aarch64-linux-musl"),
        ] {
            let pin = pins.iter().find(|p| p.name() == name).unwrap();
            let plan = PackBuildPlan::for_pin(
                root.path(),
                pin,
                triple,
                rust.as_ref(),
                zig.as_ref(),
                go.as_ref(),
            )
            .unwrap();
            assert_eq!(plan.kind(), &PackKind::Zig);
            assert_eq!(plan.triple(), triple);
            assert_eq!(
                plan.docker_platform(),
                host_platform,
                "{name} {triple} must not qemu the Zig compiler"
            );
            let args = plan.docker_build_args();
            assert!(
                args.contains(&host_platform.to_string()),
                "{name} {triple} args={args:?}"
            );
            assert!(
                args.iter()
                    .any(|a| a == &format!("ZIG_ARCH={host_zig_arch}")),
                "{name} {triple} args={args:?}"
            );
            assert!(
                args.iter()
                    .any(|a| a == &format!("ZIG_TARGET={zig_target}")),
                "{name} {triple} args={args:?}"
            );
            assert!(
                args.iter().any(|a| a == &format!("ZIG_SHA256={host_sha}")),
                "{name} {triple} args={args:?}"
            );
        }
        let rust_pin = pins.iter().find(|p| p.name() == PYTHON_PACK).unwrap();
        let rust_amd = PackBuildPlan::for_pin(
            root.path(),
            rust_pin,
            X86_64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(
            rust_amd.docker_platform(),
            "linux/amd64",
            "rust packs keep triple platform (qemu amd64 is fine)"
        );
        let err = zig
            .as_ref()
            .unwrap()
            .for_docker_platform("linux/s390x")
            .unwrap_err();
        assert!(err.contains("unknown docker platform"), "{err}");
        assert!(zig_target_for_triple("x86_64-unknown-linux-gnu").is_err());
    }

    #[test]
    fn recording_docker_port_covers_both_superhtml_triples_on_host_platform() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &["--pack".into(), "superhtml".into(), "--both".into()],
            &docker,
        )
        .unwrap();
        let dests = docker.recorded_dests();
        assert_eq!(dests.len(), 2);
        let host_platform = host_native_docker_platform().unwrap();
        for args in docker.recorded_args() {
            assert!(args.contains(&"--platform".to_string()), "{args:?}");
            assert!(
                args.contains(&host_platform.to_string()),
                "zig pack must use host-native {host_platform}, args={args:?}"
            );
            if host_platform != "linux/amd64" {
                assert!(
                    !args.contains(&"linux/amd64".to_string()),
                    "must not qemu amd64 for zig, args={args:?}"
                );
            }
        }
        for d in &dests {
            assert_eq!(d.file_name().unwrap(), "superhtml");
            check_static::check_path(d).unwrap();
        }
    }

    #[test]
    fn pack_build_plan_docker_args_export_named_elf() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let php = pins.iter().find(|p| p.name() == PHPANTOM_PACK).unwrap();
        let plan = PackBuildPlan::for_pin(
            root.path(),
            php,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
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
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let ty = &pins[0];
        let err = PackBuildPlan::for_pin(
            root.path(),
            ty,
            "x86_64-unknown-linux-gnu",
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap_err();
        assert!(err.contains("unknown triple"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let missing = PackBuildPlan::for_pin(
            empty.path(),
            ty,
            X86_64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
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
    fn unknown_packs_fail_closed_full_is_allowed() {
        assert!(run(&["--pack".into(), "csharp-ls".into()]).is_err());
        assert!(run(&["--nope".into()]).is_err());
        assert!(run(&["--pack".into()]).is_err());
        assert!(run(&["--target".into()]).is_err());
        assert!(run(&["--target".into(), "x86_64-unknown-linux-gnu".into()]).is_err());
        let full = expand_pack_list("full").unwrap();
        assert_eq!(full, full_pack_names());
        let csv = expand_pack_list("clangd,tsgo,gopls,zls").unwrap();
        assert_eq!(csv, vec![CLANGD_PACK, TSGO_PACK, GOPLS_PACK, ZLS_PACK]);
        assert!(is_heavy_pack(CLANGD_PACK));
        assert!(expand_pack_list("csharp-ls").is_err());
    }

    #[test]
    fn default_is_slim_both_triples_and_workspace_pins_exist() {
        let (packs, targets, cache_fill) = parse_pack_args(&[]).unwrap();
        assert_eq!(packs, slim_pack_names());
        assert_eq!(targets.len(), 2);
        assert!(!cache_fill);
        let slim = parse_pack_args(&["--slim".into(), "--both".into()]).unwrap();
        assert_eq!(slim.0.len(), 5);
        let full = parse_pack_args(&["--full".into()]).unwrap();
        assert_eq!(full.0, full_pack_names());
        let named = parse_pack_args(&["--pack".into(), "full".into()]).unwrap();
        assert_eq!(named.0, full_pack_names());
        assert!(workspace_root().join(PINS_REL).is_file());
        assert!(workspace_root()
            .join("docker/engine-pack.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack-zig.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack-go.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack-clangd.Dockerfile")
            .is_file());
        assert!(workspace_root()
            .join("docker/engine-pack-clangd-cache-fill.Dockerfile")
            .is_file());
        let stub =
            fs::read_to_string(workspace_root().join("docker/engine-pack.Dockerfile")).unwrap();
        assert!(!stub.contains("cat /pack-id.txt"));
        assert!(stub.contains("UPSTREAM_SHA"));
        let clangd_df =
            fs::read_to_string(workspace_root().join("docker/engine-pack-clangd.Dockerfile"))
                .unwrap();
        let active: String = clangd_df
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect();
        assert!(!active.to_ascii_lowercase().contains("cmake"));
        assert!(clangd_df.contains("COPY"));
        let fill = fs::read_to_string(
            workspace_root().join("docker/engine-pack-clangd-cache-fill.Dockerfile"),
        )
        .unwrap();
        assert!(fill.to_ascii_lowercase().contains("cmake"));
        let go_df =
            fs::read_to_string(workspace_root().join("docker/engine-pack-go.Dockerfile")).unwrap();
        assert!(go_df.contains("CGO_ENABLED=0"));
        assert!(!go_df.contains("CGO_ENABLED=1"));
        let (pins, rust, zig, go) = load_pins(&workspace_root()).unwrap();
        assert_eq!(pins.len(), 9);
        assert!(zig.is_some());
        assert_eq!(go.expect("go toolchain pin").version(), "1.26");
        assert_eq!(rust.expect("rust toolchain pin").channel(), "1.98.0");
        for name in [
            PYTHON_PACK,
            RUST_PACK,
            PHPANTOM_PACK,
            BIOME_PACK,
            SUPERHTML_PACK,
            CLANGD_PACK,
            TSGO_PACK,
            GOPLS_PACK,
            ZLS_PACK,
        ] {
            assert!(pins.iter().any(|p| p.name() == name));
        }
        let clangd = pins.iter().find(|p| p.name() == CLANGD_PACK).unwrap();
        assert_eq!(clangd.kind(), &PackKind::Cached);
        assert_eq!(clangd.sha().len(), 40);
        assert_ne!(clangd.sha(), "latest");
    }

    #[test]
    fn pack_build_plan_covers_heavy_kinds_without_docker() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let gopls = pins.iter().find(|p| p.name() == GOPLS_PACK).unwrap();
        let plan = PackBuildPlan::for_pin(
            root.path(),
            gopls,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(plan.kind(), &PackKind::Go);
        assert_eq!(plan.pack(), GOPLS_PACK);
        assert_eq!(plan.go_package(), Some("."));
        assert_eq!(
            plan.dest(),
            root.path()
                .join("target/musl")
                .join(AARCH64_MUSL)
                .join("engines/gopls/gopls")
        );
        let args = plan.docker_build_args();
        assert!(args.iter().any(|a| a == "CGO_ENABLED=0"));
        assert!(args.iter().any(|a| a == "GOOS=linux"));
        assert!(args.iter().any(|a| a == "GOARCH=arm64"));
        assert!(args.iter().any(|a| a == "GO_PACKAGE=."));
        assert!(args.iter().any(|a| a == "GO_VERSION=1.26"));
        assert!(args.iter().any(|a| a.contains("engine-pack-go.Dockerfile")));
        assert!(!args.iter().any(|a| a.contains("cmake")));

        let tsgo = pins.iter().find(|p| p.name() == TSGO_PACK).unwrap();
        let ts = PackBuildPlan::for_pin(
            root.path(),
            tsgo,
            X86_64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(ts.go_package(), Some("./cmd/tsgo"));
        assert!(ts.docker_build_args().iter().any(|a| a == "GOARCH=amd64"));

        let zls = pins.iter().find(|p| p.name() == ZLS_PACK).unwrap();
        let zplan = PackBuildPlan::for_pin(
            root.path(),
            zls,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(zplan.kind(), &PackKind::Zig);
        assert_eq!(zplan.dest().file_name().unwrap(), "zls");
        assert!(zplan
            .docker_build_args()
            .iter()
            .any(|a| a.contains("ZIG_TARGET=aarch64-linux-musl")));

        let clangd = pins.iter().find(|p| p.name() == CLANGD_PACK).unwrap();
        let cplan = PackBuildPlan::for_pin(
            root.path(),
            clangd,
            X86_64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(cplan.kind(), &PackKind::Cached);
        assert_eq!(cplan.cache_key(), format!("{}:{X86_64_MUSL}", clangd.sha()));
        assert_eq!(
            cplan.cache_src(),
            root.path()
                .join("target/pack-cache/clangd")
                .join(clangd.sha())
                .join(X86_64_MUSL)
                .join("clangd")
        );
        let cargs = cplan.docker_build_args();
        assert!(cargs
            .iter()
            .any(|a| a.contains("engine-pack-clangd.Dockerfile")));
        assert!(!cargs.iter().any(|a| a.contains("cache-fill")));
        assert!(!cargs
            .iter()
            .any(|a| a.to_ascii_lowercase().contains("cmake")));
        assert_eq!(cplan, cplan.clone());
    }

    #[test]
    fn pack_outcome_miss_is_documented_cache_gap_not_cmake() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &[
                "--pack".into(),
                "clangd".into(),
                "--target".into(),
                AARCH64_MUSL.into(),
            ],
            &docker,
        )
        .unwrap();
        assert!(docker.recorded_dests().is_empty());
        let dest = root
            .path()
            .join("target/musl")
            .join(AARCH64_MUSL)
            .join("engines/clangd/clangd");
        assert!(!dest.exists());
    }

    #[test]
    fn clangd_cache_hit_copies_without_cmake() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let clangd = pins.iter().find(|p| p.name() == CLANGD_PACK).unwrap();
        let plan = PackBuildPlan::for_pin(
            root.path(),
            clangd,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        fs::create_dir_all(plan.cache_src().parent().unwrap()).unwrap();
        fs::write(plan.cache_src(), check_static::fixture_static_elf64()).unwrap();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &[
                "--pack".into(),
                "clangd".into(),
                "--target".into(),
                AARCH64_MUSL.into(),
            ],
            &docker,
        )
        .unwrap();
        assert!(docker.recorded_dests().is_empty());
        check_static::check_path(plan.dest()).unwrap();
    }

    #[test]
    fn cache_fill_plan_is_value_object_cmake_kind_without_docker() {
        let root = fixture_root();
        let (pins, rust, zig, go) = load_pins(root.path()).unwrap();
        let clangd = pins.iter().find(|p| p.name() == CLANGD_PACK).unwrap();
        let plan = PackBuildPlan::cache_fill_for_pin(
            root.path(),
            clangd,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap();
        assert_eq!(plan.kind(), &PackKind::Cmake);
        assert_eq!(plan.dest(), plan.cache_src());
        let args = plan.docker_build_args();
        assert!(args
            .iter()
            .any(|a| a.contains("engine-pack-clangd-cache-fill.Dockerfile")));
        assert!(args.iter().any(|a| a.starts_with("CACHE_KEY=")));
        let gopls = pins.iter().find(|p| p.name() == GOPLS_PACK).unwrap();
        let err = PackBuildPlan::cache_fill_for_pin(
            root.path(),
            gopls,
            AARCH64_MUSL,
            rust.as_ref(),
            zig.as_ref(),
            go.as_ref(),
        )
        .unwrap_err();
        assert!(err.contains("clangd only"), "{err}");
        let parsed = parse_pack_args(&["--cache-fill".into()]).unwrap();
        assert_eq!(parsed.0, vec![CLANGD_PACK]);
        assert!(parsed.2);
        assert!(
            parse_pack_args(&["--cache-fill".into(), "--pack".into(), "gopls".into()]).is_err()
        );
    }

    #[test]
    fn recording_docker_port_extracts_gopls_and_zls_both_triples() {
        let root = fixture_root();
        let docker = RecordingDockerPort::new();
        run_at(
            root.path(),
            &["--pack".into(), "gopls,zls".into(), "--both".into()],
            &docker,
        )
        .unwrap();
        let dests = docker.recorded_dests();
        assert_eq!(dests.len(), 4);
        for d in &dests {
            check_static::check_path(d).unwrap();
            let name = d.file_name().unwrap();
            assert!(name == "gopls" || name == "zls", "{d:?}");
        }
    }

    #[test]
    fn command_docker_port_exists_for_production() {
        let _ = CommandDockerPort;
        let _ = RecordingDockerPort::default();
        let _ = PackKind::Cmake;
        let _ = is_heavy_pack(TSGO_PACK);
    }
}
