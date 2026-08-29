//! C T1 (Tree-sitter) without pack. T3 via clangd when EngineSupervisor is ready.
//! Fail closed: stub / DT_NEEDED packs never exec.

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
    LanguageId::new("c")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-c"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_c::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CIndexer;

impl LanguageIndexer for CIndexer {
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
pub struct CLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
}

impl CLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            supervisor: None,
        }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
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

impl Default for CLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for CLanguageFactory {
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
    match node.kind() {
        "function_definition" | "declaration" => {
            if let Some(name_n) = find_ident(node) {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                if !name.is_empty() {
                    let kind = if node.kind() == "function_definition" {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Variable
                    };
                    out.push(make(file, uri, &name, name_n, kind));
                }
            }
        }
        "identifier" | "type_identifier" => {
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

fn find_ident(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if let Some(n) = find_ident(child) {
            return Some(n);
        }
    }
    None
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
        "function_definition" => Some(5u32),
        "identifier" | "type_identifier" => Some(6),
        _ => None,
    };
    if let Some(t) = ty {
        if node.start_position().row == node.end_position().row {
            let text = node.utf8_text(src).unwrap_or("");
            if !text.is_empty() && text.len() < 64 {
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
    use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
    use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind};
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};
    use progressive_lsp_workspace::{CompileCommandsAdapter, WorkspaceSource};
    use std::path::PathBuf;

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

    #[test]
    fn c_t1_f12_without_pack() {
        assert_eq!(language_id().as_str(), "c");
        assert_eq!(grammar_id(), "tree-sitter-c");
        assert_eq!(CIndexer.language_id().as_str(), "c");
        assert_eq!(CIndexer.grammar_id(), "tree-sitter-c");
        assert_eq!(CLanguageFactory::new().language_id().as_str(), "c");
        assert_eq!(CLanguageFactory::new().grammar_id(), "tree-sitter-c");
        assert!(CLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let greet = dir.path().join("src/greet.c");
        let main = dir.path().join("src/main.c");
        std::fs::write(&greet, "int greet(void) { return 1; }\n").unwrap();
        std::fs::write(&main, "int greet(void);\nint run(void) { return greet(); }\n").unwrap();
        let json = format!(
            r#"[{{"directory":"{}","file":"src/greet.c"}},{{"directory":"{}","file":"src/main.c"}}]"#,
            dir.path().display(),
            dir.path().display()
        );
        std::fs::write(dir.path().join("compile_commands.json"), json).unwrap();
        assert_eq!(
            CompileCommandsAdapter.detect(dir.path()).unwrap().kind,
            "compile_commands"
        );
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("cc", "c").with_file(&greet).with_file(&main),
            &CIndexer,
        );
        let src = std::fs::read_to_string(&main).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(&src, None).unwrap();
        let toks = tokens_from_tree(&src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let greet_syms = CIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.c",
            &greet_src,
            &greet_tree,
        );
        let main_syms = CIndexer.extract(
            &FileId::new(main.to_string_lossy().as_ref()),
            "file:///main.c",
            &src,
            &tree,
        );
        assert!(main_syms.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Variable));
        assert!(greet_syms.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Method));
        assert!(greet_syms.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Variable));
        assert!(greet_syms.iter().all(|s| !s.name.is_empty()));
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&5), "function_definition tokens");
        assert!(types.contains(&6), "identifier tokens");
        assert_eq!(
            encode(&[(0, 1, 4, 5), (0, 8, 5, 6)]),
            vec![0, 1, 4, 5, 0, 0, 7, 5, 6, 0]
        );
        assert_eq!(
            encode(&[(0, 1, 2, 5), (1, 0, 3, 6)]),
            vec![0, 1, 2, 5, 0, 1, 0, 3, 6, 0]
        );
        let shared = SharedIndex::new(svc);
        let factory = CLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.resolver_chain().len(), 1);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("greet.c") || l.uri.contains("main.c")),
                    "{:?}",
                    r.locations
                );
            }
            ResolveOutcome::NotReady => panic!("T1 must answer without clangd pack"),
        }
    }

    #[test]
    fn c_t3_f12_and_find_implementation_via_fake_clangd() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::clangd();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///greet.c"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "clangd".into(),
            path: PathBuf::from("/p/clangd"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "clangd",
            &LanguageId::new("c"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let index = SharedIndex::new(IndexService::new());
        let factory = CLanguageFactory::with_graph(Arc::new(index)).with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 2);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.c"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.iter().any(|l| l.uri.contains("greet.c")));
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.c"),
            Position::default(),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
    }
}
