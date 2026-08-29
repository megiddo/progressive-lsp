//! Query / Command types. Protocol builds these; resolvers do not parse JSON-RPC.

use progressive_lsp_core::{FileId, Tier};

use crate::tree_sitter::IndexedSymbol;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    pub fn is_within(self, range: Range) -> bool {
        if self.line < range.start.line || self.line > range.end.line {
            return false;
        }
        if self.line == range.start.line && self.character < range.start.character {
            return false;
        }
        if self.line == range.end.line && self.character > range.end.character {
            return false;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn point(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryKind {
    Definition,
    References,
    TypeDefinition,
    Implementation,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
}

impl QueryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::References => "references",
            Self::TypeDefinition => "typeDefinition",
            Self::Implementation => "implementation",
            Self::Hover => "hover",
            Self::DocumentSymbol => "documentSymbol",
            Self::WorkspaceSymbol => "workspaceSymbol",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveQuery {
    pub file: FileId,
    pub position: Position,
    pub kind: QueryKind,
    /// Workspace symbol query text. Ignored for other kinds.
    pub symbol_query: Option<String>,
}

impl ResolveQuery {
    pub fn new(file: FileId, position: Position, kind: QueryKind) -> Self {
        Self {
            file,
            position,
            kind,
            symbol_query: None,
        }
    }

    pub fn workspace_symbol(query: impl Into<String>) -> Self {
        Self {
            file: FileId::new(""),
            position: Position::default(),
            kind: QueryKind::WorkspaceSymbol,
            symbol_query: Some(query.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspLocation {
    pub uri: String,
    pub range: Range,
    pub tier: Tier,
}

impl LspLocation {
    pub fn new(uri: impl Into<String>, range: Range, tier: Tier) -> Self {
        Self {
            uri: uri.into(),
            range,
            tier,
        }
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    pub name: String,
    pub arity: Option<u32>,
    /// T3 type text (ty / rust-analyzer). Absent at T1/T2.
    pub type_info: Option<String>,
}

impl Hover {
    pub fn signature(&self) -> String {
        match (&self.type_info, self.arity) {
            (Some(ty), _) => format!("{}: {ty}", self.name),
            (None, Some(n)) => format!("{}({})", self.name, n),
            (None, None) => self.name.clone(),
        }
    }

    pub fn named(name: impl Into<String>, arity: Option<u32>) -> Self {
        Self {
            name: name.into(),
            arity,
            type_info: None,
        }
    }

    pub fn typed(name: impl Into<String>, type_info: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arity: None,
            type_info: Some(type_info.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    File,
    Class,
    Interface,
    Enum,
    Method,
    Constructor,
    Field,
    Variable,
    Package,
}

impl SymbolKind {
    pub fn lsp_number(self) -> i64 {
        match self {
            Self::File => 1,
            Self::Class => 5,
            Self::Interface => 11,
            Self::Enum => 10,
            Self::Method => 6,
            Self::Constructor => 9,
            Self::Field => 8,
            Self::Variable => 13,
            Self::Package => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub arity: Option<u32>,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveResult {
    pub locations: Vec<LspLocation>,
    pub tier: Tier,
    pub hover: Option<Hover>,
    pub symbols: Vec<DocumentSymbol>,
}

impl ResolveResult {
    pub fn empty(tier: Tier) -> Self {
        Self {
            locations: Vec::new(),
            tier,
            hover: None,
            symbols: Vec::new(),
        }
    }

    pub fn locations(tier: Tier, locations: Vec<LspLocation>) -> Self {
        Self {
            locations,
            tier,
            hover: None,
            symbols: Vec::new(),
        }
    }
}

/// Chain of Responsibility outcome. `NotReady` means try the next handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    Ready(ResolveResult),
    NotReady,
}

impl ResolveOutcome {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

/// Empty symbol store used by default Tree-sitter resolvers in tests.
pub struct EmptyIndex;

impl crate::tree_sitter::SymbolIndex for EmptyIndex {
    fn symbols_in(&self, _file: &FileId) -> Vec<IndexedSymbol> {
        Vec::new()
    }

    fn all_symbols(&self) -> Vec<IndexedSymbol> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FileId;

    #[test]
    fn position_within_range_inclusive() {
        let range = Range::new(Position::new(1, 2), Position::new(3, 4));
        assert!(Position::new(1, 2).is_within(range));
        assert!(Position::new(2, 0).is_within(range));
        assert!(Position::new(3, 4).is_within(range));
        assert!(!Position::new(1, 1).is_within(range));
        assert!(!Position::new(3, 5).is_within(range));
        assert!(!Position::new(0, 9).is_within(range));
        assert!(!Position::new(4, 0).is_within(range));
        assert_eq!(Range::point(Position::new(5, 6)).start.line, 5);
    }

    #[test]
    fn query_kind_names() {
        assert_eq!(QueryKind::Definition.as_str(), "definition");
        assert_eq!(QueryKind::References.as_str(), "references");
        assert_eq!(QueryKind::TypeDefinition.as_str(), "typeDefinition");
        assert_eq!(QueryKind::Implementation.as_str(), "implementation");
        assert_eq!(QueryKind::Hover.as_str(), "hover");
        assert_eq!(QueryKind::DocumentSymbol.as_str(), "documentSymbol");
        assert_eq!(QueryKind::WorkspaceSymbol.as_str(), "workspaceSymbol");
    }

    #[test]
    fn resolve_query_builders() {
        let q = ResolveQuery::new(FileId::new("A.java"), Position::new(1, 2), QueryKind::Hover);
        assert_eq!(q.file.as_str(), "A.java");
        assert_eq!(q.kind, QueryKind::Hover);
        assert_eq!(q.symbol_query, None);
        let ws = ResolveQuery::workspace_symbol("Lib");
        assert_eq!(ws.kind, QueryKind::WorkspaceSymbol);
        assert_eq!(ws.symbol_query.as_deref(), Some("Lib"));
        assert_eq!(ws.file.as_str(), "");
    }

    #[test]
    fn hover_signature_name_and_arity() {
        let with = Hover {
            name: "greet".into(),
            arity: Some(2),
            type_info: None,
        };
        assert_eq!(with.signature(), "greet(2)");
        let none = Hover {
            name: "Lib".into(),
            arity: None,
            type_info: None,
        };
        assert_eq!(none.signature(), "Lib");
        let typed = Hover::typed("x", "int");
        assert_eq!(typed.signature(), "x: int");
        assert_eq!(Hover::named("y", Some(1)).signature(), "y(1)");
    }

    #[test]
    fn symbol_kind_lsp_numbers_are_distinct() {
        let kinds = [
            SymbolKind::File,
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::Enum,
            SymbolKind::Method,
            SymbolKind::Constructor,
            SymbolKind::Field,
            SymbolKind::Variable,
            SymbolKind::Package,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for k in kinds {
            assert!(seen.insert(k.lsp_number()), "{k:?}");
        }
        assert_eq!(SymbolKind::Class.lsp_number(), 5);
        assert_eq!(SymbolKind::Method.lsp_number(), 6);
    }

    #[test]
    fn location_holds_tier_for_protocol() {
        let loc = LspLocation::new(
            "file:///a/Lib.java",
            Range::point(Position::new(2, 3)),
            Tier::Syntax,
        );
        assert_eq!(loc.uri, "file:///a/Lib.java");
        assert_eq!(loc.tier, Tier::Syntax);
        assert_eq!(loc.range.start.line, 2);
        assert_eq!(loc.range.start.character, 3);
    }

    #[test]
    fn resolve_result_helpers() {
        let empty = ResolveResult::empty(Tier::Graph);
        assert!(empty.locations.is_empty());
        assert_eq!(empty.tier, Tier::Graph);
        assert!(empty.hover.is_none());
        let ready = ResolveResult::locations(Tier::Syntax, vec![LspLocation::new(
            "u",
            Range::default(),
            Tier::Syntax,
        )]);
        assert_eq!(ready.locations.len(), 1);
        assert!(ResolveOutcome::Ready(empty).is_ready());
        assert!(!ResolveOutcome::NotReady.is_ready());
    }
}
