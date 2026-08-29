//! Interned identity newtypes and language version value objects.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use semver::Version;
use sha2::{Digest, Sha256};

use crate::error::ConfigError;

/// Interned language id (e.g. `"java"`). Equality is id equality.
#[derive(Clone, Debug, Eq)]
pub struct LanguageId(Arc<str>);

impl LanguageId {
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(Arc::from(id.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for LanguageId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref() == other.0.as_ref()
    }
}

impl Hash for LanguageId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ref().hash(state);
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interned package id. Equality is id equality.
#[derive(Clone, Debug, Eq)]
pub struct PackageId(Arc<str>);

impl PackageId {
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(Arc::from(id.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for PackageId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref() == other.0.as_ref()
    }
}

impl Hash for PackageId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ref().hash(state);
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interned file id. Equality is id equality.
#[derive(Clone, Debug, Eq)]
pub struct FileId(Arc<str>);

impl FileId {
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(Arc::from(id.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for FileId {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref() == other.0.as_ref()
    }
}

impl Hash for FileId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ref().hash(state);
    }
}

impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// SHA-256 of the canonical absolute workspace path. Stable across reconnects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WorkspaceId([u8; 32]);

impl WorkspaceId {
    pub fn from_workspace_path(path: &Path) -> Result<Self, ConfigError> {
        let canonical = canonicalize_workspace(path)?;
        Ok(Self::from_canonical_bytes(&path_fingerprint(&canonical)))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        Self(out)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex_encode(&self.0)
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Intelligence tier: T1 syntax, T2 graph, T3 types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Syntax,
    Graph,
    Types,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Graph => "graph",
            Self::Types => "types",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(Self::Syntax),
            "graph" => Some(Self::Graph),
            "types" => Some(Self::Types),
            _ => None,
        }
    }
}

/// Effective supported version = `min(window, grammar, engine)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageVersion {
    pub language: LanguageId,
    pub effective: Version,
    pub window_latest: Version,
    pub grammar_pin: String,
    pub engine_pin: Option<String>,
}

impl LanguageVersion {
    pub fn compute(
        language: LanguageId,
        window_latest: Version,
        grammar_version: Version,
        engine_version: Option<Version>,
        grammar_pin: impl Into<String>,
        engine_pin: Option<String>,
    ) -> Self {
        let mut effective = min_version(window_latest.clone(), grammar_version);
        if let Some(engine) = engine_version {
            effective = min_version(effective, engine);
        }
        Self {
            language,
            effective,
            window_latest,
            grammar_pin: grammar_pin.into(),
            engine_pin,
        }
    }
}

fn min_version(a: Version, b: Version) -> Version {
    if a <= b {
        a
    } else {
        b
    }
}

fn canonicalize_workspace(path: &Path) -> Result<std::path::PathBuf, ConfigError> {
    if path.as_os_str().is_empty() {
        return Err(ConfigError::Prefix("workspace path is empty".into()));
    }
    std::fs::canonicalize(path).map_err(|e| {
        ConfigError::Io(format!("canonicalize {}: {e}", path.display()))
    })
}

fn path_fingerprint(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn language_id_equality_is_value() {
        let a = LanguageId::new("java");
        let b = LanguageId::new(String::from("java"));
        let c = LanguageId::new("rust");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_str(), "java");
        assert_eq!(a.to_string(), "java");
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }

    #[test]
    fn package_and_file_id_equality() {
        assert_eq!(PackageId::new("p"), PackageId::new("p"));
        assert_ne!(PackageId::new("p"), PackageId::new("q"));
        assert_eq!(PackageId::new("p").as_str(), "p");
        assert_eq!(PackageId::new("p").to_string(), "p");
        assert_eq!(FileId::new("f"), FileId::new("f"));
        assert_ne!(FileId::new("f"), FileId::new("g"));
        assert_eq!(FileId::new("f").as_str(), "f");
        assert_eq!(FileId::new("f").to_string(), "f");
        let mut pkgs = HashSet::new();
        pkgs.insert(PackageId::new("p"));
        assert!(pkgs.contains(&PackageId::new("p")));
        let mut files = HashSet::new();
        files.insert(FileId::new("f"));
        assert!(files.contains(&FileId::new("f")));
    }

    #[test]
    fn workspace_id_is_sha256_of_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let id = WorkspaceId::from_workspace_path(dir.path()).unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let expected = WorkspaceId::from_canonical_bytes(&path_fingerprint(&canon));
        assert_eq!(id, expected);
        assert_eq!(id, WorkspaceId::from_workspace_path(dir.path()).unwrap());
        assert_eq!(id.as_bytes(), expected.as_bytes());
        assert_ne!(id.as_bytes(), &[0u8; 32]);
        assert_ne!(id.as_bytes(), &[1u8; 32]);
        assert_eq!(id.to_hex().len(), 64);
        assert_eq!(id.to_hex(), expected.to_hex());
        assert_ne!(id.to_hex(), String::new());
        assert_ne!(id, WorkspaceId::default());
        assert_eq!(id.to_string(), id.to_hex());
        assert!(id.to_hex().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn workspace_id_differs_for_distinct_paths() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let id_a = WorkspaceId::from_workspace_path(a.path()).unwrap();
        let id_b = WorkspaceId::from_workspace_path(b.path()).unwrap();
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn workspace_id_rejects_empty_and_missing() {
        let err = WorkspaceId::from_workspace_path(Path::new("")).unwrap_err();
        assert!(matches!(err, ConfigError::Prefix(_)));
        let missing = WorkspaceId::from_workspace_path(Path::new(
            "/definitely-not-a-workspace-path-progressive-lsp",
        ))
        .unwrap_err();
        assert!(matches!(missing, ConfigError::Io(_)));
    }

    #[test]
    fn hex_encode_is_lowercase_bytes() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x0a]), "00ff0a");
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x10]), "10");
    }

    #[test]
    fn tier_round_trip_and_unknown() {
        for (tier, name) in [
            (Tier::Syntax, "syntax"),
            (Tier::Graph, "graph"),
            (Tier::Types, "types"),
        ] {
            assert_eq!(tier.as_str(), name);
            assert_eq!(Tier::parse(name), Some(tier));
        }
        assert_eq!(Tier::parse("SYNTAX"), None);
        assert_eq!(Tier::parse(""), None);
        assert_eq!(Tier::parse("t4"), None);
    }

    #[test]
    fn language_version_effective_is_min_of_three() {
        let v = LanguageVersion::compute(
            LanguageId::new("java"),
            Version::new(26, 0, 0),
            Version::new(25, 0, 0),
            Some(Version::new(24, 0, 0)),
            "grammar-25",
            Some("engine-24".into()),
        );
        assert_eq!(v.effective, Version::new(24, 0, 0));
        assert_eq!(v.window_latest, Version::new(26, 0, 0));
        assert_eq!(v.grammar_pin, "grammar-25");
        assert_eq!(v.engine_pin.as_deref(), Some("engine-24"));
        assert_eq!(v.language.as_str(), "java");
    }

    #[test]
    fn language_version_min_uses_window_when_smallest() {
        let v = LanguageVersion::compute(
            LanguageId::new("go"),
            Version::new(1, 25, 0),
            Version::new(1, 27, 0),
            Some(Version::new(1, 26, 0)),
            "g",
            Some("e".into()),
        );
        assert_eq!(v.effective, Version::new(1, 25, 0));
    }

    #[test]
    fn language_version_min_uses_grammar_when_smallest() {
        let v = LanguageVersion::compute(
            LanguageId::new("php"),
            Version::new(8, 5, 0),
            Version::new(8, 3, 0),
            Some(Version::new(8, 4, 0)),
            "g",
            None,
        );
        assert_eq!(v.effective, Version::new(8, 3, 0));
        assert_eq!(v.engine_pin, None);
    }

    #[test]
    fn language_version_without_engine_is_min_window_grammar() {
        let v = LanguageVersion::compute(
            LanguageId::new("java"),
            Version::new(26, 0, 0),
            Version::new(26, 0, 0),
            None,
            "g",
            None,
        );
        assert_eq!(v.effective, Version::new(26, 0, 0));
    }

    #[test]
    fn min_version_prefers_equal_left() {
        let a = Version::new(1, 0, 0);
        assert_eq!(min_version(a.clone(), Version::new(1, 0, 0)), a);
        assert_eq!(min_version(Version::new(2, 0, 0), Version::new(1, 0, 0)), Version::new(1, 0, 0));
    }
}
