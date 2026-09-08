//! Python T1 + heuristic T2 (Tree-sitter) without pack. T3 via ty when EngineSupervisor is ready.
//! Optional stack-graphs stay opt-in. No CPython, pylsp, pyright, or ruff-as-types.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId, T2Backend};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    CallSite, GraphFacts, GraphIndex, ImportDecl, IndexedSymbol, Position, Range, ResolverChain,
    SymbolKind, T2Strategy, TreeSitterResolver, TypeEdge,
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
        walk(
            tree.root_node(),
            source.as_bytes(),
            file,
            uri,
            None,
            &mut out,
        );
        out
    }
    fn extract_graph(&self, file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
        extract_graph_facts(file, source, tree)
    }
}

#[derive(Clone)]
pub struct PythonLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
    t2: T2Strategy,
}

impl PythonLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            supervisor: None,
            t2: T2Strategy::default_heuristic(),
        }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            graph: Some(graph),
            supervisor: None,
            t2: T2Strategy::default_heuristic(),
        }
    }
    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }
    pub fn with_t2(mut self, t2: T2Strategy) -> Self {
        self.t2 = t2;
        self
    }
    pub fn with_t2_backend(self, backend: T2Backend) -> Self {
        self.with_t2(T2Strategy::from_backend(backend))
    }
    pub fn t2_name(&self) -> &'static str {
        self.t2.backend_name()
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
                    Some(self.t2.build(g.clone())),
                    Box::new(TreeSitterResolver::new(g.clone())),
                )
            }
            None => ResolverChain::empty(),
        }
    }
}

fn walk(
    node: Node,
    src: &[u8],
    file: &FileId,
    uri: &str,
    container: Option<&str>,
    out: &mut Vec<IndexedSymbol>,
) {
    match node.kind() {
        "function_definition" | "class_definition" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = if node.kind() == "class_definition" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Method
                };
                let arity = if kind == SymbolKind::Method {
                    Some(py_arity(node))
                } else {
                    None
                };
                out.push(make(file, uri, &name, name_n, kind, arity, container));
                if kind == SymbolKind::Class {
                    let next = Some(name);
                    let mut c = node.walk();
                    for child in node.children(&mut c) {
                        walk(child, src, file, uri, next.as_deref(), out);
                    }
                    return;
                }
            }
        }
        "identifier" => {
            let name = node.utf8_text(src).unwrap_or("").to_string();
            if !name.is_empty() {
                out.push(make(
                    file,
                    uri,
                    &name,
                    node,
                    SymbolKind::Variable,
                    None,
                    container,
                ));
            }
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, src, file, uri, container, out);
    }
}

fn extract_graph_facts(file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
    let mut facts = GraphFacts::default();
    walk_graph(tree.root_node(), source.as_bytes(), file, &mut facts);
    facts
}

fn walk_graph(node: Node, src: &[u8], file: &FileId, facts: &mut GraphFacts) {
    match node.kind() {
        "import_statement" | "import_from_statement" => {
            let raw = node.utf8_text(src).unwrap_or("").to_string();
            let path = raw
                .trim()
                .trim_start_matches("from")
                .trim()
                .trim_start_matches("import")
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(',')
                .replace('/', ".");
            if !path.is_empty() && path != "*" {
                facts.imports.push(ImportDecl::new(file.clone(), path));
            }
        }
        "class_definition" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| n.utf8_text(src).unwrap_or("").to_string())
                .unwrap_or_default();
            if let Some(supers) = node.child_by_field_name("superclasses") {
                let mut c = supers.walk();
                for child in supers.children(&mut c) {
                    if child.kind() == "identifier" {
                        let parent = child.utf8_text(src).unwrap_or("").to_string();
                        if !parent.is_empty() && !name.is_empty() {
                            facts.edges.push(TypeEdge::new(name.clone(), parent));
                        }
                    }
                }
            }
        }
        "call" => {
            if let Some(fn_n) = node.child_by_field_name("function") {
                let name = fn_n.utf8_text(src).unwrap_or("").to_string();
                let simple = name.rsplit('.').next().unwrap_or(&name).to_string();
                if !simple.is_empty() {
                    let arity = node
                        .child_by_field_name("arguments")
                        .map(named_arg_count)
                        .unwrap_or(0);
                    let start = fn_n.start_position();
                    facts.calls.push(CallSite::new(
                        file.clone(),
                        simple,
                        arity,
                        start.row as u32,
                        start.column as u32,
                    ));
                }
            }
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk_graph(child, src, file, facts);
    }
}

fn py_arity(node: Node) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut n = 0u32;
    let mut c = params.walk();
    for child in params.children(&mut c) {
        if child.is_named() {
            n += 1;
        }
    }
    n
}

fn named_arg_count(args: Node) -> u32 {
    let mut n = 0u32;
    let mut c = args.walk();
    for child in args.children(&mut c) {
        if child.is_named() {
            n += 1;
        }
    }
    n
}

fn make(
    file: &FileId,
    uri: &str,
    name: &str,
    node: Node,
    kind: SymbolKind,
    arity: Option<u32>,
    container: Option<&str>,
) -> IndexedSymbol {
    let range = Range::new(
        Position::new(
            node.start_position().row as u32,
            node.start_position().column as u32,
        ),
        Position::new(
            node.end_position().row as u32,
            node.end_position().column as u32,
        ),
    );
    IndexedSymbol {
        file: file.clone(),
        uri: uri.to_string(),
        name: name.to_string(),
        kind,
        range,
        selection_range: range,
        arity,
        fqn: match container {
            Some(c) => format!("{c}.{name}"),
            None => name.to_string(),
        },
        container: container.map(str::to_string),
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
        let ds = if dl == 0 {
            start.saturating_sub(ps)
        } else {
            start
        };
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

    fn fixture_dir(name: &str, marker: &str) -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut candidates = vec![
            manifest.join(format!("../fixtures/{name}")),
            PathBuf::from(format!("fixtures/{name}")),
        ];
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(format!("fixtures/{name}")));
        }
        for c in &candidates {
            if c.join(marker).is_file() {
                return c.clone();
            }
        }
        panic!("missing fixtures/{name}/{marker}; tried {candidates:?}");
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

    #[test]
    fn python_t1_f12_without_pack() {
        assert_eq!(language_id().as_str(), "python");
        assert_eq!(grammar_id(), "tree-sitter-python");
        assert_eq!(PythonIndexer.language_id().as_str(), "python");
        assert_eq!(PythonIndexer.grammar_id(), "tree-sitter-python");
        assert_eq!(
            PythonLanguageFactory::new().language_id().as_str(),
            "python"
        );
        assert_eq!(
            PythonLanguageFactory::new().grammar_id(),
            "tree-sitter-python"
        );
        assert!(PythonLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"greet\"\n",
        )
        .unwrap();
        let greet = dir.path().join("greet.py");
        let main = dir.path().join("main.py");
        std::fs::write(&greet, "def greet(name):\n    return name\n").unwrap();
        std::fs::write(&main, "def run():\n    return greet(\"x\")\n").unwrap();
        assert_eq!(
            PyprojectAdapter.detect(dir.path()).unwrap().kind,
            "pyproject"
        );
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
        assert!(greet_syms
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Method));
        let shared = SharedIndex::new(svc);
        let factory = PythonLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.t2_name(), "heuristic");
        assert_eq!(factory.resolver_chain().len(), 2);
        let pos = line_col(&src, "greet");
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("greet.py")),
                    "{:?}",
                    r.locations
                );
            }
            ResolveOutcome::NotReady => panic!("T2 must answer without ty pack"),
        }
    }

    #[test]
    fn python_t3_when_fake_ty_ready() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture(
            "greet",
            "file:///greet.py",
        ));
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
        let factory =
            PythonLanguageFactory::with_graph(Arc::new(index)).with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 3);
        let q = ResolveQuery::new(
            FileId::new("main.py"),
            Position::default(),
            QueryKind::Definition,
        );
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
        svc.ingest_package(
            &PackageIngest::new("pkg", "python").with_file(&greet),
            &PythonIndexer,
        );
        let shared = SharedIndex::new(svc);
        let factory =
            PythonLanguageFactory::with_graph(Arc::new(shared)).with_supervisor(Arc::new(sup));
        let src = "greet";
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(greet.to_string_lossy().as_ref()),
            line_col("def greet(name):\n    return name\n", src),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Graph),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn python_tsg_stays_opt_in() {
        let g: Arc<dyn GraphIndex> = Arc::new(progressive_lsp_resolve::EmptyIndex);
        let f = PythonLanguageFactory::with_graph(g).with_t2_backend(T2Backend::StackGraphs);
        assert_eq!(f.t2_name(), "stack-graphs");
        assert_eq!(f.resolver_chain().len(), 2);
    }

    #[test]
    fn python_extract_graph_import_class_and_call() {
        let src = r#"
import math
from greet import greet

class Base:
    pass

class Lib(Base):
    def extra(self):
        return greet("x")

def greet(name):
    return name

def run():
    return greet("x")
"#;
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let file = FileId::new("t.py");
        let facts = PythonIndexer.extract_graph(&file, src, &tree);
        assert!(facts
            .imports
            .iter()
            .any(|i| i.path.contains("math") || i.path.contains("greet")));
        assert!(facts
            .edges
            .iter()
            .any(|e| e.child_fqn == "Lib" && e.parent_fqn == "Base"));
        assert!(facts.calls.iter().any(|c| c.name == "greet"));
        let syms = PythonIndexer.extract(&file, "file:///t.py", src, &tree);
        assert!(syms
            .iter()
            .any(|s| s.name == "Lib" && s.kind == SymbolKind::Class));
        assert!(syms
            .iter()
            .any(|s| s.name == "extra" && s.container.as_deref() == Some("Lib")));
        let _ = py_arity(tree.root_node());
        let _ = named_arg_count(tree.root_node());
    }

    #[test]
    fn python_heuristic_fixture_definition_and_references() {
        let root = fixture_dir("python-heuristic", "greet.py");
        let greet = root.join("greet.py");
        let app = root.join("app.py");
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("heuristic", "python")
                .with_file(&greet)
                .with_file(&app),
            &PythonIndexer,
        );
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let app_src = std::fs::read_to_string(&app).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let app_tree = p.parse(&app_src, None).unwrap();
        let facts = PythonIndexer.extract_graph(
            &FileId::new(app.to_string_lossy().as_ref()),
            &app_src,
            &app_tree,
        );
        assert!(
            facts.imports.iter().any(|i| i.path.contains("greet")),
            "{:?}",
            facts.imports
        );
        let greet_syms = PythonIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.py",
            &greet_src,
            &greet_tree,
        );
        assert!(greet_syms
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Method && s.arity == Some(1)));
        assert!(greet_syms
            .iter()
            .any(|s| s.name == "Lib" && s.kind == SymbolKind::Class));
        let factory = PythonLanguageFactory::with_graph(Arc::new(SharedIndex::new(svc)));
        let src = std::fs::read_to_string(&app).unwrap();
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(app.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert!(r.locations.iter().any(|l| l.uri.contains("greet.py")));
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(app.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::References,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert!(!r.locations.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }
}
