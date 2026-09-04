//! `LanguageCatalog` Registry and `ServeMode` Strategy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::discover::DiscoverKind;
use crate::error::IdeError;

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

    pub fn is_known(&self, language_id: &str) -> bool {
        language_id != "plaintext" && stock_language_ids().contains(&language_id)
    }

    /// Method × min tier × ceiling from the language matrix. Java has no T3 offers.
    pub fn discover_offers(&self, language_id: &str) -> &'static [DiscoverOffer] {
        offers_for(language_id)
    }

    pub fn offer(&self, language_id: &str, kind: DiscoverKind) -> Option<DiscoverOffer> {
        self.discover_offers(language_id)
            .iter()
            .copied()
            .find(|o| o.kind == kind)
    }

    /// Languages with a T2 row (not Rust/CSS/HTML `—`).
    pub fn has_t2(&self, language_id: &str) -> bool {
        !matches!(language_id, "rust" | "css" | "html" | "plaintext") && self.is_known(language_id)
    }

    /// Java none; C# T1/T2 ceiling; others have a types engine in the matrix.
    pub fn t3_supported(&self, language_id: &str) -> bool {
        self.is_known(language_id) && !matches!(language_id, "java" | "csharp")
    }

    pub fn ceiling(&self, language_id: &str) -> Option<WireTier> {
        if !self.is_known(language_id) {
            return None;
        }
        if matches!(language_id, "java" | "csharp") {
            Some(WireTier::Graph)
        } else {
            Some(WireTier::Types)
        }
    }
}

/// Wire `syntax` / `graph` / `types`. Value object; comparable for menu gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WireTier {
    Syntax = 1,
    Graph = 2,
    Types = 3,
}

impl WireTier {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(Self::Syntax),
            "graph" => Some(Self::Graph),
            "types" => Some(Self::Types),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Graph => "graph",
            Self::Types => "types",
        }
    }

    pub fn meets(self, min: Self) -> bool {
        self >= min
    }
}

/// Method × min tier × language ceiling. Value object on the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoverOffer {
    kind: DiscoverKind,
    min_tier: WireTier,
    ceiling: WireTier,
}

impl DiscoverOffer {
    pub const fn new(kind: DiscoverKind, min_tier: WireTier, ceiling: WireTier) -> Self {
        Self {
            kind,
            min_tier,
            ceiling,
        }
    }

    pub fn kind(self) -> DiscoverKind {
        self.kind
    }

    pub fn min_tier(self) -> WireTier {
        self.min_tier
    }

    pub fn ceiling(self) -> WireTier {
        self.ceiling
    }

    pub fn is_t3_only(self) -> bool {
        self.min_tier == WireTier::Types
    }
}

/// Stock stdio vs control-socket vs mux. Default is [`ServeMode::ControlSocket`]
/// with an owned [`ControlSocketPath`]. [`ServeMode::StockStdio`] stays an
/// explicit variant. [`ServeMode::Mux`] is the container Strategy (`serve --mux`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServeMode {
    StockStdio,
    ControlSocket,
    Mux,
}

/// Value object. CLI path, else `$PREFIX/run/poc-ide.sock`, else temp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlSocketPath {
    path: PathBuf,
}

impl ControlSocketPath {
    pub const FILE_NAME: &'static str = "poc-ide.sock";

    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Tests inject prefix / home / temp so they never require `$HOME`.
    pub fn resolve_in(
        cli: Option<&Path>,
        prefix_home: Option<&str>,
        home: Option<&str>,
        temp_dir: &Path,
    ) -> Self {
        if let Some(path) = cli {
            if !path.as_os_str().is_empty() {
                return Self::from_path(path);
            }
        }
        if let Some(prefix) = prefix_home.filter(|s| !s.is_empty()) {
            return Self::from_path(Path::new(prefix).join("run").join(Self::FILE_NAME));
        }
        if let Some(home) = home.filter(|s| !s.is_empty()) {
            return Self::from_path(
                Path::new(home)
                    .join(".progressivelsp")
                    .join("run")
                    .join(Self::FILE_NAME),
            );
        }
        Self::from_path(temp_dir.join(Self::FILE_NAME))
    }

    pub fn resolve_default(cli: Option<&Path>) -> Self {
        Self::resolve_in(
            cli,
            std::env::var("PROGRESSIVE_LSP_HOME").ok().as_deref(),
            std::env::var("HOME").ok().as_deref(),
            &std::env::temp_dir(),
        )
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(self) -> PathBuf {
        self.path
    }

    pub fn ensure_parent(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

impl ServeMode {
    pub fn is_stock_stdio(self) -> bool {
        matches!(self, Self::StockStdio)
    }

    pub fn is_control_socket(self) -> bool {
        matches!(self, Self::ControlSocket)
    }

    pub fn is_mux(self) -> bool {
        matches!(self, Self::Mux)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StockStdio => "stock-stdio",
            Self::ControlSocket => "control-socket",
            Self::Mux => "mux",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "stock-stdio" => Some(Self::StockStdio),
            "control-socket" => Some(Self::ControlSocket),
            "mux" => Some(Self::Mux),
            _ => None,
        }
    }

    /// `progressive-lsp` argv. [`ServeMode::Mux`] is `serve --mux`.
    pub fn serve_args(self, control_socket: Option<&Path>) -> Result<Vec<String>, IdeError> {
        match self {
            Self::StockStdio => Ok(vec!["serve".into()]),
            Self::Mux => Ok(vec!["serve".into(), "--mux".into()]),
            Self::ControlSocket => {
                let path = control_socket.ok_or_else(IdeError::control_socket_missing)?;
                Ok(vec![
                    "serve".into(),
                    "--control-socket".into(),
                    path.to_string_lossy().into_owned(),
                ])
            }
        }
    }
}

impl Default for ServeMode {
    fn default() -> Self {
        Self::ControlSocket
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

fn stock_language_ids() -> &'static [&'static str] {
    &[
        "rust",
        "python",
        "java",
        "javascript",
        "typescript",
        "php",
        "html",
        "css",
        "go",
        "zig",
        "c",
        "cpp",
        "csharp",
    ]
}

const T1_T2_CEILING: &[DiscoverOffer] = &[
    DiscoverOffer::new(DiscoverKind::Definition, WireTier::Syntax, WireTier::Graph),
    DiscoverOffer::new(
        DiscoverKind::Implementation,
        WireTier::Graph,
        WireTier::Graph,
    ),
    DiscoverOffer::new(DiscoverKind::References, WireTier::Syntax, WireTier::Graph),
];

const T2_THEN_T3: &[DiscoverOffer] = &[
    DiscoverOffer::new(DiscoverKind::Definition, WireTier::Syntax, WireTier::Types),
    DiscoverOffer::new(
        DiscoverKind::Implementation,
        WireTier::Graph,
        WireTier::Types,
    ),
    DiscoverOffer::new(DiscoverKind::References, WireTier::Syntax, WireTier::Types),
];

const T1_THEN_T3: &[DiscoverOffer] = &[
    DiscoverOffer::new(DiscoverKind::Definition, WireTier::Syntax, WireTier::Types),
    DiscoverOffer::new(
        DiscoverKind::Implementation,
        WireTier::Types,
        WireTier::Types,
    ),
    DiscoverOffer::new(DiscoverKind::References, WireTier::Syntax, WireTier::Types),
];

fn offers_for(language_id: &str) -> &'static [DiscoverOffer] {
    match language_id {
        "java" | "csharp" => T1_T2_CEILING,
        "javascript" | "typescript" | "php" | "go" | "zig" => T2_THEN_T3,
        "rust" | "css" | "html" | "python" | "c" | "cpp" => T1_THEN_T3,
        _ => &[],
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
    fn serve_mode_strategy_stock_stdio_vs_control_socket_vs_mux() {
        assert!(ServeMode::StockStdio.is_stock_stdio());
        assert!(!ServeMode::StockStdio.is_control_socket());
        assert_eq!(ServeMode::StockStdio.as_str(), "stock-stdio");
        assert!(ServeMode::ControlSocket.is_control_socket());
        assert!(!ServeMode::ControlSocket.is_stock_stdio());
        assert_eq!(ServeMode::ControlSocket.as_str(), "control-socket");
        assert_eq!(ServeMode::default(), ServeMode::ControlSocket);
        assert_eq!(ServeMode::parse("stock-stdio"), Some(ServeMode::StockStdio));
        assert_eq!(
            ServeMode::parse("control-socket"),
            Some(ServeMode::ControlSocket)
        );
        assert_eq!(ServeMode::parse("mux"), Some(ServeMode::Mux));
        assert!(ServeMode::Mux.is_mux());
        assert!(!ServeMode::StockStdio.is_mux());
        assert!(!ServeMode::ControlSocket.is_mux());
        assert!(!ServeMode::Mux.is_stock_stdio());
        assert!(!ServeMode::Mux.is_control_socket());
        assert_eq!(ServeMode::Mux.as_str(), "mux");
        assert_ne!(ServeMode::Mux.as_str(), "");
        assert_ne!(ServeMode::StockStdio.as_str(), "");
        assert_eq!(ServeMode::parse(""), None);
        assert_eq!(ServeMode::parse("StockStdio"), None);
        assert_ne!(ServeMode::StockStdio, ServeMode::ControlSocket);
        assert_ne!(ServeMode::Mux, ServeMode::ControlSocket);

        let mux = ServeMode::Mux.serve_args(None).unwrap();
        assert_eq!(mux, vec!["serve", "--mux"]);
        let mux_ignores_socket = ServeMode::Mux
            .serve_args(Some(Path::new("/tmp/x.sock")))
            .unwrap();
        assert_eq!(mux_ignores_socket, vec!["serve", "--mux"]);
        assert_eq!(mux.iter().filter(|a| *a == "--mux").count(), 1);

        let stock = ServeMode::StockStdio.serve_args(None).unwrap();
        assert_eq!(stock, vec!["serve"]);
        assert!(!stock
            .iter()
            .any(|a| a.contains("mux") || a.contains("control")));
        let stock_ignores_socket = ServeMode::StockStdio
            .serve_args(Some(Path::new("/tmp/x.sock")))
            .unwrap();
        assert_eq!(stock_ignores_socket, vec!["serve"]);

        let missing = ServeMode::ControlSocket.serve_args(None).unwrap_err();
        assert!(missing.is_control_socket_missing());
        let ctrl = ServeMode::ControlSocket
            .serve_args(Some(Path::new("/tmp/plsp.sock")))
            .unwrap();
        assert_eq!(ctrl, vec!["serve", "--control-socket", "/tmp/plsp.sock"]);
        assert!(!ctrl.iter().any(|a| a.contains("mux")));
        assert_eq!(ctrl.iter().filter(|a| *a == "--mux").count(), 0);
    }

    #[test]
    fn control_socket_path_value_object_resolve_order() {
        // Value object: CLI → prefix → home → temp. ServeMode does not own the path.
        assert_eq!(ServeMode::default(), ServeMode::ControlSocket);
        assert!(ServeMode::default().is_control_socket());
        let owned =
            ControlSocketPath::resolve_in(None, Some("/pfx"), Some("/home/me"), Path::new("/tmp"));
        assert_eq!(owned.as_path(), Path::new("/pfx/run/poc-ide.sock"));
        let args = ServeMode::default()
            .serve_args(Some(owned.as_path()))
            .unwrap();
        assert_eq!(
            args,
            vec!["serve", "--control-socket", "/pfx/run/poc-ide.sock"]
        );
        assert!(ServeMode::default()
            .serve_args(None)
            .unwrap_err()
            .is_control_socket_missing());

        let cli = ControlSocketPath::resolve_in(
            Some(Path::new("/cli.sock")),
            Some("/pfx"),
            Some("/home/me"),
            Path::new("/tmp"),
        );
        assert_eq!(cli.as_path(), Path::new("/cli.sock"));
        let via_home =
            ControlSocketPath::resolve_in(None, None, Some("/home/me"), Path::new("/tmp"));
        assert_eq!(
            via_home.as_path(),
            Path::new("/home/me/.progressivelsp/run/poc-ide.sock")
        );
        let via_temp = ControlSocketPath::resolve_in(None, None, None, Path::new("/tmp"));
        assert_eq!(via_temp.as_path(), Path::new("/tmp/poc-ide.sock"));
        let empty_cli = ControlSocketPath::resolve_in(
            Some(Path::new("")),
            Some("/pfx"),
            None,
            Path::new("/tmp"),
        );
        assert_eq!(empty_cli.as_path(), Path::new("/pfx/run/poc-ide.sock"));
        let copied = ControlSocketPath::from_path("/x.sock");
        assert_eq!(copied.into_path(), PathBuf::from("/x.sock"));
        assert_eq!(ControlSocketPath::FILE_NAME, "poc-ide.sock");
        let tmp = tempfile::tempdir().unwrap();
        let nested = ControlSocketPath::from_path(tmp.path().join("run").join("poc-ide.sock"));
        nested.ensure_parent().unwrap();
        assert!(tmp.path().join("run").is_dir());
    }

    #[test]
    fn discover_offer_value_object_java_has_no_t3_csharp_ceiling() {
        let catalog = LanguageCatalog::new();
        assert!(catalog.is_known("java"));
        assert!(catalog.is_known("csharp"));
        assert!(!catalog.is_known("plaintext"));
        assert!(!catalog.is_known("unknown"));
        assert!(catalog.has_t2("java"));
        assert!(catalog.has_t2("csharp"));
        assert!(!catalog.has_t2("rust"));
        assert!(!catalog.has_t2("css"));
        assert!(!catalog.has_t2("html"));
        assert!(!catalog.has_t2("plaintext"));
        assert!(catalog.has_t2("javascript"));
        assert!(!catalog.t3_supported("java"));
        assert!(!catalog.t3_supported("csharp"));
        assert!(catalog.t3_supported("rust"));
        assert!(catalog.t3_supported("python"));
        assert!(!catalog.t3_supported("plaintext"));
        assert_eq!(catalog.ceiling("java"), Some(WireTier::Graph));
        assert_eq!(catalog.ceiling("csharp"), Some(WireTier::Graph));
        assert_eq!(catalog.ceiling("rust"), Some(WireTier::Types));
        assert_eq!(catalog.ceiling("plaintext"), None);

        let java = catalog.discover_offers("java");
        assert_eq!(java.len(), 3);
        assert!(java.iter().all(|o| o.ceiling() == WireTier::Graph));
        assert!(java.iter().all(|o| !o.is_t3_only()));
        assert_eq!(
            catalog
                .offer("java", DiscoverKind::Implementation)
                .unwrap()
                .min_tier(),
            WireTier::Graph
        );
        assert_eq!(
            catalog
                .offer("java", DiscoverKind::Definition)
                .unwrap()
                .min_tier(),
            WireTier::Syntax
        );
        let rust_impl = catalog.offer("rust", DiscoverKind::Implementation).unwrap();
        assert_eq!(rust_impl.min_tier(), WireTier::Types);
        assert!(rust_impl.is_t3_only());
        assert!(catalog
            .offer("plaintext", DiscoverKind::Definition)
            .is_none());
        assert!(catalog.discover_offers("plaintext").is_empty());
        assert_eq!(
            catalog
                .offer("javascript", DiscoverKind::Implementation)
                .unwrap()
                .min_tier(),
            WireTier::Graph
        );
        assert_eq!(WireTier::parse("syntax"), Some(WireTier::Syntax));
        assert_eq!(WireTier::parse("graph"), Some(WireTier::Graph));
        assert_eq!(WireTier::parse("types"), Some(WireTier::Types));
        assert_eq!(WireTier::parse("nope"), None);
        assert_eq!(WireTier::Syntax.as_str(), "syntax");
        assert_eq!(WireTier::Graph.as_str(), "graph");
        assert_eq!(WireTier::Types.as_str(), "types");
        assert!(WireTier::Graph.meets(WireTier::Syntax));
        assert!(!WireTier::Syntax.meets(WireTier::Graph));
        assert!(WireTier::Types.meets(WireTier::Types));
        assert_ne!(WireTier::Syntax, WireTier::Graph);
        let offer = DiscoverOffer::new(DiscoverKind::Definition, WireTier::Syntax, WireTier::Types);
        assert_eq!(offer.kind(), DiscoverKind::Definition);
        assert_eq!(offer.min_tier(), WireTier::Syntax);
        assert_eq!(offer.ceiling(), WireTier::Types);
        assert!(!offer.is_t3_only());
    }
}
