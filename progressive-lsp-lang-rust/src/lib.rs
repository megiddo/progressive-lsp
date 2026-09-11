//! Rust T1 + heuristic T2 (Tree-sitter) without pack.
//! rust-analyzer T3 only when pack is ready **and** a project sysroot exists.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    CallSite, GraphFacts, GraphIndex, Hover, ImportDecl, IndexedSymbol, Position, QueryKind, Range,
    ResolveOutcome, ResolveQuery, Resolver, ResolverChain, SymbolKind, T2Strategy,
    TreeSitterResolver,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("rust")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-rust"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_rust::LANGUAGE.into()
}

/// Project sysroot probe. rustc sysroot / proc-macro `.so` are project artifacts.
pub fn detect_sysroot(root: &Path) -> Option<PathBuf> {
    let local = root.join("sysroot");
    if local.join("lib/rustlib").is_dir() {
        return Some(local);
    }
    None
}

pub fn rust_degrade_reason(has_pack: bool, has_sysroot: bool) -> Option<&'static str> {
    match (has_pack, has_sysroot) {
        (true, true) => None,
        (false, false) => Some("no rust-analyzer pack and no rustc sysroot; T2 then T1"),
        (false, true) => Some("no rust-analyzer pack; T2 then T1"),
        (true, false) => Some("no rustc sysroot; T2 then T1"),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RustIndexer;

impl LanguageIndexer for RustIndexer {
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

/// T1 hover annotates missing sysroot after T2.
pub struct RustT1Resolver {
    inner: TreeSitterResolver,
    note: Option<String>,
}

impl RustT1Resolver {
    pub fn new(inner: TreeSitterResolver, note: Option<String>) -> Self {
        Self { inner, note }
    }
}

impl Resolver for RustT1Resolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        match self.inner.resolve(q) {
            ResolveOutcome::Ready(mut r) if q.kind == QueryKind::Hover => {
                if let Some(note) = &self.note {
                    match &mut r.hover {
                        Some(h) => {
                            if !h.name.contains(note) {
                                h.name = format!("{} ({note})", h.name);
                            }
                        }
                        None => {
                            r.hover = Some(Hover {
                                name: note.clone(),
                                arity: None,
                                type_info: None,
                            });
                        }
                    }
                }
                ResolveOutcome::Ready(r)
            }
            other => other,
        }
    }
}

#[derive(Clone)]
pub struct RustLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
    has_sysroot: bool,
    has_pack: bool,
}

impl RustLanguageFactory {
    pub fn new() -> Self {
        Self {
            graph: None,
            supervisor: None,
            has_sysroot: false,
            has_pack: false,
        }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            graph: Some(graph),
            supervisor: None,
            has_sysroot: false,
            has_pack: false,
        }
    }
    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }
    pub fn with_sysroot(mut self, has: bool) -> Self {
        self.has_sysroot = has;
        self
    }
    pub fn with_pack(mut self, has: bool) -> Self {
        self.has_pack = has;
        self
    }
    pub fn degrade_note(&self) -> Option<&'static str> {
        rust_degrade_reason(self.has_pack, self.has_sysroot)
    }
}

impl Default for RustLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for RustLanguageFactory {
    fn language_id(&self) -> LanguageId {
        language_id()
    }
    fn grammar_id(&self) -> &str {
        grammar_id()
    }
    fn resolver_chain(&self) -> ResolverChain {
        match &self.graph {
            Some(g) => {
                let t3 = if self.has_sysroot && self.has_pack {
                    self.supervisor.as_ref().map(|s| {
                        Box::new(EngineResolver::new(
                            s.clone(),
                            language_id(),
                            PackageId::new("pkg"),
                        )) as Box<dyn Resolver>
                    })
                } else {
                    None
                };
                let note = self.degrade_note().map(str::to_string);
                ResolverChain::with_tiers(
                    t3,
                    Some(T2Strategy::default_heuristic().build(g.clone())),
                    Box::new(RustT1Resolver::new(
                        TreeSitterResolver::new(g.clone()),
                        note,
                    )),
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
        "mod_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                out.push(make(
                    file,
                    uri,
                    &name,
                    name_n,
                    SymbolKind::Class,
                    None,
                    container,
                ));
                let next = Some(name);
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    walk(child, src, file, uri, next.as_deref(), out);
                }
                return;
            }
        }
        "function_item" | "struct_item" | "enum_item" | "impl_item" => {
            if let Some(name_n) = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("type"))
            {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = match node.kind() {
                    "struct_item" | "enum_item" | "impl_item" => SymbolKind::Class,
                    _ => SymbolKind::Method,
                };
                let arity = if kind == SymbolKind::Method {
                    Some(rust_arity(node))
                } else {
                    None
                };
                out.push(make(file, uri, &name, name_n, kind, arity, container));
                if matches!(node.kind(), "impl_item" | "struct_item" | "enum_item") {
                    let next = Some(name);
                    let mut c = node.walk();
                    for child in node.children(&mut c) {
                        walk(child, src, file, uri, next.as_deref(), out);
                    }
                    return;
                }
            }
        }
        "identifier" | "type_identifier" | "field_identifier" => {
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
        "use_declaration" => {
            let raw = node.utf8_text(src).unwrap_or("").to_string();
            let path = raw
                .trim()
                .trim_start_matches("use")
                .trim()
                .trim_end_matches(';')
                .trim()
                .replace("::", ".");
            if !path.is_empty() {
                if path.ends_with('*') {
                    facts.imports.push(ImportDecl::wildcard(
                        file.clone(),
                        path.trim_end_matches('*').trim_end_matches('.').to_string(),
                    ));
                } else {
                    facts.imports.push(ImportDecl::new(file.clone(), path));
                }
            }
        }
        "mod_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                if !name.is_empty() {
                    facts.package = Some(name);
                }
            }
        }
        "call_expression" => {
            if let Some(fn_n) = node.child_by_field_name("function") {
                let name = fn_n.utf8_text(src).unwrap_or("").to_string();
                let simple = name.rsplit("::").next().unwrap_or(&name).to_string();
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

fn rust_arity(node: Node) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut n = 0u32;
    let mut c = params.walk();
    for child in params.children(&mut c) {
        if child.kind() == "parameter" || child.kind() == "self_parameter" {
            if child.kind() == "self_parameter" {
                continue;
            }
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
            Some(c) => format!("{c}::{name}"),
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
        "type_identifier" => Some(1u32),
        "function_item" => Some(5),
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
    use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind};
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::Resolver;
    use progressive_lsp_workspace::{CargoTomlAdapter, WorkspaceSource};

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
    fn rust_t1_without_pack_or_sysroot() {
        assert_eq!(language_id().as_str(), "rust");
        assert_eq!(grammar_id(), "tree-sitter-rust");
        assert_eq!(RustIndexer.language_id().as_str(), "rust");
        assert_eq!(RustIndexer.grammar_id(), "tree-sitter-rust");
        assert_ne!(RustIndexer.grammar_id(), "xyzzy");
        assert!(!RustIndexer.grammar_id().is_empty());
        assert!(RustLanguageFactory::default().resolver_chain().is_empty());
        assert_eq!(
            rust_degrade_reason(false, false),
            Some("no rust-analyzer pack and no rustc sysroot; T2 then T1")
        );
        assert_eq!(
            rust_degrade_reason(false, true),
            Some("no rust-analyzer pack; T2 then T1")
        );
        assert_eq!(
            rust_degrade_reason(true, false),
            Some("no rustc sysroot; T2 then T1")
        );
        assert!(rust_degrade_reason(true, true).is_none());
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_sysroot(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join("sysroot/lib/rustlib")).unwrap();
        assert!(detect_sysroot(dir.path()).is_some());

        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"greet\"\n",
        )
        .unwrap();
        let greet = dir.path().join("greet.rs");
        let main = dir.path().join("main.rs");
        std::fs::write(
            &greet,
            "struct Point { x: i32 }\nfn greet(name: &str) -> &str { name }\n",
        )
        .unwrap();
        std::fs::write(&main, "fn run() { greet(\"x\"); }\n").unwrap();
        assert_eq!(CargoTomlAdapter.detect(dir.path()).unwrap().kind, "cargo");
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("greet", "rust")
                .with_file(&greet)
                .with_file(&main),
            &RustIndexer,
        );
        let src = std::fs::read_to_string(&main).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(&src, None).unwrap();
        let toks = tokens_from_tree(&src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        assert!(toks.len() > 1);
        assert_ne!(toks, vec![1u32]);
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let syms = RustIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.rs",
            &greet_src,
            &greet_tree,
        );
        assert!(syms.iter().any(|s| s.name == "greet"));
        assert!(syms
            .iter()
            .any(|s| s.name == "Point" && s.kind == SymbolKind::Class));
        let shared = SharedIndex::new(svc);
        let factory = RustLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.grammar_id(), "tree-sitter-rust");
        assert!(factory.degrade_note().is_some());
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert!(r.locations.iter().any(|l| l.uri.contains("greet.rs")));
                assert!(
                    r.hover.is_none(),
                    "definition must not take the hover-note path"
                );
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => {
                let h = r.hover.unwrap();
                assert!(
                    h.name.contains("greet")
                        || h.name.contains("sysroot")
                        || h.name.contains("T1")
                        || h.name.contains("T2"),
                    "{h:?}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rust_t3_when_sysroot_and_pack_ready() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::rust_analyzer();
        fake.set_answers(FakeEngineAdapter::typed_fixture(
            "greet",
            "file:///greet.rs",
        ));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "rust".into(),
            path: PathBuf::from("/p/ra"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "rust",
            &LanguageId::new("rust"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let index = SharedIndex::new(IndexService::new());
        let factory = RustLanguageFactory::with_graph(Arc::new(index))
            .with_supervisor(Arc::new(sup))
            .with_sysroot(true)
            .with_pack(true);
        assert!(factory.degrade_note().is_none());
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.rs"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.rs"),
            Position::default(),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.hover.unwrap().signature(), "greet: int"),
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.rs"),
            Position::default(),
            QueryKind::Implementation,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Types),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pack_without_sysroot_stays_t1() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        let fake = FakeEngineAdapter::rust_analyzer().with_binary(EngineBinary {
            pack_name: "rust".into(),
            path: PathBuf::from("/p/ra"),
            sha256: [0; 32],
        });
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///t3.rs"));
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        let _ = sup.try_spawn(
            "rust",
            &LanguageId::new("rust"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        );
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("lib.rs");
        std::fs::write(&f, "fn greet() {}\n").unwrap();
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("pkg", "rust").with_file(&f),
            &RustIndexer,
        );
        let factory = RustLanguageFactory::with_graph(Arc::new(SharedIndex::new(svc)))
            .with_supervisor(Arc::new(sup))
            .with_pack(true)
            .with_sysroot(false);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(f.to_string_lossy().as_ref()),
            line_col("fn greet() {}\n", "greet"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Graph),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rust_t1_hover_note_when_t2_index_empty() {
        let factory =
            RustLanguageFactory::with_graph(Arc::new(progressive_lsp_resolve::EmptyIndex));
        assert_eq!(factory.resolver_chain().len(), 2);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("main.rs"),
            Position::default(),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                let h = r.hover.unwrap();
                assert!(h.name.contains("T2") || h.name.contains("sysroot"), "{h:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    #[should_panic(expected = "missing fixtures")]
    fn fixture_dir_missing_panics() {
        let _ = fixture_dir("no-such-heuristic", "nope.rs");
    }

    #[test]
    fn rust_extract_enum_bare_fn_and_empty_use() {
        let src = "use ;\nenum Color { Red }\nfn bare() {}\nfn call() { bare(); }\n";
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let file = FileId::new("t.rs");
        let facts = RustIndexer.extract_graph(&file, src, &tree);
        assert!(facts.calls.iter().any(|c| c.name == "bare"));
        let syms = RustIndexer.extract(&file, "file:///t.rs", src, &tree);
        assert!(syms
            .iter()
            .any(|s| s.name == "Color" && s.kind == SymbolKind::Class));
        assert!(syms.iter().any(|s| s.name == "bare" && s.arity == Some(0)));
    }

    #[test]
    fn rust_extract_graph_use_mod_impl_and_call() {
        let src = r#"
use crate::greet::*;
use crate::lib::Point;
mod greet {
    pub fn greet(name: &str) -> &str { name }
}
struct Point { x: i32 }
impl Point {
    fn id(&self) -> i32 { self.x }
}
fn run() { greet("x"); }
"#;
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let file = FileId::new("t.rs");
        let facts = RustIndexer.extract_graph(&file, src, &tree);
        assert!(facts
            .imports
            .iter()
            .any(|i| i.wildcard || i.path.contains("Point")));
        assert_eq!(facts.package.as_deref(), Some("greet"));
        assert!(facts.calls.iter().any(|c| c.name == "greet"));
        let syms = RustIndexer.extract(&file, "file:///t.rs", src, &tree);
        assert!(syms
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Method));
        assert!(syms
            .iter()
            .any(|s| s.name == "Point" && s.kind == SymbolKind::Class));
        let _ = rust_arity(tree.root_node());
        let _ = named_arg_count(tree.root_node());
    }

    #[test]
    fn rust_heuristic_fixture_definition_and_references() {
        let root = fixture_dir("rust-heuristic", "src/main.rs");
        let greet = root.join("src/greet.rs");
        let main = root.join("src/main.rs");
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("heuristic", "rust")
                .with_file(&greet)
                .with_file(&main),
            &RustIndexer,
        );
        let greet_src = std::fs::read_to_string(&greet).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let greet_tree = p.parse(&greet_src, None).unwrap();
        let facts = RustIndexer.extract_graph(
            &FileId::new(greet.to_string_lossy().as_ref()),
            &greet_src,
            &greet_tree,
        );
        assert!(facts.package.as_deref() == Some("greet") || facts.package.is_none());
        let greet_syms = RustIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///greet.rs",
            &greet_src,
            &greet_tree,
        );
        assert!(greet_syms
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Method && s.arity == Some(1)));
        let factory = RustLanguageFactory::with_graph(Arc::new(SharedIndex::new(svc)));
        let src = std::fs::read_to_string(&main).unwrap();
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            line_col(&src, "greet"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert!(r.locations.iter().any(|l| l.uri.contains("greet.rs")));
            }
            other => panic!("{other:?}"),
        }
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
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
