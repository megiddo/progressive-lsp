//! Domain Result for the POC IDE. User paths never `unwrap`.

use std::io;
use std::path::PathBuf;

/// Typed failure from Ports and value-object constructors.
#[derive(Debug, thiserror::Error)]
pub enum IdeError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("path not found: {0}")]
    NotFound(PathBuf),
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("path has no parent: {0}")]
    NoParent(PathBuf),
    #[error("is a directory: {0}")]
    IsDirectory(PathBuf),
    #[error("invalid UTF-8: {0}")]
    InvalidUtf8(PathBuf),
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error("watch: {0}")]
    Watch(String),
    #[error("lsp: {0}")]
    Lsp(String),
    #[error("lsp method missing: {0}")]
    LspMethodMissing(String),
    #[error("progressive-lsp binary not found")]
    MissingBinary,
    #[error("control: {0}")]
    Control(String),
    #[error("log: {0}")]
    Log(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl IdeError {
    pub fn is_not_absolute(&self) -> bool {
        matches!(self, Self::NotAbsolute(_))
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    pub fn is_not_a_directory(&self) -> bool {
        matches!(self, Self::NotADirectory(_))
    }

    pub fn is_no_parent(&self) -> bool {
        matches!(self, Self::NoParent(_))
    }

    pub fn is_directory(&self) -> bool {
        matches!(self, Self::IsDirectory(_))
    }

    pub fn is_invalid_utf8(&self) -> bool {
        matches!(self, Self::InvalidUtf8(_))
    }

    pub fn is_clipboard(&self) -> bool {
        matches!(self, Self::Clipboard(_))
    }

    pub fn is_watch(&self) -> bool {
        matches!(self, Self::Watch(_))
    }

    pub fn is_lsp(&self) -> bool {
        matches!(self, Self::Lsp(_))
    }

    pub fn is_lsp_method_missing(&self) -> bool {
        matches!(self, Self::LspMethodMissing(_))
    }

    pub fn is_missing_binary(&self) -> bool {
        matches!(self, Self::MissingBinary)
    }

    pub fn is_control(&self) -> bool {
        matches!(self, Self::Control(_))
    }

    pub fn is_log(&self) -> bool {
        matches!(self, Self::Log(_))
    }

    pub fn is_control_socket_missing(&self) -> bool {
        matches!(self, Self::Control(m) if m == "control socket missing")
    }

    pub fn is_pending_mux(&self) -> bool {
        matches!(self, Self::Control(m) if m == "pending_mux")
    }

    pub fn is_io(&self) -> bool {
        matches!(self, Self::Io(_))
    }

    pub fn clipboard(msg: impl Into<String>) -> Self {
        Self::Clipboard(msg.into())
    }

    pub fn watch(msg: impl Into<String>) -> Self {
        Self::Watch(msg.into())
    }

    pub fn lsp(msg: impl Into<String>) -> Self {
        Self::Lsp(msg.into())
    }

    pub fn lsp_method_missing(method: impl Into<String>) -> Self {
        Self::LspMethodMissing(method.into())
    }

    pub fn control(msg: impl Into<String>) -> Self {
        Self::Control(msg.into())
    }

    pub fn control_socket_missing() -> Self {
        Self::Control("control socket missing".into())
    }

    pub fn pending_mux() -> Self {
        Self::Control("pending_mux".into())
    }

    pub fn log(msg: impl Into<String>) -> Self {
        Self::Log(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ide_error_domain_result_display_names_each_variant() {
        assert_eq!(
            IdeError::NotAbsolute(PathBuf::from("rel")).to_string(),
            "path is not absolute: rel"
        );
        assert_eq!(
            IdeError::NotFound(PathBuf::from("/missing")).to_string(),
            "path not found: /missing"
        );
        assert_eq!(
            IdeError::NotADirectory(PathBuf::from("/tmp/file")).to_string(),
            "not a directory: /tmp/file"
        );
        assert_eq!(
            IdeError::NoParent(PathBuf::from("/")).to_string(),
            "path has no parent: /"
        );
        assert_eq!(
            IdeError::IsDirectory(PathBuf::from("/ws")).to_string(),
            "is a directory: /ws"
        );
        assert_eq!(
            IdeError::InvalidUtf8(PathBuf::from("/ws/a.rs")).to_string(),
            "invalid UTF-8: /ws/a.rs"
        );
        assert_eq!(
            IdeError::clipboard("denied").to_string(),
            "clipboard: denied"
        );
        assert_eq!(IdeError::watch("overflow").to_string(), "watch: overflow");
        assert_eq!(IdeError::lsp("eof").to_string(), "lsp: eof");
        assert_eq!(
            IdeError::lsp_method_missing("textDocument/definition").to_string(),
            "lsp method missing: textDocument/definition"
        );
        assert_eq!(
            IdeError::MissingBinary.to_string(),
            "progressive-lsp binary not found"
        );
        assert_eq!(IdeError::control("refused").to_string(), "control: refused");
        assert_eq!(
            IdeError::control_socket_missing().to_string(),
            "control: control socket missing"
        );
        assert_eq!(IdeError::pending_mux().to_string(), "control: pending_mux");
        assert_eq!(
            IdeError::log("sink unavailable").to_string(),
            "log: sink unavailable"
        );
        let io = IdeError::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert!(io.to_string().contains("denied"));
    }

    #[test]
    fn ide_error_domain_result_classifiers() {
        assert!(IdeError::NotAbsolute(PathBuf::from("x")).is_not_absolute());
        assert!(!IdeError::NotAbsolute(PathBuf::from("x")).is_not_found());
        assert!(IdeError::NotFound(PathBuf::from("/n")).is_not_found());
        assert!(!IdeError::NotFound(PathBuf::from("/n")).is_not_a_directory());
        assert!(IdeError::NotADirectory(PathBuf::from("/f")).is_not_a_directory());
        assert!(!IdeError::NotADirectory(PathBuf::from("/f")).is_no_parent());
        assert!(IdeError::NoParent(PathBuf::from("/")).is_no_parent());
        assert!(!IdeError::NoParent(PathBuf::from("/")).is_directory());
        assert!(IdeError::IsDirectory(PathBuf::from("/ws")).is_directory());
        assert!(!IdeError::IsDirectory(PathBuf::from("/ws")).is_invalid_utf8());
        assert!(IdeError::InvalidUtf8(PathBuf::from("/a")).is_invalid_utf8());
        assert!(!IdeError::InvalidUtf8(PathBuf::from("/a")).is_clipboard());
        assert!(IdeError::clipboard("x").is_clipboard());
        assert!(!IdeError::clipboard("x").is_watch());
        assert!(IdeError::watch("x").is_watch());
        assert!(!IdeError::watch("x").is_lsp());
        assert!(IdeError::lsp("x").is_lsp());
        assert!(!IdeError::lsp("x").is_lsp_method_missing());
        assert!(IdeError::lsp_method_missing("m").is_lsp_method_missing());
        assert!(!IdeError::lsp_method_missing("m").is_missing_binary());
        assert!(IdeError::MissingBinary.is_missing_binary());
        assert!(!IdeError::MissingBinary.is_control());
        assert!(IdeError::control("x").is_control());
        assert!(!IdeError::control("x").is_control_socket_missing());
        assert!(!IdeError::control("x").is_pending_mux());
        assert!(IdeError::control_socket_missing().is_control_socket_missing());
        assert!(IdeError::control_socket_missing().is_control());
        assert!(!IdeError::control_socket_missing().is_pending_mux());
        assert!(IdeError::pending_mux().is_pending_mux());
        assert!(IdeError::pending_mux().is_control());
        assert!(!IdeError::pending_mux().is_control_socket_missing());
        assert!(!IdeError::control("x").is_log());
        assert!(!IdeError::control("x").is_io());
        assert!(IdeError::log("x").is_log());
        assert!(!IdeError::log("x").is_io());
        assert!(!IdeError::log("x").is_control());
        let io = IdeError::from(io::Error::other("x"));
        assert!(io.is_io());
        assert!(!io.is_not_absolute());
        assert!(!io.is_control());
        assert!(!io.is_log());
    }
}
