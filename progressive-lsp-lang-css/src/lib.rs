//! CSS T1 + biome T3 when pack ready. T1 fallback. No Node CSS LS.
//! Darwin: biome musl-clean unknown; adapter + Fake tests, real ELF is Linux CI.

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
    LanguageId::new("css")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-css"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_css::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CssIndexer;

impl LanguageIndexer for CssIndexer {
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
pub struct CssLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
}
impl CssLanguageFactory {
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
impl Default for CssLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl LanguageFactory for CssLanguageFactory {
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

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    if matches!(
        node.kind(),
        "class_name" | "id_name" | "tag_name" | "property_name" | "id_selector" | "class_selector"
    ) {
        let raw = node.utf8_text(src).unwrap_or("").trim_start_matches(['#', '.']);
        if !raw.is_empty() {
            out.push(make(
                file,
                uri,
                raw,
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

fn collect(node: Node, src: &[u8], out: &mut Vec<(u32, u32, u32, u32)>) {
    let ty = match node.kind() {
        "class_name" | "id_name" | "tag_name" => Some(2u32),
        "property_name" => Some(8),
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

    fn parse(src: &str) -> Tree {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        p.parse(src, None).unwrap()
    }

    fn token_types(data: &[u32]) -> Vec<u32> {
        data.chunks(5).map(|c| c[3]).collect()
    }

    #[test]
    fn css_ids_factory_and_chain() {
        assert_eq!(language_id().as_str(), "css");
        assert_eq!(grammar_id(), "tree-sitter-css");
        assert_eq!(CssIndexer.language_id().as_str(), "css");
        assert_eq!(CssIndexer.grammar_id(), "tree-sitter-css");
        assert_eq!(CssLanguageFactory::new().language_id().as_str(), "css");
        assert_eq!(CssLanguageFactory::new().grammar_id(), "tree-sitter-css");
        assert!(CssLanguageFactory::default().resolver_chain().is_empty());
        let mut svc = IndexService::new();
        svc.index_text(std::path::Path::new("a.css"), "#x { color: red; }", &CssIndexer, false);
        let factory = CssLanguageFactory::with_index(Arc::new(SharedIndex::new(svc)));
        assert_eq!(factory.resolver_chain().len(), 1);
        assert!(!factory.resolver_chain().is_empty());
    }

    #[test]
    fn css_selectors_tokens_and_columns() {
        let src = "#main-box { color: red; } .foo_bar { margin: 0; }\n# { }";
        let tree = parse(src);
        let file = FileId::new("a.css");
        let syms = CssIndexer.extract(&file, "file:///a.css", src, &tree);
        assert!(syms.iter().any(|s| s.name == "main-box" || s.name == "#main-box"));
        assert!(syms.iter().any(|s| s.name == "foo_bar" || s.name == ".foo_bar"));
        assert!(syms.iter().any(|s| {
            s.range.end.character == s.range.start.character + s.name.len() as u32
        }));
        assert_eq!(
            encode(&[(0, 1, 4, 2), (0, 8, 5, 8)]),
            vec![0, 1, 4, 2, 0, 0, 7, 5, 8, 0]
        );
        assert_eq!(encode(&[(0, 1, 2, 2), (1, 0, 3, 8)]), vec![0, 1, 2, 2, 0, 1, 0, 3, 8, 0]);
        assert!(syms.iter().any(|s| s.name == "color"), "walk must emit property_name");
        assert!(syms.iter().all(|s| !s.name.is_empty()));
        let toks = tokens_from_tree(src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let types = token_types(&toks);
        assert!(types.contains(&2), "class/id/tag tokens");
        assert!(types.contains(&8), "property_name tokens");
        let mut svc = IndexService::new();
        svc.index_text(std::path::Path::new("a.css"), src, &CssIndexer, false);
        let factory = CssLanguageFactory::with_index(Arc::new(SharedIndex::new(svc)));
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.css"),
            Position::new(0, 1),
            QueryKind::DocumentSymbol,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(r.symbols.iter().any(|s| s.name == "main-box"));
                assert!(r.symbols.iter().any(|s| s.name == "foo_bar"));
            }
            ResolveOutcome::NotReady => panic!("ready"),
        }
    }

    #[test]
    fn css_t3_via_fake_biome() {
        use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
        use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter};
        use progressive_lsp_index::{IndexService, SharedIndex};
        use std::path::PathBuf;

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::biome().with_binary(EngineBinary {
            pack_name: "biome".into(),
            path: PathBuf::from("/p/biome"),
            sha256: [0; 32],
        });
        fake.set_answers(FakeEngineAdapter::typed_fixture("main-box", "file:///a.css"));
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "biome",
            &LanguageId::new("css"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let factory = CssLanguageFactory::with_index(Arc::new(SharedIndex::new(IndexService::new())))
            .with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 2);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("a.css"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
    }
}
