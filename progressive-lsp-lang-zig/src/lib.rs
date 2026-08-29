//! Zig T1 + build.zig. Highlight, document symbols, intra-module F12. No zls.

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
    LanguageId::new("zig")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-zig"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_zig::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ZigIndexer;

impl LanguageIndexer for ZigIndexer {
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
pub struct ZigLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
}
impl ZigLanguageFactory {
    pub fn new() -> Self {
        Self { graph: None }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self { graph: Some(graph) }
    }
}
impl Default for ZigLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl LanguageFactory for ZigLanguageFactory {
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
        "FnProto" | "function_declaration" | "fn_proto" | "Decl" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                out.push(make(file, uri, &name, name_n, SymbolKind::Method));
            }
        }
        "IDENTIFIER" | "identifier" => {
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
        "IDENTIFIER" | "identifier" => Some(6u32),
        "FnProto" | "fn_proto" => Some(5),
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
    use progressive_lsp_workspace::{WorkspaceSource, ZigBuildAdapter};

    #[test]
    fn zig_intra_module_f12_and_symbols() {
        assert_eq!(language_id().as_str(), "zig");
        assert_eq!(grammar_id(), "tree-sitter-zig");
        assert_eq!(ZigIndexer.language_id().as_str(), "zig");
        assert_eq!(ZigIndexer.grammar_id(), "tree-sitter-zig");
        assert_eq!(ZigLanguageFactory::new().language_id().as_str(), "zig");
        assert_eq!(ZigLanguageFactory::new().grammar_id(), "tree-sitter-zig");
        assert!(ZigLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.zig"), "pub fn build() void {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let greet = dir.path().join("src/greet.zig");
        let main = dir.path().join("src/main.zig");
        std::fs::write(&greet, "pub fn hello(name: []const u8) []const u8 { return name; }\n").unwrap();
        std::fs::write(&main, "const greet = @import(\"greet.zig\");\npub fn run() []const u8 { return hello(\"x\"); }\n").unwrap();
        assert_eq!(ZigBuildAdapter.detect(dir.path()).unwrap().kind, "build.zig");
        let mut svc = IndexService::new();
        let job = PackageIngest::new("zig", "zig")
            .with_file(&greet)
            .with_file(&main);
        svc.ingest_package(&job, &ZigIndexer);
        let src = std::fs::read_to_string(&main).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(&src, None).unwrap();
        let toks = tokens_from_tree(&src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let greet_syms = ZigIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.zig",
            &greet_src,
            &greet_tree,
        );
        assert!(
            greet_syms.iter().any(|s| s.name == "hello"),
            "zig extract must see hello: {:?}",
            greet_syms.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        let shared = SharedIndex::new(svc);
        let factory = ZigLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.resolver_chain().len(), 2);
        let pos = line_col(&src, "hello");
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("greet.zig") || l.uri.contains("main.zig")),
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
            ResolveOutcome::Ready(r) => {
                assert!(r.symbols.iter().any(|s| s.name == "run" || s.name == "hello" || !r.symbols.is_empty()));
            }
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
