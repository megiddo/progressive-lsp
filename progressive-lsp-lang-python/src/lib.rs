//! Python T1 (Tree-sitter) without pack. T3 via ty when EngineSupervisor is ready.
//! No CPython, pylsp, pyright, or ruff-as-types.

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
    LanguageId::new("python")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-python"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_python::LANGUAGE.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PythonIndexer;

impl LanguageIndexer for PythonIndexer {
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
pub struct PythonLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
}

impl PythonLanguageFactory {
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

impl Default for PythonLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for PythonLanguageFactory {
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
                ResolverChain::with_tiers(
                    t3,
                    None,
                    Box::new(TreeSitterResolver::new(g.clone())),
                )
            }
            None => ResolverChain::empty(),
        }
    }
}

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    match node.kind() {
        "function_definition" | "class_definition" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = if node.kind() == "class_definition" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Method
                };
                out.push(make(file, uri, &name, name_n, kind));
            }
        }
        "identifier" => {
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
        "class_definition" => Some(1u32),
        "function_definition" => Some(5),
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
    use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
    use progressive_lsp_engine::{
        AbortSpawnHooks, EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind,
    };
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};
    use progressive_lsp_workspace::{PyprojectAdapter, WorkspaceSource};
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
    fn python_t1_f12_without_pack() {
        assert_eq!(language_id().as_str(), "python");
        assert_eq!(grammar_id(), "tree-sitter-python");
        assert_eq!(PythonIndexer.language_id().as_str(), "python");
        assert_eq!(PythonIndexer.grammar_id(), "tree-sitter-python");
        assert_eq!(PythonLanguageFactory::new().language_id().as_str(), "python");
        assert_eq!(PythonLanguageFactory::new().grammar_id(), "tree-sitter-python");
        assert!(PythonLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"greet\"\n").unwrap();
        let greet = dir.path().join("greet.py");
        let main = dir.path().join("main.py");
        std::fs::write(&greet, "def greet(name):\n    return name\n").unwrap();
        std::fs::write(&main, "def run():\n    return greet(\"x\")\n").unwrap();
        assert_eq!(PyprojectAdapter.detect(dir.path()).unwrap().kind, "pyproject");
        let mut svc = IndexService::new();
        let job = PackageIngest::new("greet", "python")
            .with_file(&greet)
            .with_file(&main);
        svc.ingest_package(&job, &PythonIndexer);
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
        let greet_syms = PythonIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.py",
            &greet_src,
            &greet_tree,
        );
        assert!(greet_syms.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Method));
        let shared = SharedIndex::new(svc);
        let factory = PythonLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.resolver_chain().len(), 1);
        let pos = line_col(&src, "greet");
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("greet.py")),
                    "{:?}",
                    r.locations
                );
            }
            ResolveOutcome::NotReady => panic!("T1 must answer without ty pack"),
        }
    }

    #[test]
    fn python_t3_when_fake_ty_ready() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///greet.py"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "python",
            &LanguageId::new("python"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let index = SharedIndex::new(IndexService::new());
        let factory = PythonLanguageFactory::with_graph(Arc::new(index)).with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 2);
        let q = ResolveQuery::new(FileId::new("main.py"), Position::default(), QueryKind::Definition);
        match factory.resolver_chain().resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.iter().any(|l| l.uri.contains("greet.py")));
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.py"),
            Position::default(),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.hover.unwrap().signature(), "greet: int"),
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.py"),
            Position::default(),
            QueryKind::References,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.py"),
            Position::default(),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn on_engine_spawn_abort_keeps_python_t1() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix).with_hooks(Arc::new(AbortSpawnHooks {
            message: "skip-ty".into(),
        }));
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                PathBuf::from("/ws").as_path(),
            )
            .is_err());
        let dir = tempfile::tempdir().unwrap();
        let greet = dir.path().join("greet.py");
        std::fs::write(&greet, "def greet(name):\n    return name\n").unwrap();
        let mut svc = IndexService::new();
        svc.ingest_package(&PackageIngest::new("pkg", "python").with_file(&greet), &PythonIndexer);
        let shared = SharedIndex::new(svc);
        let factory =
            PythonLanguageFactory::with_graph(Arc::new(shared)).with_supervisor(Arc::new(sup));
        let src = "greet";
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(greet.to_string_lossy().as_ref()),
            line_col("def greet(name):\n    return name\n", src),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Syntax),
            other => panic!("{other:?}"),
        }
    }
}
