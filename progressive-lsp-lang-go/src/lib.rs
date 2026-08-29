//! Go T1 + go.mod. Highlight, document symbols, intra-module F12. No gopls.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphIndex, HeuristicResolver, IndexedSymbol, Position, Range, ResolverChain,
    SymbolKind, TreeSitterResolver,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("go")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-go"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_go::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GoIndexer;

impl LanguageIndexer for GoIndexer {
    fn language_id(&self) -> LanguageId {
        language_id()
    }
    fn grammar_id(&self) -> &'static str {
        grammar_id()
    }
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_language()
    }
    fn extract(&self, file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol> {
        let mut out = Vec::new();
        walk(tree.root_node(), source.as_bytes(), file, uri, &mut out);
        out
    }
}

#[derive(Clone)]
pub struct GoLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
}
impl GoLanguageFactory {
    pub fn new() -> Self {
        Self { graph: None }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self { graph: Some(graph) }
    }
}
impl Default for GoLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl LanguageFactory for GoLanguageFactory {
    fn language_id(&self) -> LanguageId {
        language_id()
    }
    fn grammar_id(&self) -> &str {
        grammar_id()
    }
    fn resolver_chain(&self) -> ResolverChain {
        match &self.graph {
            Some(g) => ResolverChain::new(vec![
                Box::new(HeuristicResolver::new(g.clone())),
                Box::new(TreeSitterResolver::new(g.clone())),
            ]),
            None => ResolverChain::empty(),
        }
    }
}

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    match node.kind() {
        "function_declaration" | "method_declaration" | "type_declaration" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = if node.kind() == "type_declaration" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Method
                };
                out.push(make(file, uri, &name, name_n, kind));
            }
        }
        "type_identifier" | "identifier" | "field_identifier" => {
            let name = node.utf8_text(src).unwrap_or("").to_string();
            if !name.is_empty() {
                out.push(make(file, uri, &name, node, SymbolKind::Variable));
            }
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, src, file, uri, out);
    }
}

fn make(file: &FileId, uri: &str, name: &str, node: Node, kind: SymbolKind) -> IndexedSymbol {
    let range = Range::new(
        Position::new(node.start_position().row as u32, node.start_position().column as u32),
        Position::new(node.end_position().row as u32, node.end_position().column as u32),
    );
    IndexedSymbol {
        file: file.clone(),
        uri: uri.to_string(),
        name: name.to_string(),
        kind,
        range,
        selection_range: range,
        arity: None,
        fqn: name.to_string(),
        container: None,
    }
}

pub fn tokens_from_tree(source: &str, tree: &Tree) -> Vec<u32> {
    let mut raw = Vec::new();
    collect(tree.root_node(), source.as_bytes(), &mut raw);
    let mut data = Vec::new();
    let mut pl = 0u32;
    let mut ps = 0u32;
    for &(line, start, len, ty) in &raw {
        let dl = line.saturating_sub(pl);
        let ds = if dl == 0 { start.saturating_sub(ps) } else { start };
        data.extend_from_slice(&[dl, ds, len, ty, 0]);
        pl = line;
        ps = start;
    }
    data
}

fn collect(node: Node, src: &[u8], out: &mut Vec<(u32, u32, u32, u32)>) {
    let ty = match node.kind() {
        "type_identifier" => Some(1u32),
        "function_declaration" => Some(5),
        "identifier" => Some(6),
        _ => None,
    };
    if let Some(t) = ty {
        if node.start_position().row == node.end_position().row {
            let text = node.utf8_text(src).unwrap_or("");
            if !text.is_empty() {
                out.push((
                    node.start_position().row as u32,
                    node.start_position().column as u32,
                    text.len() as u32,
                    t,
                ));
            }
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect(child, src, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};
    use progressive_lsp_workspace::{GoModAdapter, WorkspaceSource};

    #[test]
    fn go_intra_module_f12_and_symbols() {
        assert_eq!(language_id().as_str(), "go");
        assert_eq!(grammar_id(), "tree-sitter-go");
        assert_eq!(GoIndexer.language_id().as_str(), "go");
        assert_eq!(GoIndexer.grammar_id(), "tree-sitter-go");
        assert_eq!(GoLanguageFactory::new().language_id().as_str(), "go");
        assert_eq!(GoLanguageFactory::new().grammar_id(), "tree-sitter-go");
        assert!(GoLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/greet\ngo 1.22\n").unwrap();
        let greet = dir.path().join("greet.go");
        let main = dir.path().join("main.go");
        std::fs::write(&greet, "package greet\nfunc Hello(name string) string { return name }\n").unwrap();
        std::fs::write(&main, "package greet\nfunc Run() string { return Hello(\"x\") }\n").unwrap();
        assert_eq!(GoModAdapter.detect(dir.path()).unwrap().kind, "go.mod");
        let mut svc = IndexService::new();
        let job = PackageIngest::new("greet", "go")
            .with_file(&greet)
            .with_file(&main);
        svc.ingest_package(&job, &GoIndexer);
        let src = std::fs::read_to_string(&main).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(&src, None).unwrap();
        let toks = tokens_from_tree(&src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&5) || types.contains(&6));
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let greet_syms = GoIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.go",
            &greet_src,
            &greet_tree,
        );
        assert!(greet_syms.iter().any(|s| s.name == "Hello" && s.kind == SymbolKind::Method));
        let shared = SharedIndex::new(svc);
        let factory = GoLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.resolver_chain().len(), 2);
        let pos = line_col(&src, "Hello");
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("greet.go")),
                    "{:?}",
                    r.locations
                );
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            Position::default(),
            QueryKind::DocumentSymbol,
        )) {
            ResolveOutcome::Ready(r) => assert!(r.symbols.iter().any(|s| s.name == "Run")),
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    fn line_col(src: &str, needle: &str) -> Position {
        let byte = src.find(needle).expect(needle);
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in src.char_indices() {
            if i == byte {
                return Position::new(line, col);
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        panic!("{needle}");
    }
}
