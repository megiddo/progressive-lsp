//! Default T2 Strategy: name/arity, import, hierarchy, scope. Same [`Resolver`] trait as T1.

use std::sync::Arc;

use progressive_lsp_core::Tier;

use crate::graph::{prefer_imported, GraphIndex};
use crate::query::{
    Hover, LspLocation, QueryKind, ResolveOutcome, ResolveQuery, ResolveResult, SymbolKind,
};
use crate::tree_sitter::IndexedSymbol;
use crate::Resolver;

/// Heuristic graph resolver. Used only after a package finishes ingest (T2).
pub struct HeuristicResolver {
    index: Arc<dyn GraphIndex>,
}

impl HeuristicResolver {
    pub fn new(index: Arc<dyn GraphIndex>) -> Self {
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

    fn package_ready_for(&self, q: &ResolveQuery) -> bool {
        match self.index.package_of_file(&q.file) {
            Some(pkg) => self.index.package_tier(&pkg) == Some(Tier::Graph),
            None => false,
        }
    }

    fn lookup(&self, q: &ResolveQuery, at: &IndexedSymbol) -> Vec<IndexedSymbol> {
        let all = self.index.all_symbols();
        let imports = self.index.imports_in(&q.file);
        let call = self.index.call_at(&q.file, q.position);
        let arity = call.as_ref().map(|c| c.arity).or(at.arity);
        let mut preferred: Vec<IndexedSymbol> =
            prefer_imported(&at.name, &imports, &all).into_iter().cloned().collect();
        if preferred.is_empty() {
            preferred = all
                .iter()
                .filter(|s| s.name == at.name || s.fqn.ends_with(&format!(".{}", at.name)))
                .cloned()
                .collect();
        }
        if let Some(n) = arity {
            let by_arity: Vec<IndexedSymbol> = preferred
                .iter()
                .filter(|s| s.arity == Some(n))
                .cloned()
                .collect();
            if !by_arity.is_empty() {
                preferred = by_arity;
            }
        }
        let decls: Vec<IndexedSymbol> = preferred
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Method
                        | SymbolKind::Constructor
                        | SymbolKind::Class
                        | SymbolKind::Interface
                        | SymbolKind::Enum
                        | SymbolKind::Field
                )
            })
            .cloned()
            .collect();
        if !decls.is_empty() {
            preferred = decls;
        }
        if let Some(container) = &at.container {
            let parents = self.index.parents_of(container);
            let mut scoped: Vec<IndexedSymbol> = preferred
                .iter()
                .filter(|s| {
                    s.container.as_deref() == Some(container.as_str())
                        || s.fqn == *container
                        || parents.iter().any(|p| {
                            s.container.as_deref() == Some(p.as_str()) || s.fqn == *p
                        })
                })
                .cloned()
                .collect();
            if scoped.is_empty() {
                scoped = preferred;
            }
            preferred = scoped;
        }
        preferred
    }
}

impl Resolver for HeuristicResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        if !self.package_ready_for(q) && q.kind != QueryKind::WorkspaceSymbol {
            return ResolveOutcome::NotReady;
        }
        let tier = Tier::Graph;
        match q.kind {
            QueryKind::DocumentSymbol | QueryKind::WorkspaceSymbol => ResolveOutcome::NotReady,
            QueryKind::Hover => {
                let Some(sym) = self.identifier_at(q) else {
                    return ResolveOutcome::NotReady;
                };
                let hits = self.lookup(q, &sym);
                let best = hits.first().cloned().unwrap_or(sym);
                let hover = Hover {
                    name: best.name.clone(),
                    arity: best.arity,
                };
                ResolveOutcome::Ready(ResolveResult {
                    locations: vec![best.to_location(tier)],
                    tier,
                    hover: Some(hover),
                    symbols: Vec::new(),
                })
            }
            QueryKind::Definition | QueryKind::TypeDefinition | QueryKind::References => {
                let Some(at) = self.identifier_at(q) else {
                    return ResolveOutcome::NotReady;
                };
                let mut hits = self.lookup(q, &at);
                if q.kind == QueryKind::TypeDefinition {
                    hits.retain(|s| {
                        matches!(
                            s.kind,
                            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
                        )
                    });
                }
                if hits.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                let locs: Vec<LspLocation> = hits.into_iter().map(|s| s.to_location(tier)).collect();
                ResolveOutcome::Ready(ResolveResult::locations(tier, locs))
            }
            QueryKind::Implementation => {
                let Some(at) = self.identifier_at(q) else {
                    return ResolveOutcome::NotReady;
                };
                let children: Vec<LspLocation> = self
                    .index
                    .all_symbols()
                    .into_iter()
                    .filter(|s| {
                        self.index
                            .parents_of(&s.fqn)
                            .iter()
                            .any(|p| p == &at.fqn || p == &at.name)
                    })
                    .map(|s| s.to_location(tier))
                    .collect();
                if children.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(tier, children))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CallSite, ImportDecl, TypeEdge};
    use crate::query::{Position, Range};
    use progressive_lsp_core::{FileId, PackageId};
    use std::sync::Mutex;

    struct MemGraph {
        symbols: Mutex<Vec<IndexedSymbol>>,
        imports: Vec<ImportDecl>,
        edges: Vec<TypeEdge>,
        ready: bool,
        pkg: PackageId,
        calls: Vec<CallSite>,
    }

    impl crate::tree_sitter::SymbolIndex for MemGraph {
        fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol> {
            self.symbols
                .lock()
                .unwrap()
                .iter()
                .filter(|s| &s.file == file)
                .cloned()
                .collect()
        }
        fn all_symbols(&self) -> Vec<IndexedSymbol> {
            self.symbols.lock().unwrap().clone()
        }
    }

    impl GraphIndex for MemGraph {
        fn imports_in(&self, _file: &FileId) -> Vec<ImportDecl> {
            self.imports.clone()
        }
        fn parents_of(&self, type_fqn: &str) -> Vec<String> {
            self.edges
                .iter()
                .filter(|e| e.child_fqn == type_fqn)
                .map(|e| e.parent_fqn.clone())
                .collect()
        }
        fn package_tier(&self, _package: &PackageId) -> Option<Tier> {
            if self.ready {
                Some(Tier::Graph)
            } else {
                Some(Tier::Syntax)
            }
        }
        fn package_of_file(&self, _file: &FileId) -> Option<PackageId> {
            Some(self.pkg.clone())
        }
        fn call_at(&self, file: &FileId, pos: Position) -> Option<CallSite> {
            self.calls
                .iter()
                .find(|c| &c.file == file && c.covers(pos))
                .cloned()
        }
    }

    fn sym(file: &str, name: &str, line: u32, kind: SymbolKind, fqn: &str, arity: Option<u32>) -> IndexedSymbol {
        let range = Range::new(Position::new(line, 0), Position::new(line, 20));
        IndexedSymbol {
            file: FileId::new(file),
            uri: format!("file:///{file}"),
            name: name.into(),
            kind,
            range,
            selection_range: Range::new(Position::new(line, 0), Position::new(line, name.len() as u32)),
            arity,
            fqn: fqn.into(),
            container: Some(fqn.rsplit_once('.').map(|(c, _)| c.to_string()).unwrap_or_default()),
        }
    }

    #[test]
    fn not_ready_until_package_is_graph() {
        let idx = Arc::new(MemGraph {
            symbols: Mutex::new(vec![sym("A.java", "Lib", 0, SymbolKind::Class, "com.Lib", None)]),
            imports: vec![],
            edges: vec![],
            ready: false,
            pkg: PackageId::new("app"),
            calls: vec![],
        });
        let r = HeuristicResolver::new(idx);
        let q = ResolveQuery::new(FileId::new("A.java"), Position::new(0, 0), QueryKind::Definition);
        assert!(!r.resolve(&q).is_ready());
    }

    #[test]
    fn import_and_arity_and_hierarchy() {
        let file = FileId::new("App.java");
        let idx = Arc::new(MemGraph {
            symbols: Mutex::new(vec![
                IndexedSymbol {
                    file: file.clone(),
                    uri: "file:///App.java".into(),
                    name: "greet".into(),
                    kind: SymbolKind::Variable,
                    range: Range::new(Position::new(4, 15), Position::new(4, 20)),
                    selection_range: Range::new(Position::new(4, 15), Position::new(4, 20)),
                    arity: None,
                    fqn: "app.App.greet".into(),
                    container: Some("app.App".into()),
                },
                IndexedSymbol {
                    file: FileId::new("Lib.java"),
                    uri: "file:///Lib.java".into(),
                    name: "greet".into(),
                    kind: SymbolKind::Method,
                    range: Range::new(Position::new(2, 0), Position::new(2, 20)),
                    selection_range: Range::new(Position::new(2, 0), Position::new(2, 5)),
                    arity: Some(1),
                    fqn: "com.Lib.greet".into(),
                    container: Some("com.Lib".into()),
                },
                IndexedSymbol {
                    file: FileId::new("Lib.java"),
                    uri: "file:///Lib.java".into(),
                    name: "greet".into(),
                    kind: SymbolKind::Method,
                    range: Range::new(Position::new(3, 0), Position::new(3, 20)),
                    selection_range: Range::new(Position::new(3, 0), Position::new(3, 5)),
                    arity: Some(2),
                    fqn: "com.Lib.greet".into(),
                    container: Some("com.Lib".into()),
                },
                IndexedSymbol {
                    file: FileId::new("Base.java"),
                    uri: "file:///Base.java".into(),
                    name: "run".into(),
                    kind: SymbolKind::Method,
                    range: Range::new(Position::new(1, 0), Position::new(1, 10)),
                    selection_range: Range::new(Position::new(1, 0), Position::new(1, 3)),
                    arity: Some(0),
                    fqn: "com.Base.run".into(),
                    container: Some("com.Base".into()),
                },
                IndexedSymbol {
                    file: FileId::new("Base.java"),
                    uri: "file:///Base.java".into(),
                    name: "Base".into(),
                    kind: SymbolKind::Class,
                    range: Range::new(Position::new(0, 0), Position::new(0, 20)),
                    selection_range: Range::new(Position::new(0, 0), Position::new(0, 4)),
                    arity: None,
                    fqn: "com.Base".into(),
                    container: None,
                },
                IndexedSymbol {
                    file: FileId::new("Child.java"),
                    uri: "file:///Child.java".into(),
                    name: "Child".into(),
                    kind: SymbolKind::Class,
                    range: Range::new(Position::new(0, 0), Position::new(0, 20)),
                    selection_range: Range::new(Position::new(0, 0), Position::new(0, 5)),
                    arity: None,
                    fqn: "com.Child".into(),
                    container: None,
                },
            ]),
            imports: vec![ImportDecl::new(file.clone(), "com.Lib")],
            edges: vec![TypeEdge::new("com.Child", "com.Base")],
            ready: true,
            pkg: PackageId::new("app"),
            calls: vec![CallSite::new(file.clone(), "greet", 1, 4, 15)],
        });
        let r = HeuristicResolver::new(idx);
        match r.resolve(&ResolveQuery::new(
            FileId::new("App.java"),
            Position::new(4, 16),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.tier, Tier::Graph);
                assert_eq!(res.locations.len(), 1);
                assert_eq!(res.locations[0].range.start.line, 2);
            }
            ResolveOutcome::NotReady => panic!("T2 ready"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("Base.java"),
            Position::new(0, 1),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::Ready(res) => {
                assert!(res.locations.iter().any(|l| l.uri.contains("Child.java")), "{:?}", res.locations);
            }
            ResolveOutcome::NotReady => panic!("implementation of Base is Child"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("Child.java"),
            Position::new(0, 1),
            QueryKind::TypeDefinition,
        )) {
            ResolveOutcome::Ready(res) => {
                assert!(res.locations.iter().any(|l| l.uri.contains("Child.java")));
            }
            ResolveOutcome::NotReady => panic!("type def"),
        }
        match r.resolve(&ResolveQuery::workspace_symbol("x")) {
            ResolveOutcome::NotReady => {}
            other => panic!("{other:?}"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("App.java"),
            Position::new(4, 16),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(res) => {
                assert_eq!(res.tier, Tier::Graph);
                assert!(res.hover.is_some());
            }
            ResolveOutcome::NotReady => panic!("hover"),
        }
        match r.resolve(&ResolveQuery::new(
            FileId::new("missing.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        )) {
            ResolveOutcome::NotReady => {}
            ResolveOutcome::Ready(r) => panic!("{r:?}"),
        }
    }
}
