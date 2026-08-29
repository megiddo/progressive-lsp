//! Dist `manifest.json`: core crate semver is independent of engine SHAs.
//! Engine SHAs live on pack [`crate::Manifest`] artifacts, not in Cargo.toml.

use serde::{Deserialize, Serialize};

use progressive_lsp_core::InstallError;

/// Product proto package. Breaking wire changes → `progressive.v2`.
pub const DIST_PROTO: &str = "progressive.v1";

/// Darwin / local `xtask dist` writes this payload kind. Real musl ELFs are Linux CI.
pub const DIST_PAYLOAD_STUB: &str = "stub";

/// Per-triple musl targets that Linux CI must publish as the real dist.
pub const MUSL_TRIPLES: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
];

pub const DIST_TRIPLES: &[&str] = MUSL_TRIPLES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistManifest {
    pub schema_version: String,
    /// Workspace / core crate semver. Not an engine SHA.
    pub core_version: String,
    pub proto: String,
    /// `stub` on Darwin (and any host that only packs stubs). Never claim `musl-elf` here.
    pub payload_kind: String,
    pub artifacts: Vec<DistArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistArtifact {
    pub triple: String,
    pub flavor: String,
    pub rel_path: String,
    pub sha256: String,
}

impl DistManifest {
    pub fn new(core_version: impl Into<String>, payload_kind: impl Into<String>) -> Self {
        Self {
            schema_version: "1".into(),
            core_version: core_version.into(),
            proto: DIST_PROTO.into(),
            payload_kind: payload_kind.into(),
            artifacts: Vec::new(),
        }
    }

    pub fn parse(json: &str) -> Result<Self, InstallError> {
        let m: Self =
            serde_json::from_str(json).map_err(|e| InstallError::Manifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), InstallError> {
        if self.schema_version.is_empty() {
            return Err(InstallError::Manifest("schema_version is required".into()));
        }
        if self.core_version.is_empty() {
            return Err(InstallError::Manifest("core_version is required".into()));
        }
        if self.proto != DIST_PROTO {
            return Err(InstallError::Manifest(format!(
                "proto must be {DIST_PROTO}, got {}",
                self.proto
            )));
        }
        if self.payload_kind.is_empty() {
            return Err(InstallError::Manifest("payload_kind is required".into()));
        }
        for a in &self.artifacts {
            if a.triple.is_empty() || a.flavor.is_empty() || a.rel_path.is_empty() {
                return Err(InstallError::Manifest("dist artifact fields required".into()));
            }
            if a.rel_path.contains("..") || a.rel_path.starts_with('/') {
                return Err(InstallError::Manifest(format!(
                    "rel_path must be relative: {}",
                    a.rel_path
                )));
            }
            if a.sha256.len() != 64 {
                return Err(InstallError::Manifest(format!(
                    "sha256 for {} must be 32 bytes hex",
                    a.rel_path
                )));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, InstallError> {
        serde_json::to_string_pretty(self).map_err(|e| InstallError::Manifest(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_is_not_an_engine_sha() {
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: MUSL_TRIPLES[0].into(),
            flavor: "slim".into(),
            rel_path: "x86_64-unknown-linux-musl/slim.tar".into(),
            sha256: "aa".repeat(32),
        });
        let json = m.to_json().unwrap();
        let parsed = DistManifest::parse(&json).unwrap();
        assert_eq!(parsed.core_version, "0.1.0");
        assert_eq!(parsed.proto, DIST_PROTO);
        assert_eq!(parsed.payload_kind, DIST_PAYLOAD_STUB);
        assert_ne!(parsed.core_version, parsed.artifacts[0].sha256);
        assert_eq!(DIST_TRIPLES.len(), 2);
        assert!(MUSL_TRIPLES.contains(&"aarch64-unknown-linux-musl"));
    }

    #[test]
    fn rejects_bad_dist_manifest() {
        assert!(DistManifest::parse("{}").is_err());
        assert!(DistManifest::parse("[").is_err());
        let mut m = DistManifest::new("", DIST_PAYLOAD_STUB);
        m.schema_version = "1".into();
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.schema_version.clear();
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.proto = "progressive.v2".into();
        assert!(m.validate().is_err());
        let m = DistManifest::new("0.1.0", "");
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: String::new(),
            flavor: "slim".into(),
            rel_path: "x.tar".into(),
            sha256: "aa".repeat(32),
        });
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: "t".into(),
            flavor: String::new(),
            rel_path: "x.tar".into(),
            sha256: "aa".repeat(32),
        });
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: "t".into(),
            flavor: "slim".into(),
            rel_path: String::new(),
            sha256: "aa".repeat(32),
        });
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: "t".into(),
            flavor: "slim".into(),
            rel_path: "/abs.tar".into(),
            sha256: "aa".repeat(32),
        });
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: "t".into(),
            flavor: "slim".into(),
            rel_path: "foo/../x.tar".into(),
            sha256: "aa".repeat(32),
        });
        assert!(m.validate().is_err());
        let mut m = DistManifest::new("0.1.0", DIST_PAYLOAD_STUB);
        m.artifacts.push(DistArtifact {
            triple: "t".into(),
            flavor: "slim".into(),
            rel_path: "x.tar".into(),
            sha256: "abcd".into(),
        });
        assert!(m.validate().is_err());
    }
}
