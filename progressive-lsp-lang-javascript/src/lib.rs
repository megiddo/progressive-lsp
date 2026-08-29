//! JavaScript/TypeScript T1 via tree-sitter-javascript.
//! T2: heuristic import/export Strategy (oxc_resolver/oxc_semantic not wired; see language-matrix).
//! T3: tsgo via EngineSupervisor. Never Node tsserver.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphFacts, GraphIndex, HeuristicResolver, ImportDecl, IndexedSymbol, Position, Range,
    ResolverChain, SymbolKind, TreeSitterResolver,
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
    fn extract_graph(&self, file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
        let mut facts = GraphFacts::default();
        walk_graph(tree.root_node(), source.as_bytes(), file, &mut facts);
        facts
    }
}

#[derive(Clone)]
pub struct JavaScriptLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    typescript: bool,
    supervisor: Option<Arc<EngineSupervisor>>,
}
impl JavaScriptLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            typescript: false,
            supervisor: None,
        }
    }
    pub fn typescript() -> Self {
        Self {
            graph: None,
            typescript: true,
            supervisor: None,
        }
    }
    pub fn with_index(graph: Arc<dyn GraphIndex>) -> Self {
        Self::with_graph(graph)
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            graph: Some(graph),
            typescript: false,
            supervisor: None,
        }
    }
    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }
    pub fn attach_graph(mut self, graph: Arc<dyn GraphIndex>) -> Self {
        self.graph = Some(graph);
        self
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
        match &self.graph {
            Some(g) => {
                let t3 = self.supervisor.as_ref().map(|s| {
                    Box::new(EngineResolver::new(
                        s.clone(),
                        self.language_id(),
                        PackageId::new("pkg"),
                    )) as Box<dyn progressive_lsp_resolve::Resolver>
                });
                ResolverChain::with_tiers(
                    t3,
                    Some(Box::new(HeuristicResolver::new(g.clone()))),
                    Box::new(TreeSitterResolver::new(g.clone())),
                )
            }
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

fn walk_graph(node: Node, src: &[u8], file: &FileId, facts: &mut GraphFacts) {
    if matches!(node.kind(), "import_statement" | "export_statement") {
        let text = node.utf8_text(src).unwrap_or("");
        for part in text.split(|c: char| c == '"' || c == '\'' || c == '`' || c.is_whitespace() || c == '{' || c == '}' || c == ',' || c == ';') {
            let p = part.trim();
            if p.is_empty() || matches!(p, "import" | "export" | "from" | "as" | "default" | "*" | "{" | "}") {
                continue;
            }
            if p.starts_with('.') || p.contains('/') {
                facts.imports.push(ImportDecl::new(file.clone(), p));
            } else if p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                facts.imports.push(ImportDecl::new(file.clone(), p));
            }
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk_graph(child, src, file, facts);
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
        assert_eq!(factory.resolver_chain().len(), 2);
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
        let facts = JavaScriptIndexer.extract_graph(&FileId::new("b.js"), "import { greet } from \"./greet.js\";\n", &p.parse("import { greet } from \"./greet.js\";\n", None).unwrap());
        assert!(!facts.imports.is_empty());
    }

    #[test]
    fn ts_t3_go_to_type_via_fake_tsgo() {
        use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
        use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind};
        use progressive_lsp_index::IndexService;
        use std::path::PathBuf;

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::tsgo();
        fake.set_answers(FakeEngineAdapter::typed_fixture("id", "file:///id.ts"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "tsgo".into(),
            path: PathBuf::from("/p/tsgo"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "tsgo",
            &LanguageId::new("typescript"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let factory = JavaScriptLanguageFactory::typescript()
            .attach_graph(Arc::new(SharedIndex::new(IndexService::new())))
            .with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 3);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("app.ts"),
            Position::default(),
            QueryKind::TypeDefinition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.iter().any(|l| l.uri.contains("id.ts")));
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("app.ts"),
            Position::default(),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.hover.unwrap().signature(), "id: int"),
            other => panic!("{other:?}"),
        }
    }
}
