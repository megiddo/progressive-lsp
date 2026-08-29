//! Go T1/T2 + gopls T3 when pack and project go.mod/go.work are present.
//! Do not bundle a Go SDK. Missing project toolchain → T2/T1.

use std::path::Path;
use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphIndex, HeuristicResolver, IndexedSymbol, Position, Range, ResolverChain, SymbolKind,
    TreeSitterResolver,
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

pub fn project_go_present(root: &Path) -> bool {
    root.join("go.mod").is_file() || root.join("go.work").is_file()
}

pub fn go_degrade_reason(has_pack: bool, has_project: bool) -> Option<&'static str> {
    match (has_pack, has_project) {
        (true, true) => None,
        (false, false) => Some("no gopls pack and no go.mod/go.work; T2/T1"),
        (false, true) => Some("no gopls pack; T2/T1"),
        (true, false) => Some("no project go.mod/go.work; T2/T1"),
    }
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
    supervisor: Option<Arc<EngineSupervisor>>,
    has_pack: bool,
    has_project: bool,
}
impl GoLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            supervisor: None,
            has_pack: false,
            has_project: false,
        }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            graph: Some(graph),
            supervisor: None,
            has_pack: false,
            has_project: false,
        }
    }
    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }
    pub fn with_pack(mut self, has: bool) -> Self {
        self.has_pack = has;
        self
    }
    pub fn with_project(mut self, has: bool) -> Self {
        self.has_project = has;
        self
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
            Some(g) => {
                let t3 = if self.has_pack && self.has_project {
                    self.supervisor.as_ref().map(|s| {
                        Box::new(EngineResolver::new(
                            s.clone(),
                            language_id(),
                            PackageId::new("pkg"),
                        )) as Box<dyn progressive_lsp_resolve::Resolver>
                    })
                } else {
                    None
                };
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

    #[test]
    fn go_t3_when_pack_and_project_else_t2() {
        use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
        use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind};
        use std::path::PathBuf;

        assert!(project_go_present(tempfile::tempdir().unwrap().path()) == false);
        let proj = tempfile::tempdir().unwrap();
        std::fs::write(proj.path().join("go.mod"), "module example.com/x\n").unwrap();
        assert!(project_go_present(proj.path()));
        assert!(go_degrade_reason(false, true).unwrap().contains("no gopls pack"));
        assert!(go_degrade_reason(true, false).is_some());
        assert!(go_degrade_reason(true, true).is_none());
        assert!(go_degrade_reason(false, false).is_some());

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::gopls();
        fake.set_answers(FakeEngineAdapter::typed_fixture("Hello", "file:///greet.go"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "gopls".into(),
            path: PathBuf::from("/p/gopls"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "gopls",
            &LanguageId::new("go"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let index = SharedIndex::new(IndexService::new());
        let t3 = GoLanguageFactory::with_graph(Arc::new(index.clone()))
            .with_supervisor(Arc::new(sup))
            .with_pack(true)
            .with_project(true);
        assert_eq!(t3.resolver_chain().len(), 3);
        match t3.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.go"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
        let degrade = GoLanguageFactory::with_graph(Arc::new(index)).with_pack(true).with_project(false);
        assert_eq!(degrade.resolver_chain().len(), 2);
    }
}
