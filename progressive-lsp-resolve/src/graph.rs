//! Type graph facts used by [`HeuristicResolver`](crate::HeuristicResolver).

use progressive_lsp_core::{FileId, PackageId, Tier};

use crate::query::Position;
use crate::tree_sitter::{IndexedSymbol, SymbolIndex};

/// One `import` / `use` declaration extracted from a CST.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportDecl {
    pub file: FileId,
    pub path: String,
    pub simple: String,
    pub wildcard: bool,
}

impl ImportDecl {
    pub fn new(file: FileId, path: impl Into<String>) -> Self {
        let path = path.into();
        let simple = path
            .rsplit(['.', '\\'])
            .next()
            .unwrap_or(path.as_str())
            .to_string();
        Self {
            file,
            path,
            simple,
            wildcard: false,
        }
    }

    pub fn wildcard(file: FileId, path: impl Into<String>) -> Self {
        let mut d = Self::new(file, path);
        d.wildcard = true;
        d
    }

    pub fn matches_name(&self, name: &str) -> bool {
        if self.wildcard {
            true
        } else {
            self.simple == name || self.path == name || self.path.ends_with(&format!(".{name}"))
        }
    }
}

/// Child type extends or implements parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeEdge {
    pub child_fqn: String,
    pub parent_fqn: String,
}

impl TypeEdge {
    pub fn new(child_fqn: impl Into<String>, parent_fqn: impl Into<String>) -> Self {
        Self {
            child_fqn: child_fqn.into(),
            parent_fqn: parent_fqn.into(),
        }
    }
}

/// Visitor output beyond declarations: imports, hierarchy, call-site arity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphFacts {
    pub imports: Vec<ImportDecl>,
    pub edges: Vec<TypeEdge>,
    pub package: Option<String>,
    pub calls: Vec<CallSite>,
}

impl GraphFacts {
    pub fn is_empty(&self) -> bool {
        self.imports.is_empty()
            && self.edges.is_empty()
            && self.package.is_none()
            && self.calls.is_empty()
    }
}

/// Identifier use with argument count (for arity Strategy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub file: FileId,
    pub name: String,
    pub arity: u32,
    pub line: u32,
    pub character: u32,
}

impl CallSite {
    pub fn new(file: FileId, name: impl Into<String>, arity: u32, line: u32, character: u32) -> Self {
        Self {
            file,
            name: name.into(),
            arity,
            line,
            character,
        }
    }

    pub fn covers(&self, pos: Position) -> bool {
        pos.line == self.line
            && pos.character >= self.character
            && pos.character <= self.character + self.name.len() as u32
    }
}

/// Graph facts + package ingest readiness. Same store as [`SymbolIndex`].
pub trait GraphIndex: SymbolIndex {
    fn imports_in(&self, file: &FileId) -> Vec<ImportDecl>;
    fn parents_of(&self, type_fqn: &str) -> Vec<String>;
    fn package_tier(&self, package: &PackageId) -> Option<Tier>;
    fn package_of_file(&self, file: &FileId) -> Option<PackageId>;
    fn call_at(&self, file: &FileId, pos: Position) -> Option<CallSite>;
}

impl GraphIndex for crate::query::EmptyIndex {
    fn imports_in(&self, _file: &FileId) -> Vec<ImportDecl> {
        Vec::new()
    }
    fn parents_of(&self, _type_fqn: &str) -> Vec<String> {
        Vec::new()
    }
    fn package_tier(&self, _package: &PackageId) -> Option<Tier> {
        None
    }
    fn package_of_file(&self, _file: &FileId) -> Option<PackageId> {
        None
    }
    fn call_at(&self, _file: &FileId, _pos: Position) -> Option<CallSite> {
        None
    }
}

/// Filter candidates by imported names and same-package FQN.
pub fn prefer_imported<'a>(
    name: &str,
    imports: &[ImportDecl],
    candidates: &'a [IndexedSymbol],
) -> Vec<&'a IndexedSymbol> {
    let imported: Vec<&ImportDecl> = imports.iter().filter(|i| i.matches_name(name)).collect();
    if imported.is_empty() {
        return candidates.iter().filter(|s| s.name == name).collect();
    }
    let mut hit: Vec<&IndexedSymbol> = candidates
        .iter()
        .filter(|s| {
            imported.iter().any(|i| {
                s.fqn == i.path
                    || s.fqn == format!("{}.{}", i.path, name)
                    || (i.wildcard && (s.fqn.starts_with(&format!("{}.", i.path)) || s.fqn == i.path) && s.name == name)
            })
        })
        .collect();
    if hit.is_empty() {
        hit = candidates.iter().filter(|s| s.name == name).collect();
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_decl_simple_and_wildcard() {
        let f = FileId::new("A.java");
        let d = ImportDecl::new(f.clone(), "com.example.lib.Lib");
        assert_eq!(d.simple, "Lib");
        assert!(d.matches_name("Lib"));
        assert!(d.matches_name("com.example.lib.Lib"));
        assert!(!d.matches_name("App"));
        let w = ImportDecl::wildcard(f, "com.example.lib");
        assert!(w.wildcard);
        assert!(w.matches_name("lib"));
        assert!(w.matches_name("App"), "star import matches any simple name");
        assert_eq!(TypeEdge::new("C", "P").parent_fqn, "P");
        assert!(GraphFacts::default().is_empty());
        let mut facts = GraphFacts::default();
        facts.package = Some("p".into());
        assert!(!facts.is_empty());
        let call = CallSite::new(FileId::new("f"), "greet", 1, 3, 8);
        assert!(call.covers(Position::new(3, 8)));
        assert!(call.covers(Position::new(3, 12)));
        assert!(!call.covers(Position::new(3, 20)));
        assert!(!call.covers(Position::new(2, 8)));
    }

    #[test]
    fn prefer_imported_picks_fqn() {
        let file = FileId::new("A.java");
        let imports = vec![ImportDecl::new(file.clone(), "com.Lib")];
        let local = IndexedSymbol {
            file: FileId::new("B.java"),
            uri: "file:///B.java".into(),
            name: "Lib".into(),
            kind: crate::SymbolKind::Class,
            range: crate::Range::default(),
            selection_range: crate::Range::default(),
            arity: None,
            fqn: "other.Lib".into(),
            container: None,
        };
        let imported = IndexedSymbol {
            file: FileId::new("L.java"),
            uri: "file:///L.java".into(),
            name: "Lib".into(),
            kind: crate::SymbolKind::Class,
            range: crate::Range::default(),
            selection_range: crate::Range::default(),
            arity: None,
            fqn: "com.Lib".into(),
            container: None,
        };
        let cands = vec![local, imported];
        let got = prefer_imported("Lib", &imports, &cands);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].fqn, "com.Lib");
        let none = prefer_imported("Nope", &imports, &cands);
        assert!(none.is_empty());
        let empty = crate::query::EmptyIndex;
        assert!(empty.imports_in(&file).is_empty());
        assert!(empty.parents_of("x").is_empty());
        assert!(empty.package_tier(&PackageId::new("p")).is_none());
        assert!(empty.package_of_file(&file).is_none());
        assert!(empty.call_at(&file, Position::default()).is_none());

        let wild = vec![ImportDecl::wildcard(file, "com.pkg")];
        let in_pkg = IndexedSymbol {
            file: FileId::new("F.java"),
            uri: "file:///F.java".into(),
            name: "Foo".into(),
            kind: crate::SymbolKind::Class,
            range: crate::Range::default(),
            selection_range: crate::Range::default(),
            arity: None,
            fqn: "com.pkg.Foo".into(),
            container: None,
        };
        let other = IndexedSymbol {
            file: FileId::new("O.java"),
            uri: "file:///O.java".into(),
            name: "Foo".into(),
            kind: crate::SymbolKind::Class,
            range: crate::Range::default(),
            selection_range: crate::Range::default(),
            arity: None,
            fqn: "other.Foo".into(),
            container: None,
        };
        let cands = vec![other, in_pkg];
        let got = prefer_imported("Foo", &wild, &cands);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].fqn, "com.pkg.Foo");
        let named = IndexedSymbol {
            file: FileId::new("X.java"),
            uri: "file:///X.java".into(),
            name: "Lib".into(),
            kind: crate::SymbolKind::Class,
            range: crate::Range::default(),
            selection_range: crate::Range::default(),
            arity: None,
            fqn: "pkg.Lib".into(),
            container: None,
        };
        let named_cands = [named];
        let by_name = prefer_imported("Lib", &[], &named_cands);
        assert_eq!(by_name.len(), 1);
    }
}
