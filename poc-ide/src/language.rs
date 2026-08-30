//! `LanguageCatalog` Registry and `ServeMode` Strategy.

use std::collections::BTreeMap;
use std::path::Path;

/// Extension → `languageId`. Unknown → `plaintext`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageCatalog {
    overrides: BTreeMap<String, String>,
}

impl LanguageCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test override. `ext` is stored without a leading dot, lowercased.
    pub fn override_extension(
        &mut self,
        ext: impl AsRef<str>,
        language_id: impl Into<String>,
    ) -> &mut Self {
        let ext = normalize_ext(ext.as_ref());
        self.overrides.insert(ext, language_id.into());
        self
    }

    pub fn for_path(&self, path: impl AsRef<Path>) -> &str {
        let ext = path
            .as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .map(normalize_ext)
            .unwrap_or_default();
        if let Some(id) = self.overrides.get(&ext) {
            return id.as_str();
        }
        stock_language_id(&ext).unwrap_or("plaintext")
    }

    pub fn skips_did_open(&self, path: impl AsRef<Path>) -> bool {
        self.for_path(path) == "plaintext"
    }

    pub fn override_len(&self) -> usize {
        self.overrides.len()
    }
}

/// Stock stdio vs control-socket. IDE-4 uses [`ServeMode::StockStdio`];
/// [`ServeMode::ControlSocket`] is present for IDE-5 and is not activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServeMode {
    StockStdio,
    ControlSocket,
}

impl ServeMode {
    pub fn is_stock_stdio(self) -> bool {
        matches!(self, Self::StockStdio)
    }

    pub fn is_control_socket(self) -> bool {
        matches!(self, Self::ControlSocket)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StockStdio => "stock-stdio",
            Self::ControlSocket => "control-socket",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "stock-stdio" => Some(Self::StockStdio),
            "control-socket" => Some(Self::ControlSocket),
            _ => None,
        }
    }
}

impl Default for ServeMode {
    fn default() -> Self {
        Self::StockStdio
    }
}

fn normalize_ext(ext: &str) -> String {
    ext.trim_start_matches('.').to_ascii_lowercase()
}

fn stock_language_id(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "java" => Some("java"),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "php" => Some("php"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "go" => Some("go"),
        "zig" => Some("zig"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some("cpp"),
        "cs" => Some("csharp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_catalog_registry_maps_v1_extensions() {
        let catalog = LanguageCatalog::new();
        let cases = [
            ("/ws/lib.rs", "rust"),
            ("/ws/LIB.RS", "rust"),
            ("/ws/app.py", "python"),
            ("/ws/Main.java", "java"),
            ("/ws/a.js", "javascript"),
            ("/ws/a.mjs", "javascript"),
            ("/ws/a.cjs", "javascript"),
            ("/ws/a.jsx", "javascript"),
            ("/ws/a.ts", "typescript"),
            ("/ws/a.tsx", "typescript"),
            ("/ws/index.php", "php"),
            ("/ws/page.html", "html"),
            ("/ws/page.htm", "html"),
            ("/ws/site.css", "css"),
            ("/ws/main.go", "go"),
            ("/ws/main.zig", "zig"),
            ("/ws/foo.c", "c"),
            ("/ws/foo.h", "c"),
            ("/ws/foo.cpp", "cpp"),
            ("/ws/foo.cc", "cpp"),
            ("/ws/foo.cxx", "cpp"),
            ("/ws/foo.hpp", "cpp"),
            ("/ws/foo.hxx", "cpp"),
            ("/ws/foo.hh", "cpp"),
            ("/ws/Program.cs", "csharp"),
        ];
        for (path, want) in cases {
            assert_eq!(catalog.for_path(path), want, "{path}");
            assert!(!catalog.skips_did_open(path), "{path} must didOpen");
        }
        assert_eq!(LanguageCatalog::default(), LanguageCatalog::new());
        assert_eq!(catalog.override_len(), 0);
    }

    #[test]
    fn language_catalog_registry_unknown_is_plaintext() {
        let catalog = LanguageCatalog::new();
        assert_eq!(catalog.for_path("/ws/notes.txt"), "plaintext");
        assert_eq!(catalog.for_path("/ws/README"), "plaintext");
        assert_eq!(catalog.for_path("/ws/.gitignore"), "plaintext");
        assert_eq!(catalog.for_path("/ws/unknown.unknown"), "plaintext");
        assert_eq!(catalog.for_path("/ws/dir"), "plaintext");
        assert!(catalog.skips_did_open("/ws/notes.txt"));
        assert!(catalog.skips_did_open("/ws/README"));
        assert!(stock_language_id("").is_none());
        assert!(stock_language_id("md").is_none());
        assert_eq!(normalize_ext(".RS"), "rs");
        assert_eq!(normalize_ext("Rs"), "rs");
    }

    #[test]
    fn language_catalog_registry_plaintext_skips_did_open() {
        let mut catalog = LanguageCatalog::new();
        assert!(catalog.skips_did_open("/ws/a.txt"));
        assert!(!catalog.skips_did_open("/ws/a.rs"));
        catalog.override_extension("rs", "plaintext");
        assert_eq!(catalog.for_path("/ws/a.rs"), "plaintext");
        assert!(catalog.skips_did_open("/ws/a.rs"));
        catalog.override_extension(".txt", "rust");
        assert_eq!(catalog.for_path("/ws/a.txt"), "rust");
        assert!(!catalog.skips_did_open("/ws/a.txt"));
        assert_eq!(catalog.override_len(), 2);
        catalog.override_extension("txt", "python");
        assert_eq!(catalog.for_path("/ws/a.txt"), "python");
        assert_eq!(catalog.override_len(), 2);
    }

    #[test]
    fn serve_mode_strategy_stock_stdio_vs_control_socket() {
        assert!(ServeMode::StockStdio.is_stock_stdio());
        assert!(!ServeMode::StockStdio.is_control_socket());
        assert_eq!(ServeMode::StockStdio.as_str(), "stock-stdio");
        assert!(ServeMode::ControlSocket.is_control_socket());
        assert!(!ServeMode::ControlSocket.is_stock_stdio());
        assert_eq!(ServeMode::ControlSocket.as_str(), "control-socket");
        assert_eq!(ServeMode::default(), ServeMode::StockStdio);
        assert_eq!(ServeMode::parse("stock-stdio"), Some(ServeMode::StockStdio));
        assert_eq!(
            ServeMode::parse("control-socket"),
            Some(ServeMode::ControlSocket)
        );
        assert_eq!(ServeMode::parse("mux"), None);
        assert_eq!(ServeMode::parse(""), None);
        assert_eq!(ServeMode::parse("StockStdio"), None);
        assert_ne!(ServeMode::StockStdio, ServeMode::ControlSocket);
    }
}
