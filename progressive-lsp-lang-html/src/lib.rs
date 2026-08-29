//! HTML T1 + superhtml T3 when pack ready. T1 fallback if pack absent.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphIndex, IndexedSymbol, Position, Range, ResolverChain, SymbolKind, TreeSitterResolver,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("html")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-html"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_html::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HtmlIndexer;

impl LanguageIndexer for HtmlIndexer {
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
        extract_symbols(file, uri, source, tree)
    }
}

#[derive(Clone)]
pub struct HtmlLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
}
impl HtmlLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            supervisor: None,
        }
    }
    pub fn with_index(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            graph: Some(graph),
            supervisor: None,
        }
    }
    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }
}
impl Default for HtmlLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl LanguageFactory for HtmlLanguageFactory {
    fn language_id(&self) -> LanguageId {
        language_id()
    }
    fn grammar_id(&self) -> &str {
        grammar_id()
    }
    fn resolver_chain(&self) -> ResolverChain {
        match &self.graph {
            Some(g) => {
                let t3 = self.supervisor.as_ref().map(|s| {
                    Box::new(EngineResolver::new(
                        s.clone(),
                        language_id(),
                        PackageId::new("pkg"),
                    )) as Box<dyn progressive_lsp_resolve::Resolver>
                });
                ResolverChain::with_tiers(t3, None, Box::new(TreeSitterResolver::new(g.clone())))
            }
            None => ResolverChain::empty(),
        }
    }
}

fn extract_symbols(file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol> {
    let mut out = Vec::new();
    walk(tree.root_node(), source.as_bytes(), file, uri, &mut out);
    out
}

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    if node.kind() == "tag_name" {
        let name = node.utf8_text(src).unwrap_or("").to_string();
        if !name.is_empty() {
            out.push(make(
                file,
                uri,
                &name,
                node.start_position().row as u32,
                node.start_position().column as u32,
                SymbolKind::Class,
            ));
        }
    }
    if node.kind() == "attribute_name" || node.kind() == "attribute_value" {
        let name = node.utf8_text(src).unwrap_or("").to_string();
        if !name.is_empty() {
            out.push(make(
                file,
                uri,
                &name,
                node.start_position().row as u32,
                node.start_position().column as u32,
                SymbolKind::Field,
            ));
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, src, file, uri, out);
    }
}

fn make(file: &FileId, uri: &str, name: &str, line: u32, col: u32, kind: SymbolKind) -> IndexedSymbol {
    let range = Range::new(Position::new(line, col), Position::new(line, col + name.len() as u32));
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
    encode(&raw)
}

fn collect(node: Node, src: &[u8], out: &mut Vec<(u32, u32, u32, u32)>) {
    let ty = match node.kind() {
        "tag_name" => Some(2u32),
        "attribute_name" => Some(8),
        "attribute_value" => Some(6),
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

fn encode(toks: &[(u32, u32, u32, u32)]) -> Vec<u32> {
    let mut data = Vec::new();
    let mut pl = 0u32;
    let mut ps = 0u32;
    for &(line, start, len, ty) in toks {
        let dl = line.saturating_sub(pl);
        let ds = if dl == 0 { start.saturating_sub(ps) } else { start };
        data.extend_from_slice(&[dl, ds, len, ty, 0]);
        pl = line;
        ps = start;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_index::{IndexService, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};

    #[test]
    fn html_symbols_tokens_and_id_usages() {
        assert_eq!(language_id().as_str(), "html");
        assert_eq!(grammar_id(), "tree-sitter-html");
        assert_eq!(HtmlIndexer.language_id().as_str(), "html");
        assert_eq!(HtmlIndexer.grammar_id(), "tree-sitter-html");
        let f = HtmlLanguageFactory::new();
        assert_eq!(f.language_id().as_str(), "html");
        assert_eq!(f.grammar_id(), "tree-sitter-html");
        assert!(f.resolver_chain().is_empty());
        assert!(HtmlLanguageFactory::default().resolver_chain().is_empty());
        let src = r#"<div id="main" class="box extra"><span id='main'>x</span></div>"#;
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let file = FileId::new("a.html");
        let syms = HtmlIndexer.extract(&file, "file:///a.html", src, &tree);
        assert!(syms.iter().any(|s| s.name == "div" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "span" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "main"));
        assert!(syms.iter().any(|s| s.name == "id" && s.kind == SymbolKind::Field));
        assert!(syms.iter().any(|s| s.name.contains("box") || s.name.contains("extra") || s.name == "class"));
        assert!(syms.iter().all(|s| !s.name.is_empty()));
        assert_eq!(
            encode(&[(0, 1, 3, 2), (0, 8, 4, 8)]),
            vec![0, 1, 3, 2, 0, 0, 7, 4, 8, 0]
        );
        assert_eq!(encode(&[(0, 1, 2, 2), (1, 0, 3, 8)]), vec![0, 1, 2, 2, 0, 1, 0, 3, 8, 0]);
        let toks = tokens_from_tree(src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&2), "tag_name");
        assert!(types.contains(&8), "attribute_name");
        assert!(types.contains(&6), "attribute_value");
        let mut svc = IndexService::new();
        svc.index_text(std::path::Path::new("a.html"), src, &HtmlIndexer, false);
        let factory = HtmlLanguageFactory::with_index(Arc::new(SharedIndex::new(svc)));
        assert_eq!(factory.resolver_chain().len(), 1);
        let pos = Position::new(0, src.find("main").unwrap() as u32);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.html"),
            pos,
            QueryKind::References,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(r.locations.len() >= 2, "id find-usages {:?}", r.locations);
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.html"),
            Position::default(),
            QueryKind::DocumentSymbol,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(r.symbols.iter().any(|s| s.name == "div"));
                assert!(r.symbols.iter().any(|s| s.name == "main"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn html_t3_via_fake_superhtml() {
        use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
        use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter};
        use progressive_lsp_index::IndexService;
        use std::path::PathBuf;

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::superhtml();
        fake.set_answers(FakeEngineAdapter::typed_fixture("main", "file:///a.html"));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "superhtml".into(),
            path: PathBuf::from("/p/superhtml"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "superhtml",
            &LanguageId::new("html"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let factory = HtmlLanguageFactory::with_index(Arc::new(SharedIndex::new(IndexService::new())))
            .with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 2);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.html"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
    }
}
