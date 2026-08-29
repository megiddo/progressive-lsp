//! JavaScript T1 via tree-sitter-javascript. TypeScript highlighting uses the same grammar.
//! Not Node/tsserver. oxc T2 / tsgo T3 are M4.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    IndexedSymbol, Position, Range, ResolverChain, SymbolIndex, SymbolKind, TreeSitterResolver,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("javascript")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-javascript"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_javascript::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JavaScriptIndexer;

impl LanguageIndexer for JavaScriptIndexer {
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
pub struct JavaScriptLanguageFactory {
    index: Option<Arc<dyn SymbolIndex>>,
    typescript: bool,
}
impl JavaScriptLanguageFactory {
    pub fn new() -> Self {
        Self {
            index: None,
            typescript: false,
        }
    }
    pub fn typescript() -> Self {
        Self {
            index: None,
            typescript: true,
        }
    }
    pub fn with_index(index: Arc<dyn SymbolIndex>) -> Self {
        Self {
            index: Some(index),
            typescript: false,
        }
    }
}
impl Default for JavaScriptLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl LanguageFactory for JavaScriptLanguageFactory {
    fn language_id(&self) -> LanguageId {
        if self.typescript {
            LanguageId::new("typescript")
        } else {
            language_id()
        }
    }
    fn grammar_id(&self) -> &str {
        grammar_id()
    }
    fn resolver_chain(&self) -> ResolverChain {
        match &self.index {
            Some(i) => ResolverChain::new(vec![Box::new(TreeSitterResolver::new(i.clone()))]),
            None => ResolverChain::empty(),
        }
    }
}

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    match node.kind() {
        "function_declaration" | "method_definition" | "class_declaration" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = if node.kind() == "class_declaration" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Method
                };
                out.push(make(file, uri, &name, name_n, kind));
            }
        }
        "identifier" | "property_identifier" => {
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
        "class_declaration" => Some(2u32),
        "function_declaration" | "method_definition" => Some(5),
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
    use progressive_lsp_index::{IndexService, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};

    #[test]
    fn js_and_ts_t1_symbols_and_tokens() {
        assert_eq!(language_id().as_str(), "javascript");
        assert_eq!(grammar_id(), "tree-sitter-javascript");
        assert_eq!(JavaScriptIndexer.language_id().as_str(), "javascript");
        assert_eq!(JavaScriptIndexer.grammar_id(), "tree-sitter-javascript");
        assert_eq!(JavaScriptLanguageFactory::new().language_id().as_str(), "javascript");
        assert_eq!(JavaScriptLanguageFactory::new().grammar_id(), "tree-sitter-javascript");
        assert_eq!(JavaScriptLanguageFactory::typescript().language_id().as_str(), "typescript");
        assert_eq!(JavaScriptLanguageFactory::typescript().grammar_id(), "tree-sitter-javascript");
        assert!(JavaScriptLanguageFactory::default().resolver_chain().is_empty());
        let src = "class App { greet(name) { return name; } }\nfunction run() { return greet; }\n";
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let file = FileId::new("a.js");
        let syms = JavaScriptIndexer.extract(&file, "file:///a.js", src, &tree);
        assert!(syms.iter().any(|s| s.name == "App" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Method));
        assert!(syms.iter().any(|s| s.name == "run" && s.kind == SymbolKind::Method));
        assert!(syms.iter().any(|s| s.name == "name" && s.kind == SymbolKind::Variable));
        let app = syms.iter().find(|s| s.name == "App").unwrap();
        assert_eq!(
            app.range.end.character,
            app.range.start.character + "App".len() as u32
        );
        let toks = tokens_from_tree(src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&2), "class_declaration");
        assert!(types.contains(&5), "function/method");
        assert!(types.contains(&6), "identifier");
        let ts = "const x: number = 1; function id<T>(v: T) { return v; }";
        let tree_ts = p.parse(ts, None).unwrap();
        let ts_syms = JavaScriptIndexer.extract(&FileId::new("a.ts"), "file:///a.ts", ts, &tree_ts);
        assert!(ts_syms.iter().any(|s| s.name == "id"));
        let mut svc = IndexService::new();
        svc.index_text(std::path::Path::new("a.js"), src, &JavaScriptIndexer, false);
        let factory = JavaScriptLanguageFactory::with_index(Arc::new(SharedIndex::new(svc)));
        assert_eq!(factory.resolver_chain().len(), 1);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.js"),
            Position::default(),
            QueryKind::DocumentSymbol,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(r.symbols.iter().any(|s| s.name == "App"));
                assert!(r.symbols.iter().any(|s| s.name == "run"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }
}
