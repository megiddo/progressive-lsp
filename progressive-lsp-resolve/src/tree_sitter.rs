//! T1 resolver over an extracted symbol index (Visitor results, not JSON-RPC).

use std::sync::Arc;

use progressive_lsp_core::{FileId, Tier};

use crate::query::{
    DocumentSymbol, Hover, LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery,
    ResolveResult, SymbolKind,
};
use crate::Resolver;

/// One declaration extracted from a Tree-sitter CST walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedSymbol {
    pub file: FileId,
    pub uri: String,
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub arity: Option<u32>,
    pub fqn: String,
    pub container: Option<String>,
}

impl IndexedSymbol {
    pub fn matches_name(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        self.name.contains(needle) || self.fqn.contains(needle)
    }

    pub fn to_location(&self, tier: Tier) -> LspLocation {
        LspLocation::new(self.uri.clone(), self.selection_range, tier)
    }

    pub fn to_document_symbol(&self) -> DocumentSymbol {
        DocumentSymbol {
            name: self.name.clone(),
            kind: self.kind,
            range: self.range,
            selection_range: self.selection_range,
            arity: self.arity,
            children: Vec::new(),
        }
    }
}

/// Read-only symbol store. Index crate implements this.
pub trait SymbolIndex: Send + Sync {
    fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol>;
    fn all_symbols(&self) -> Vec<IndexedSymbol>;

    /// In-memory source when the index holds file text (serve session).
    fn file_text(&self, _file: &FileId) -> Option<&str> {
        None
    }

    fn indexed_files(&self) -> Vec<FileId> {
        let mut files: Vec<FileId> = self
            .all_symbols()
            .into_iter()
            .map(|s| s.file)
            .collect();
        files.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        files.dedup();
        files
    }
}

/// T1 handler. First capable after T3/T2 in the chain.
pub struct TreeSitterResolver {
    index: Arc<dyn SymbolIndex>,
}

impl TreeSitterResolver {
    pub fn new(index: Arc<dyn SymbolIndex>) -> Self {
        Self { index }
    }

    fn identifier_at(&self, q: &ResolveQuery) -> Option<IndexedSymbol> {
        let symbols = self.index.symbols_in(&q.file);
        symbols
            .into_iter()
            .filter(|s| q.position.is_within(s.selection_range))
            .max_by_key(|s| {
                let rank = match s.kind {
                    SymbolKind::Method
                    | SymbolKind::Constructor
                    | SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Enum => 2u8,
                    _ => 1u8,
                };
                (
                    rank,
                    s.selection_range.start.line,
                    s.selection_range.start.character,
                )
            })
    }

    fn lookup_name(&self, name: &str) -> Vec<IndexedSymbol> {
        self.index
            .all_symbols()
            .into_iter()
            .filter(|s| s.name == name || s.fqn.ends_with(&format!(".{name}")) || s.fqn == name)
            .collect()
    }

    fn reference_name(&self, q: &ResolveQuery) -> Option<String> {
        if let Some(at) = self.identifier_at(q) {
            return Some(at.name);
        }
        self.index
            .file_text(&q.file)
            .and_then(|src| identifier_at_source(src, q.position))
    }

    fn reference_locations(&self, _q: &ResolveQuery, name: &str, tier: Tier) -> Vec<LspLocation> {
        let mut locs: Vec<LspLocation> = Vec::new();
        for file in self.index.indexed_files() {
            let Some(src) = self.index.file_text(&file) else {
                continue;
            };
            let uri = self
                .index
                .symbols_in(&file)
                .first()
                .map(|s| s.uri.clone())
                .unwrap_or_else(|| format!("file:///{}", file.as_str()));
            for pos in identifier_occurrences_in_source(src, name) {
                locs.push(LspLocation::new(
                    uri.clone(),
                    Range::point(pos),
                    tier,
                ));
            }
        }
        if locs.is_empty() {
            locs = self
                .lookup_name(name)
                .into_iter()
                .map(|s| s.to_location(tier))
                .collect();
        }
        locs
    }

    fn type_locations(&self, at: &IndexedSymbol, tier: Tier) -> Vec<LspLocation> {
        let mut locs: Vec<LspLocation> = self
            .lookup_name(&at.name)
            .into_iter()
            .filter(|s| s.kind.is_type())
            .map(|s| s.to_location(tier))
            .collect();
        if locs.is_empty() && at.kind.is_type() {
            locs.push(at.to_location(tier));
        }
        locs
    }

    fn definition_locations(
        &self,
        q: &ResolveQuery,
        at: &IndexedSymbol,
        tier: Tier,
        type_definition: bool,
    ) -> Vec<LspLocation> {
        let mut hits: Vec<IndexedSymbol> = self
            .lookup_name(&at.name)
            .into_iter()
            .filter(|s| {
                if type_definition {
                    s.kind.is_type()
                } else {
                    s.kind.is_declaration()
                }
            })
            .collect();
        if !type_definition && at.kind == SymbolKind::Variable {
            let types: Vec<IndexedSymbol> = hits
                .iter()
                .filter(|s| s.kind.is_type())
                .cloned()
                .collect();
            if !types.is_empty() {
                hits = types;
                hits.sort_by_key(|s| match s.kind {
                    SymbolKind::Interface => 0,
                    SymbolKind::Class => 1,
                    SymbolKind::Enum => 2,
                    _ => 3,
                });
            }
        }
        if hits.is_empty() {
            if at.kind.is_declaration() {
                hits.push(at.clone());
            } else {
                return vec![at.to_location(tier)];
            }
        }
        if !type_definition && hits.len() > 1 {
            let query_file = &q.file;
            hits.sort_by_key(|s| {
                let same_file = &s.file == query_file;
                let type_rank = match s.kind {
                    SymbolKind::Interface => 0,
                    SymbolKind::Class => 1,
                    SymbolKind::Enum => 2,
                    _ => 3,
                };
                (
                    !same_file,
                    type_rank,
                    s.selection_range.start.line,
                    s.selection_range.start.character,
                )
            });
            hits.truncate(1);
        }
        hits.into_iter().map(|s| s.to_location(tier)).collect()
    }
}

impl Resolver for TreeSitterResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        let tier = Tier::Syntax;
        match q.kind {
            QueryKind::DocumentSymbol => {
                let symbols = self
                    .index
                    .symbols_in(&q.file)
                    .into_iter()
                    .map(|s| s.to_document_symbol())
                    .collect();
                ResolveOutcome::Ready(ResolveResult {
                    locations: Vec::new(),
                    tier,
                    hover: None,
                    symbols,
                })
            }
            QueryKind::WorkspaceSymbol => {
                let needle = q.symbol_query.as_deref().unwrap_or("");
                let locations = self
                    .index
                    .all_symbols()
                    .into_iter()
                    .filter(|s| s.matches_name(needle))
                    .map(|s| s.to_location(tier))
                    .collect();
                ResolveOutcome::Ready(ResolveResult::locations(tier, locations))
            }
            QueryKind::Hover => {
                let Some(sym) = self.identifier_at(q) else {
                    return ResolveOutcome::Ready(ResolveResult::empty(tier));
                };
                ResolveOutcome::Ready(ResolveResult {
                    locations: vec![sym.to_location(tier)],
                    tier,
                    hover: Some(Hover {
                        name: sym.name,
                        arity: sym.arity,
                        type_info: None,
                    }),
                    symbols: Vec::new(),
                })
            }
            QueryKind::Definition | QueryKind::TypeDefinition => {
                let Some(at) = self.identifier_at(q) else {
                    return ResolveOutcome::Ready(ResolveResult::empty(tier));
                };
                let locs = self.definition_locations(
                    q,
                    &at,
                    tier,
                    q.kind == QueryKind::TypeDefinition,
                );
                ResolveOutcome::Ready(ResolveResult::locations(tier, locs))
            }
            QueryKind::References => {
                let Some(name) = self.reference_name(q) else {
                    return ResolveOutcome::NotReady;
                };
                let locs = self.reference_locations(q, &name, tier);
                if locs.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(tier, locs))
            }
            QueryKind::Implementation => {
                let Some(at) = self.identifier_at(q) else {
                    return ResolveOutcome::NotReady;
                };
                let locs = self.type_locations(&at, tier);
                if locs.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(tier, locs))
            }
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Identifier under the caret when the index has no declaration span there.
pub fn identifier_at_source(source: &str, pos: Position) -> Option<String> {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut idx = 0usize;
    for (i, ch) in source.char_indices() {
        if line == pos.line && col == pos.character {
            idx = i;
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
            idx = i + ch.len_utf8();
        }
    }
    let bytes = source.as_bytes();
    if idx >= bytes.len() {
        return None;
    }
    let mut start = idx;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = idx;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    if start >= end {
        return None;
    }
    Some(source[start..end].to_string())
}

fn identifier_occurrences_in_source(source: &str, name: &str) -> Vec<Position> {
    if name.is_empty() {
        return Vec::new();
    }
    let needle = name.as_bytes();
    let mut out = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
                let after_i = i + needle.len();
                let after_ok = after_i >= bytes.len() || !is_ident_byte(bytes[after_i]);
                if before_ok && after_ok {
                    out.push(Position::new(line_no as u32, i as u32));
                }
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::Position;
    use std::sync::Mutex;

    struct MemIndex {
        symbols: Mutex<Vec<IndexedSymbol>>,
    }

    impl MemIndex {
        fn new(symbols: Vec<IndexedSymbol>) -> Arc<Self> {
            Arc::new(Self {
                symbols: Mutex::new(symbols),
            })
        }
    }

    impl SymbolIndex for MemIndex {
        fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol> {
            self.symbols
                .lock()
                .expect("index lock")
                .iter()
                .filter(|s| &s.file == file)
                .cloned()
                .collect()
        }

        fn all_symbols(&self) -> Vec<IndexedSymbol> {
            self.symbols.lock().expect("index lock").clone()
        }
    }

    fn sym(
        file: &str,
        name: &str,
        line: u32,
        kind: SymbolKind,
        fqn: &str,
        arity: Option<u32>,
    ) -> IndexedSymbol {
        let range = Range::new(Position::new(line, 0), Position::new(line, 20));
        IndexedSymbol {
            file: FileId::new(file),
            uri: format!("file:///{file}"),
            name: name.into(),
            kind,
            range,
            selection_range: Range::new(
                Position::new(line, 0),
                Position::new(line, name.len() as u32),
            ),
            arity,
            fqn: fqn.into(),
            container: None,
        }
    }

    #[test]
    fn indexed_symbol_name_match_and_empty_needle() {
        let s = sym(
            "A.java",
            "greet",
            1,
            SymbolKind::Method,
            "com.Lib.greet",
            Some(1),
        );
        assert!(s.matches_name(""));
        assert!(s.matches_name("greet"));
        assert!(s.matches_name("Lib"));
        assert!(!s.matches_name("zzz"));
        assert_eq!(s.to_location(Tier::Syntax).uri, "file:///A.java");
        assert_eq!(s.to_document_symbol().arity, Some(1));
    }

    #[test]
    fn definition_cross_file_by_name() {
        let idx = MemIndex::new(vec![
            sym(
                "App.java",
                "greet",
                4,
                SymbolKind::Method,
                "app.App.greet",
                Some(1),
            ),
            sym(
                "Lib.java",
                "greet",
                2,
                SymbolKind::Method,
                "com.example.lib.Lib.greet",
                Some(1),
            ),
        ]);
        let r = TreeSitterResolver::new(idx);
        let q = ResolveQuery::new(
            FileId::new("App.java"),
            Position::new(4, 1),
            QueryKind::Definition,
        );
        match r.resolve(&q) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.tier, Tier::Syntax);
                assert_eq!(res.locations.len(), 1);
                assert!(res.locations[0].uri.ends_with("App.java"));
            }
            ResolveOutcome::NotReady => panic!("T1 is always ready"),
        }
    }

    #[test]
    fn type_definition_filters_to_types() {
        let idx = MemIndex::new(vec![
            sym("A.java", "Lib", 0, SymbolKind::Class, "com.Lib", None),
            sym(
                "A.java",
                "Lib",
                3,
                SymbolKind::Method,
                "com.Lib.Lib",
                Some(0),
            ),
        ]);
        let r = TreeSitterResolver::new(idx);
        let q = ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(0, 0),
            QueryKind::TypeDefinition,
        );
        match r.resolve(&q) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.locations.len(), 1);
                assert_eq!(res.locations[0].range.start.line, 0);
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn hover_uses_name_and_arity() {
        let idx = MemIndex::new(vec![sym(
            "A.java",
            "greet",
            2,
            SymbolKind::Method,
            "Lib.greet",
            Some(2),
        )]);
        let r = TreeSitterResolver::new(idx);
        let q = ResolveQuery::new(FileId::new("A.java"), Position::new(2, 1), QueryKind::Hover);
        match r.resolve(&q) {
            ResolveOutcome::Ready(res) => {
                let h = res.hover.expect("hover");
                assert_eq!(h.signature(), "greet(2)");
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn document_and_workspace_symbol() {
        let idx = MemIndex::new(vec![
            sym("A.java", "Lib", 0, SymbolKind::Class, "com.Lib", None),
            sym("B.java", "App", 0, SymbolKind::Class, "com.App", None),
        ]);
        let r = TreeSitterResolver::new(idx);
        let doc = ResolveQuery::new(
            FileId::new("A.java"),
            Position::default(),
            QueryKind::DocumentSymbol,
        );
        match r.resolve(&doc) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.symbols.len(), 1);
                assert_eq!(res.symbols[0].name, "Lib");
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        let ws = ResolveQuery::workspace_symbol("App");
        match r.resolve(&ws) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.locations.len(), 1);
                assert!(res.locations[0].uri.ends_with("B.java"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        let all = ResolveQuery::workspace_symbol("");
        match r.resolve(&all) {
            ResolveOutcome::Ready(res) => assert_eq!(res.locations.len(), 2),
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn references_and_implementation_and_misses() {
        let idx = MemIndex::new(vec![
            sym("A.java", "x", 1, SymbolKind::Field, "A.x", None),
            sym("B.java", "x", 2, SymbolKind::Field, "B.x", None),
        ]);
        let r = TreeSitterResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(1, 0),
            QueryKind::References,
        )) {
            ResolveOutcome::Ready(res) => assert_eq!(res.locations.len(), 2),
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(1, 0),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::NotReady => {}
            other => panic!("{other:?}"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("missing.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(res) => assert!(res.locations.is_empty()),
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("missing.java"),
            Position::new(0, 0),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(res) => assert!(res.hover.is_none()),
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("missing.java"),
            Position::new(0, 0),
            QueryKind::References,
        )) {
            ResolveOutcome::NotReady => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn definition_type_use_skips_cross_file_variable_occurrences() {
        let usage = IndexedSymbol {
            file: FileId::new("PDFLocalCache.java"),
            uri: "file:///PDFLocalCache.java".into(),
            name: "KvStoreRouter".into(),
            kind: SymbolKind::Variable,
            range: Range::new(Position::new(10, 4), Position::new(10, 18)),
            selection_range: Range::new(Position::new(10, 4), Position::new(10, 18)),
            arity: None,
            fqn: "PDFLocalCache.KvStoreRouter".into(),
            container: None,
        };
        let idx = MemIndex::new(vec![
            sym(
                "KvStoreRouter.java",
                "KvStoreRouter",
                0,
                SymbolKind::Interface,
                "com.KvStoreRouter",
                None,
            ),
            sym(
                "Factory.java",
                "KvStoreRouter",
                5,
                SymbolKind::Variable,
                "com.Factory.KvStoreRouter",
                None,
            ),
            usage,
        ]);
        let r = TreeSitterResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("PDFLocalCache.java"),
            Position::new(10, 10),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.locations.len(), 1);
                assert!(res.locations[0].uri.contains("KvStoreRouter.java"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn implementation_of_class_usage_finds_declaration() {
        let usage = IndexedSymbol {
            file: FileId::new("CacheTest.java"),
            uri: "file:///CacheTest.java".into(),
            name: "DefaultLocalConfigService".into(),
            kind: SymbolKind::Variable,
            range: Range::new(Position::new(1, 4), Position::new(1, 29)),
            selection_range: Range::new(Position::new(1, 4), Position::new(1, 29)),
            arity: None,
            fqn: "CacheTest.DefaultLocalConfigService".into(),
            container: Some("CacheTest".into()),
        };
        let idx = MemIndex::new(vec![
            sym(
                "Svc.java",
                "DefaultLocalConfigService",
                0,
                SymbolKind::Class,
                "com.DefaultLocalConfigService",
                None,
            ),
            usage,
        ]);
        let r = TreeSitterResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("CacheTest.java"),
            Position::new(1, 10),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::Ready(res) => {
                assert!(
                    res.locations.iter().any(|l| l.uri.contains("Svc.java")),
                    "{:?}",
                    res.locations
                );
            }
            ResolveOutcome::NotReady => panic!("T1 implementation of a class usage"),
        }
    }

    #[test]
    fn empty_index_workspace_symbol() {
        let r = TreeSitterResolver::new(Arc::new(crate::query::EmptyIndex));
        match r.resolve(&ResolveQuery::workspace_symbol("x")) {
            ResolveOutcome::Ready(res) => assert!(res.locations.is_empty()),
            ResolveOutcome::NotReady => panic!("ready"),
        }
        let empty = crate::query::EmptyIndex;
        assert!(empty.symbols_in(&FileId::new("x")).is_empty());
        assert!(empty.all_symbols().is_empty());
    }

    #[test]
    fn identifier_prefers_method_over_overlapping_variable() {
        let method = IndexedSymbol {
            file: FileId::new("A.java"),
            uri: "file:///A.java".into(),
            name: "greet".into(),
            kind: SymbolKind::Method,
            range: Range::new(Position::new(2, 0), Position::new(2, 20)),
            selection_range: Range::new(Position::new(2, 0), Position::new(2, 5)),
            arity: Some(1),
            fqn: "Lib.greet".into(),
            container: None,
        };
        let variable = IndexedSymbol {
            file: FileId::new("A.java"),
            uri: "file:///A.java".into(),
            name: "greet".into(),
            kind: SymbolKind::Variable,
            range: Range::new(Position::new(2, 0), Position::new(2, 5)),
            selection_range: Range::new(Position::new(2, 0), Position::new(2, 5)),
            arity: None,
            fqn: "greet".into(),
            container: None,
        };
        let idx = MemIndex::new(vec![variable, method]);
        let r = TreeSitterResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(2, 1),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(res) => {
                let h = res.hover.expect("hover");
                assert_eq!(h.arity, Some(1));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn lookup_matches_simple_name_and_fqn_suffix() {
        let idx = MemIndex::new(vec![
            sym(
                "A.java",
                "run",
                1,
                SymbolKind::Method,
                "com.example.app.App.run",
                Some(0),
            ),
            sym(
                "B.java",
                "other",
                2,
                SymbolKind::Method,
                "com.example.lib.Lib.run",
                Some(0),
            ),
        ]);
        let r = TreeSitterResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(1, 0),
            QueryKind::References,
        )) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(
                    res.locations.len(),
                    2,
                    "name or .run suffix must both match"
                );
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        let by_fqn = MemIndex::new(vec![sym(
            "C.java",
            "Lib",
            0,
            SymbolKind::Class,
            "Lib",
            None,
        )]);
        let r2 = TreeSitterResolver::new(by_fqn);
        match r2.resolve(&ResolveQuery::new(
            FileId::new("C.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(res) => assert_eq!(res.locations.len(), 1),
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }
}
