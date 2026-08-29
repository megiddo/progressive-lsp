//! Domain errors. User paths never `unwrap`.

use std::fmt;

use crate::ids::{LanguageId, PackageId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedLanguage {
    pub language: LanguageId,
}

impl UnsupportedLanguage {
    pub fn new(language: impl Into<LanguageId>) -> Self {
        Self {
            language: language.into(),
        }
    }
}

impl From<&str> for LanguageId {
    fn from(value: &str) -> Self {
        LanguageId::new(value)
    }
}

impl From<String> for LanguageId {
    fn from(value: String) -> Self {
        LanguageId::new(value)
    }
}

impl fmt::Display for UnsupportedLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported language: {}", self.language)
    }
}

impl std::error::Error for UnsupportedLanguage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineNotReady {
    pub language: LanguageId,
    pub package: PackageId,
}

impl EngineNotReady {
    pub fn new(language: LanguageId, package: PackageId) -> Self {
        Self { language, package }
    }
}

impl fmt::Display for EngineNotReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "engine not ready for {}/{}",
            self.language, self.package
        )
    }
}

impl std::error::Error for EngineNotReady {}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum InstallError {
    #[error("hash mismatch: expected {expected}, got {actual}")]
    Hash { expected: String, actual: String },
    #[error("transport: {0}")]
    Transport(String),
    #[error("io: {0}")]
    Io(String),
    #[error("manifest: {0}")]
    Manifest(String),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("static link check failed: {0}")]
pub struct StaticLinkError(pub String);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("script abort: {0}")]
pub struct ScriptAbort(pub String);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("script sandbox: {0}")]
pub struct ScriptSandbox(pub String);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid TOML: {0}")]
    Toml(String),
    #[error("io: {0}")]
    Io(String),
    #[error("prefix: {0}")]
    Prefix(String),
    #[error("home directory is unset")]
    HomeUnset,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("watch overflow at generation {generation}")]
pub struct WatchOverflow {
    pub generation: u64,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("initialize failed: {0}")]
pub struct InitializeFailed(pub String);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EngineError {
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("not discovered: {0}")]
    NotDiscovered(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    Hash { expected: String, actual: String },
    #[error("engine crashed: {0}")]
    Crashed(String),
    #[error("backoff until unix_ms {next_unix_ms}")]
    Backoff { next_unix_ms: u64 },
    #[error("spawn aborted: {0}")]
    Aborted(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_language_display_and_eq() {
        let e = UnsupportedLanguage::new("zig");
        assert_eq!(e.language.as_str(), "zig");
        assert_eq!(e.to_string(), "unsupported language: zig");
        assert_eq!(e, UnsupportedLanguage::new(LanguageId::new("zig")));
        let _ = LanguageId::from("php");
        let _ = LanguageId::from(String::from("go"));
    }

    #[test]
    fn engine_not_ready_names_language_and_package() {
        let e = EngineNotReady::new(LanguageId::new("python"), PackageId::new("pkg"));
        assert_eq!(e.to_string(), "engine not ready for python/pkg");
    }

    #[test]
    fn install_error_hash_is_distinct_from_transport() {
        let hash = InstallError::Hash {
            expected: "aa".into(),
            actual: "bb".into(),
        };
        assert!(hash.to_string().contains("aa"));
        assert!(hash.to_string().contains("bb"));
        assert_ne!(hash, InstallError::Transport("x".into()));
        assert_ne!(hash, InstallError::Io("x".into()));
        assert_ne!(hash, InstallError::Manifest("x".into()));
        assert!(InstallError::Transport("x".into()).to_string().contains("transport"));
    }

    #[test]
    fn remaining_error_displays() {
        assert_eq!(
            StaticLinkError("interp".into()).to_string(),
            "static link check failed: interp"
        );
        assert_eq!(ScriptAbort("no".into()).to_string(), "script abort: no");
        assert_eq!(
            ScriptSandbox("ops".into()).to_string(),
            "script sandbox: ops"
        );
        assert_eq!(
            WatchOverflow { generation: 9 }.to_string(),
            "watch overflow at generation 9"
        );
        assert_eq!(
            InitializeFailed("abort".into()).to_string(),
            "initialize failed: abort"
        );
        assert!(EngineError::Spawn("boom".into()).to_string().contains("boom"));
        assert!(EngineError::NotDiscovered("ty".into())
            .to_string()
            .contains("ty"));
        assert!(EngineError::Hash {
            expected: "aa".into(),
            actual: "bb".into(),
        }
        .to_string()
        .contains("aa"));
        assert!(EngineError::Crashed("child".into()).to_string().contains("child"));
        assert!(EngineError::Backoff { next_unix_ms: 9 }
            .to_string()
            .contains("9"));
        assert!(EngineError::Aborted("skip".into()).to_string().contains("skip"));
        assert_ne!(
            EngineError::Spawn("x".into()),
            EngineError::NotDiscovered("x".into())
        );
        assert_ne!(
            EngineError::Crashed("x".into()),
            EngineError::Aborted("x".into())
        );
        assert_eq!(ConfigError::HomeUnset.to_string(), "home directory is unset");
        assert!(ConfigError::Toml("bad".into()).to_string().contains("bad"));
        assert!(ConfigError::Io("e".into()).to_string().contains("e"));
        assert!(ConfigError::Prefix("p".into()).to_string().contains("p"));
    }
}
