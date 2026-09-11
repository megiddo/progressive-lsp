//! SHA-pegged remote artifact store (POST-ART). Manifest schema, URL layout,
//! cache pull/push. No blobs in git; tests use `file://` and injected fetchers.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use progressive_lsp_install::hash::{hex_decode, hex_encode, sha256};
use serde::{Deserialize, Serialize};

pub const ARTIFACT_BASE_ENV: &str = "PROGRESSIVE_LSP_ARTIFACT_BASE";
pub const ARTIFACT_MANIFEST_ENV: &str = "PROGRESSIVE_LSP_ARTIFACT_MANIFEST";
pub const ARTIFACT_DEFAULT_FORMAT: &str = "tar.gz";

/// Archive extension for [`ArtifactFormat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    #[serde(rename = "tar.gz")]
    TarGz,
    Tar,
    Zip,
    #[serde(rename = "tar.bz2")]
    TarBz2,
}

impl ArtifactFormat {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "tar.gz" => Ok(Self::TarGz),
            "tar" => Ok(Self::Tar),
            "zip" => Ok(Self::Zip),
            "tar.bz2" => Ok(Self::TarBz2),
            other => Err(format!(
                "unknown artifact format {other}; expected tar.gz, tar, zip, or tar.bz2"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Tar => "tar",
            Self::Zip => "zip",
            Self::TarBz2 => "tar.bz2",
        }
    }

    pub fn file_suffix(self) -> &'static str {
        self.as_str()
    }
}

/// One row in the artifact store manifest (POST-ART.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreArtifact {
    pub pack: String,
    pub upstream_sha: String,
    pub triple: String,
    pub sha256: String,
    pub url: String,
    pub format: ArtifactFormat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreManifest {
    pub version: String,
    pub artifacts: Vec<StoreArtifact>,
}

impl StoreManifest {
    pub fn parse(json: &str) -> Result<Self, String> {
        let m: StoreManifest =
            serde_json::from_str(json).map_err(|e| format!("artifact manifest JSON: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version.is_empty() {
            return Err("artifact manifest: version is required".into());
        }
        if self.artifacts.is_empty() {
            return Err("artifact manifest: artifacts must not be empty".into());
        }
        for a in &self.artifacts {
            a.validate()?;
        }
        Ok(())
    }

    pub fn find(&self, pack: &str, upstream_sha: &str, triple: &str) -> Option<&StoreArtifact> {
        self.artifacts.iter().find(|a| {
            a.pack == pack && a.upstream_sha == upstream_sha && a.triple == triple
        })
    }
}

impl StoreArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.pack.is_empty() {
            return Err("artifact entry: pack is required".into());
        }
        if self.upstream_sha.len() != 40
            || !self
                .upstream_sha
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(format!(
                "artifact entry: upstream_sha must be 40 hex chars, got {}",
                self.upstream_sha
            ));
        }
        if self.triple.is_empty() {
            return Err("artifact entry: triple is required".into());
        }
        let bytes = hex_decode(&self.sha256).map_err(|e| format!("artifact entry: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "artifact entry: sha256 for {} must be 32 bytes",
                self.pack
            ));
        }
        if self.url.is_empty() {
            return Err("artifact entry: url is required".into());
        }
        Ok(())
    }

    pub fn sha256_bytes(&self) -> Result<[u8; 32], String> {
        let v = hex_decode(&self.sha256).map_err(|e| e.to_string())?;
        if v.len() != 32 {
            return Err("sha256 must be 32 bytes".into());
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }
}

/// POST-ART.2 layout: `{base}/engines/{pack}/{upstream_sha}/{triple}.{format}`.
pub fn artifact_url_from_base(
    base: &str,
    pack: &str,
    upstream_sha: &str,
    triple: &str,
    format: ArtifactFormat,
) -> String {
    let base = base.trim_end_matches('/');
    format!(
        "{base}/engines/{pack}/{upstream_sha}/{triple}.{}",
        format.file_suffix()
    )
}

pub trait ByteFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// Production fetcher (`http(s)://` via ureq; `file://` for tests).
pub struct NetworkFetcher;

impl ByteFetcher for NetworkFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        fetch_bytes(url)
    }
}

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).map_err(|e| format!("read {url}: {e}"));
    }
    if url.starts_with("file:") {
        return Err(format!("unsupported file URL (use file://): {url}"));
    }
    let out = std::process::Command::new("curl")
        .args(["-fsSL", url])
        .output()
        .map_err(|e| format!("curl GET {url}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl GET {url}: exit {} stderr {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

fn curl_put(url: &str, body: &[u8]) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!(
        "progressive-lsp-artifact-upload-{}",
        std::process::id()
    ));
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "-X", "PUT", "-T"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| format!("curl PUT {url}: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if !status.success() {
        return Err(format!("curl PUT {url}: exit {status}"));
    }
    Ok(())
}

pub fn load_store_manifest(_root: &Path, fetcher: &dyn ByteFetcher) -> Result<StoreManifest, String> {
    let path = std::env::var(ARTIFACT_MANIFEST_ENV).map_err(|_| {
        format!("set {ARTIFACT_MANIFEST_ENV} to a local path or https URL")
    })?;
    let json = if path.starts_with("http://") || path.starts_with("https://") {
        String::from_utf8(fetcher.fetch(&path)?).map_err(|e| format!("manifest UTF-8: {e}"))?
    } else {
        std::fs::read_to_string(&path).map_err(|e| format!("read manifest {path}: {e}"))?
    };
    StoreManifest::parse(&json)
}

pub fn resolve_store_entry(
    root: &Path,
    pack: &str,
    upstream_sha: &str,
    triple: &str,
    fetcher: &dyn ByteFetcher,
) -> Result<StoreArtifact, String> {
    if std::env::var(ARTIFACT_MANIFEST_ENV).is_ok() {
        let manifest = load_store_manifest(root, fetcher)?;
        if let Some(entry) = manifest.find(pack, upstream_sha, triple) {
            return Ok(entry.clone());
        }
    }
    let base = std::env::var(ARTIFACT_BASE_ENV).map_err(|_| {
        format!(
            "no manifest entry for {pack}:{upstream_sha}:{triple}; set {ARTIFACT_BASE_ENV} \
             or {ARTIFACT_MANIFEST_ENV}"
        )
    })?;
    let format = ArtifactFormat::parse(
        std::env::var("PROGRESSIVE_LSP_ARTIFACT_FORMAT")
            .as_deref()
            .unwrap_or(ARTIFACT_DEFAULT_FORMAT),
    )?;
    Ok(StoreArtifact {
        pack: pack.to_string(),
        upstream_sha: upstream_sha.to_string(),
        triple: triple.to_string(),
        sha256: String::new(),
        url: artifact_url_from_base(&base, pack, upstream_sha, triple, format),
        format,
    })
}

fn verify_payload_hash(payload: &[u8], want: Option<[u8; 32]>) -> Result<(), String> {
    let Some(want) = want else {
        return Ok(());
    };
    let got = sha256(payload);
    if got != want {
        return Err(format!(
            "sha256 mismatch: expected {}, got {}",
            hex_encode(&want),
            hex_encode(&got)
        ));
    }
    Ok(())
}

/// Extract `binary_name` from an archive into `dest`.
pub fn extract_binary_from_archive(
    payload: &[u8],
    format: ArtifactFormat,
    binary_name: &str,
    dest: &Path,
) -> Result<(), String> {
    match format {
        ArtifactFormat::Tar => extract_ustar_member(payload, binary_name, dest),
        ArtifactFormat::TarGz => {
            let mut dec = GzDecoder::new(payload);
            let mut buf = Vec::new();
            dec.read_to_end(&mut buf)
                .map_err(|e| format!("gzip decode: {e}"))?;
            extract_ustar_member(&buf, binary_name, dest)
        }
        ArtifactFormat::Zip | ArtifactFormat::TarBz2 => Err(format!(
            "artifact format {} is documented but not implemented in xtask yet; use tar or tar.gz",
            format.as_str()
        )),
    }
}

fn extract_ustar_member(payload: &[u8], binary_name: &str, dest: &Path) -> Result<(), String> {
    const BLOCK: usize = 512;
    let mut off = 0usize;
    while off + BLOCK <= payload.len() {
        let header = &payload[off..off + BLOCK];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name = read_tar_name(header)?;
        let size = read_tar_size(header)?;
        off += BLOCK;
        let end = off
            .checked_add(size as usize)
            .ok_or("tar size overflow")?;
        if end > payload.len() {
            return Err("tar truncated".into());
        }
        let data = &payload[off..end];
        off = end + (BLOCK - (size as usize % BLOCK)) % BLOCK;
        let base = name.rsplit('/').next().unwrap_or(&name);
        if base == binary_name {
            write_atomic(dest, data)?;
            return Ok(());
        }
    }
    Err(format!("tar archive missing member {binary_name}"))
}

fn read_tar_name(header: &[u8]) -> Result<String, String> {
    let slot = &header[0..100];
    let end = slot.iter().position(|&b| b == 0).unwrap_or(slot.len());
    let name = std::str::from_utf8(&slot[..end])
        .map_err(|e| format!("tar name utf-8: {e}"))?
        .to_string();
    if name.is_empty() {
        return Err("tar empty name".into());
    }
    Ok(name)
}

fn read_tar_size(header: &[u8]) -> Result<u64, String> {
    let slot = std::str::from_utf8(&header[124..136])
        .map_err(|e| format!("tar size utf-8: {e}"))?
        .trim_end_matches('\0')
        .trim();
    if slot.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(slot, 8).map_err(|e| format!("tar size octal: {e}"))
}

fn write_atomic(dest: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename {}: {e}", dest.display()))?;
    Ok(())
}

/// Pull one cached binary into `cache_dest`. Returns true when the file exists after pull.
pub fn pull_cache_binary(
    root: &Path,
    pack: &str,
    upstream_sha: &str,
    triple: &str,
    binary: &str,
    cache_dest: &Path,
    fetcher: &dyn ByteFetcher,
) -> Result<bool, String> {
    if cache_dest.is_file() {
        return Ok(true);
    }
    let entry = resolve_store_entry(root, pack, upstream_sha, triple, fetcher)?;
    let want_hash = if entry.sha256.is_empty() {
        None
    } else {
        Some(entry.sha256_bytes()?)
    };
    let payload = fetcher.fetch(&entry.url)?;
    verify_payload_hash(&payload, want_hash)?;
    extract_binary_from_archive(&payload, entry.format, binary, cache_dest)?;
    eprintln!(
        "xtask pack: cache pull {} ({}) -> {}",
        pack,
        triple,
        cache_dest.display()
    );
    Ok(true)
}

/// Maintainer helper: emit manifest JSON for a local cache file (no upload unless env set).
pub fn push_cache_binary(
    pack: &str,
    upstream_sha: &str,
    triple: &str,
    binary: &str,
    cache_src: &Path,
    format: ArtifactFormat,
) -> Result<(), String> {
    if !cache_src.is_file() {
        return Err(format!("cache push: missing {}", cache_src.display()));
    }
    let bytes = std::fs::read(cache_src)
        .map_err(|e| format!("read {}: {e}", cache_src.display()))?;
    let digest = hex_encode(&sha256(&bytes));
    let url = std::env::var(ARTIFACT_BASE_ENV).map(|base| {
        artifact_url_from_base(&base, pack, upstream_sha, triple, format)
    });
    let entry = StoreArtifact {
        pack: pack.to_string(),
        upstream_sha: upstream_sha.to_string(),
        triple: triple.to_string(),
        sha256: digest.clone(),
        url: url.clone().unwrap_or_else(|_| "<set PROGRESSIVE_LSP_ARTIFACT_BASE>".into()),
        format,
    };
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    eprintln!("xtask pack: cache push manifest row:\n{json}");
    if std::env::var("PROGRESSIVE_LSP_ARTIFACT_PUSH").as_deref() == Ok("1") {
        let base = url.map_err(|_| format!("cache push upload needs {ARTIFACT_BASE_ENV}"))?;
        let archive = tar_single_member(binary, &bytes)?;
        let upload = match format {
            ArtifactFormat::Tar => archive,
            ArtifactFormat::TarGz => gzip_bytes(&archive)?,
            other => {
                return Err(format!(
                    "cache push upload supports tar/tar.gz only, not {}",
                    other.as_str()
                ));
            }
        };
        let digest_upload = hex_encode(&sha256(&upload));
        curl_put(&base, &upload)?;
        eprintln!("xtask pack: uploaded {base} (archive sha256 {digest_upload})");
    }
    Ok(())
}

fn tar_single_member(name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    crate::tarball::write_ustar(&[(name.to_string(), bytes.to_vec())])
}

fn gzip_bytes(raw: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(raw).map_err(|e| format!("gzip: {e}"))?;
    enc.finish().map_err(|e| format!("gzip finish: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_static;
    use crate::tarball;

    #[test]
    fn store_manifest_parses_example_shape() {
        let elf = check_static::fixture_static_elf64();
        let tar = tarball::write_ustar(&[("clangd".into(), elf.clone())]).unwrap();
        let digest = hex_encode(&sha256(&tar));
        let json = format!(
            r#"{{
  "version": "1",
  "artifacts": [{{
    "pack": "clangd",
    "upstream_sha": "3623fe661ae35c6c80ac221f14d85be76aa870f1",
    "triple": "x86_64-unknown-linux-musl",
    "sha256": "{digest}",
    "url": "file:///tmp/clangd.tar",
    "format": "tar"
  }}]
}}"#
        );
        let m = StoreManifest::parse(&json).unwrap();
        assert_eq!(m.artifacts.len(), 1);
        assert_eq!(m.artifacts[0].pack, "clangd");
    }

    #[test]
    fn artifact_url_layout_matches_convention() {
        let url = artifact_url_from_base(
            "https://cdn.example/progressive-lsp",
            "clangd",
            "abc123",
            "aarch64-unknown-linux-musl",
            ArtifactFormat::TarGz,
        );
        assert_eq!(
            url,
            "https://cdn.example/progressive-lsp/engines/clangd/abc123/aarch64-unknown-linux-musl.tar.gz"
        );
    }

    #[test]
    fn pull_verifies_sha256_and_writes_cache() {
        let dir = tempfile::tempdir().unwrap();
        let elf = check_static::fixture_static_elf64();
        let tar = tarball::write_ustar(&[("clangd".into(), elf)]).unwrap();
        let digest = hex_encode(&sha256(&tar));
        let tar_path = dir.path().join("clangd.tar");
        std::fs::write(&tar_path, &tar).unwrap();
        let url = format!("file://{}", tar_path.display());
        let json = format!(
            r#"{{
  "version": "1",
  "artifacts": [{{
    "pack": "clangd",
    "upstream_sha": "3623fe661ae35c6c80ac221f14d85be76aa870f1",
    "triple": "x86_64-unknown-linux-musl",
    "sha256": "{digest}",
    "url": "{url}",
    "format": "tar"
  }}]
}}"#
        );
        std::fs::write(dir.path().join("manifest.json"), &json).unwrap();
        std::env::set_var(ARTIFACT_MANIFEST_ENV, dir.path().join("manifest.json"));
        let dest = dir.path().join("cache/clangd");
        let fetcher = NetworkFetcher;
        pull_cache_binary(
            dir.path(),
            "clangd",
            "3623fe661ae35c6c80ac221f14d85be76aa870f1",
            "x86_64-unknown-linux-musl",
            "clangd",
            &dest,
            &fetcher,
        )
        .unwrap();
        assert!(dest.is_file());
        std::env::remove_var(ARTIFACT_MANIFEST_ENV);
    }

    #[test]
    fn pull_rejects_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let tar = tarball::write_ustar(&[("clangd".into(), b"x".to_vec())]).unwrap();
        let tar_path = dir.path().join("clangd.tar");
        std::fs::write(&tar_path, &tar).unwrap();
        let url = format!("file://{}", tar_path.display());
        let json = format!(
            r#"{{
  "version": "1",
  "artifacts": [{{
    "pack": "clangd",
    "upstream_sha": "3623fe661ae35c6c80ac221f14d85be76aa870f1",
    "triple": "x86_64-unknown-linux-musl",
    "sha256": "{}",
    "url": "{url}",
    "format": "tar"
  }}]
}}"#,
            "0".repeat(64)
        );
        std::fs::write(dir.path().join("manifest.json"), &json).unwrap();
        std::env::set_var(ARTIFACT_MANIFEST_ENV, dir.path().join("manifest.json"));
        let err = pull_cache_binary(
            dir.path(),
            "clangd",
            "3623fe661ae35c6c80ac221f14d85be76aa870f1",
            "x86_64-unknown-linux-musl",
            "clangd",
            &dir.path().join("cache/clangd"),
            &NetworkFetcher,
        )
        .unwrap_err();
        assert!(err.contains("sha256 mismatch"), "{err}");
        std::env::remove_var(ARTIFACT_MANIFEST_ENV);
    }
}
