//! `manifest.json` schema.

use serde::{Deserialize, Serialize};

use crate::hash::{hex_decode, hex_encode};
use progressive_lsp_core::InstallError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub artifacts: Vec<ManifestArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestArtifact {
    pub name: String,
    pub rel_path: String,
    pub sha256: String,
    #[serde(default)]
    pub executable: bool,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self, InstallError> {
        let m: Manifest = serde_json::from_str(json)
            .map_err(|e| InstallError::Manifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), InstallError> {
        if self.version.is_empty() {
            return Err(InstallError::Manifest("version is required".into()));
        }
        if self.artifacts.is_empty() {
            return Err(InstallError::Manifest("artifacts must not be empty".into()));
        }
        for a in &self.artifacts {
            if a.name.is_empty() {
                return Err(InstallError::Manifest("artifact name is required".into()));
            }
            if a.rel_path.is_empty() || PathLooksAbsolute::is_absolute(&a.rel_path) {
                return Err(InstallError::Manifest(format!(
                    "rel_path must be a relative path: {}",
                    a.rel_path
                )));
            }
            if a.rel_path.contains("..") {
                return Err(InstallError::Manifest(format!(
                    "rel_path must not contain ..: {}",
                    a.rel_path
                )));
            }
            let bytes = hex_decode(&a.sha256)?;
            if bytes.len() != 32 {
                return Err(InstallError::Manifest(format!(
                    "sha256 for {} must be 32 bytes",
                    a.name
                )));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, InstallError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| InstallError::Manifest(e.to_string()))
    }
}

impl ManifestArtifact {
    pub fn sha256_bytes(&self) -> Result<[u8; 32], InstallError> {
        let v = hex_decode(&self.sha256)?;
        if v.len() != 32 {
            return Err(InstallError::Manifest("sha256 must be 32 bytes".into()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }
}

struct PathLooksAbsolute;

impl PathLooksAbsolute {
    fn is_absolute(p: &str) -> bool {
        p.starts_with('/') || p.starts_with('\\') || (p.len() >= 2 && p.as_bytes()[1] == b':')
    }
}

pub fn example_manifest_json(sha: &[u8; 32]) -> String {
    format!(
        r#"{{
  "version": "1",
  "artifacts": [
    {{
      "name": "progressive-lsp",
      "rel_path": "bin/progressive-lsp",
      "sha256": "{}",
      "executable": true
    }}
  ]
}}"#,
        hex_encode(sha)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256;

    #[test]
    fn parse_valid_manifest() {
        let json = example_manifest_json(&sha256(b"x"));
        let m = Manifest::parse(&json).unwrap();
        assert_eq!(m.version, "1");
        assert_eq!(m.artifacts[0].name, "progressive-lsp");
        assert!(m.artifacts[0].executable);
        assert_eq!(m.artifacts[0].sha256_bytes().unwrap(), sha256(b"x"));
        assert!(m.to_json().unwrap().contains("progressive-lsp"));
    }

    #[test]
    fn rejects_bad_schema() {
        assert!(Manifest::parse("{}").is_err());
        assert!(Manifest::parse("[").is_err());
        assert!(Manifest::parse(r#"{"version":"","artifacts":[{"name":"a","rel_path":"b","sha256":"aa"}]}"#).is_err());
        let empty_art = r#"{"version":"1","artifacts":[]}"#;
        assert!(Manifest::parse(empty_art).is_err());
        let abs = format!(
            r#"{{"version":"1","artifacts":[{{"name":"a","rel_path":"/etc/passwd","sha256":"{}"}}]}}"#,
            hex_encode(&sha256(b"x"))
        );
        assert!(Manifest::parse(&abs).is_err());
        let dots = format!(
            r#"{{"version":"1","artifacts":[{{"name":"a","rel_path":"foo/../x","sha256":"{}"}}]}}"#,
            hex_encode(&sha256(b"x"))
        );
        assert!(Manifest::parse(&dots).is_err());
        let no_name = format!(
            r#"{{"version":"1","artifacts":[{{"name":"","rel_path":"bin/x","sha256":"{}"}}]}}"#,
            hex_encode(&sha256(b"x"))
        );
        assert!(Manifest::parse(&no_name).is_err());
        let short = r#"{"version":"1","artifacts":[{"name":"a","rel_path":"bin/x","sha256":"abcd"}]}"#;
        assert!(Manifest::parse(short).is_err());
    }

    #[test]
    fn windows_absolute_rejected() {
        let json = format!(
            r#"{{"version":"1","artifacts":[{{"name":"a","rel_path":"C:\\x","sha256":"{}"}}]}}"#,
            hex_encode(&sha256(b"x"))
        );
        assert!(Manifest::parse(&json).is_err());
    }
}
