//! PHP T1/T2 + PHPantom T3. No host php. No intelephense.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId, PackageId};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphFacts, GraphIndex, HeuristicResolver, ImportDecl, IndexedSymbol, Position, Range,
    ResolverChain, SymbolKind, TreeSitterResolver, TypeEdge,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("php")
}

pub fn grammar_id() -> &'static str {
    "tree-sitter-php"
}

pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PhpIndexer;

impl LanguageIndexer for PhpIndexer {
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
    fn extract_graph(&self, file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
        extract_graph(file, source, tree)
    }
}

#[derive(Clone)]
pub struct PhpLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
    supervisor: Option<Arc<EngineSupervisor>>,
}

impl PhpLanguageFactory {
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

impl Default for PhpLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for PhpLanguageFactory {
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
                    Some(Box::new(HeuristicResolver::new(g.clone()))),
                    Box::new(TreeSitterResolver::new(g.clone())),
                )
            }
            None => ResolverChain::empty(),
        }
    }
}

fn extract_symbols(file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol> {
    let mut out = Vec::new();
    walk(tree.root_node(), source.as_bytes(), file, uri, None, &mut out);
    out
}

fn extract_graph(file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
    let mut facts = GraphFacts::default();
    walk_graph(tree.root_node(), source.as_bytes(), file, None, &mut facts);
    facts
}

fn ns_name(node: Node, src: &[u8]) -> String {
    node.child_by_field_name("name")
        .map(|n| text(n, src))
        .unwrap_or_default()
}

fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut c = node.walk();
    let mut found = None;
    for ch in node.children(&mut c) {
        if ch.kind() == kind {
            found = Some(ch);
            break;
        }
    }
    found
}

fn walk_named_children(
    node: Node,
    src: &[u8],
    file: &FileId,
    uri: &str,
    ns: Option<&str>,
    out: &mut Vec<IndexedSymbol>,
) {
    let mut current = ns.map(str::to_string);
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "namespace_definition" {
            let name = ns_name(child, src);
            if child.child_by_field_name("body").is_some() {
                walk(child, src, file, uri, Some(&name), out);
            } else {
                current = Some(name);
            }
        } else {
            walk(child, src, file, uri, current.as_deref(), out);
        }
    }
}

fn walk(
    node: Node,
    src: &[u8],
    file: &FileId,
    uri: &str,
    ns: Option<&str>,
    out: &mut Vec<IndexedSymbol>,
) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "trait_declaration" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = text(name_n, src);
                let fqn = match ns {
                    Some(n) if !n.is_empty() => format!("{n}\\{name}"),
                    _ => name.clone(),
                };
                let kind = if node.kind() == "interface_declaration" {
                    SymbolKind::Interface
                } else {
                    SymbolKind::Class
                };
                out.push(sym(file, uri, name, kind, node, name_n, None, fqn.clone(), ns.map(str::to_string)));
                walk_named_children(node, src, file, uri, Some(&fqn), out);
                return;
            }
        }
        "function_definition" | "method_declaration" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = text(name_n, src);
                let fqn = match ns {
                    Some(n) => format!("{n}\\{name}"),
                    None => name.clone(),
                };
                out.push(sym(
                    file,
                    uri,
                    name,
                    SymbolKind::Method,
                    node,
                    name_n,
                    Some(php_arity(node)),
                    fqn,
                    ns.map(str::to_string),
                ));
            }
        }
        "name" | "qualified_name" => {
            let name = text(node, src);
            let simple = name.rsplit('\\').next().unwrap_or(&name).to_string();
            if !simple.is_empty() {
                out.push(sym(
                    file,
                    uri,
                    simple,
                    SymbolKind::Variable,
                    node,
                    node,
                    None,
                    name,
                    ns.map(str::to_string),
                ));
            }
        }
        _ => {}
    }
    walk_named_children(node, src, file, uri, ns, out);
}

fn walk_graph_children(node: Node, src: &[u8], file: &FileId, ns: Option<&str>, facts: &mut GraphFacts) {
    let mut current = ns.map(str::to_string);
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "namespace_definition" {
            let name = ns_name(child, src);
            facts.package = Some(name.clone());
            if child.child_by_field_name("body").is_some() {
                walk_graph(child, src, file, Some(&name), facts);
            } else {
                current = Some(name);
            }
        } else {
            walk_graph(child, src, file, current.as_deref(), facts);
        }
    }
}

fn walk_graph(node: Node, src: &[u8], file: &FileId, ns: Option<&str>, facts: &mut GraphFacts) {
    match node.kind() {
        "namespace_use_declaration" | "use_declaration" | "namespace_use_clause" => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                if matches!(child.kind(), "qualified_name" | "namespace_name") {
                    let path = text(child, src);
                    if path.contains('\\') {
                        facts.imports.push(ImportDecl::new(file.clone(), path.replace('\\', ".")));
                    }
                }
            }
        }
        "class_declaration" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| text(n, src))
                .unwrap_or_default();
            let fqn = match ns {
                Some(n) => format!("{n}\\{name}"),
                None => name,
            };
            if let Some(base) = child_of_kind(node, "base_clause") {
                let parent = text(base, src)
                    .replace("extends", "")
                    .trim()
                    .to_string();
                if !parent.is_empty() {
                    facts.edges.push(TypeEdge::new(fqn, parent));
                }
            }
        }
        _ => {}
    }
    walk_graph_children(node, src, file, ns, facts);
}

fn php_arity(node: Node) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut n = 0u32;
    let mut c = params.walk();
    for child in params.children(&mut c) {
        if child.kind().contains("parameter") {
            n += 1;
        }
    }
    n
}

fn text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn node_range(node: Node) -> Range {
    Range::new(
        Position::new(node.start_position().row as u32, node.start_position().column as u32),
        Position::new(node.end_position().row as u32, node.end_position().column as u32),
    )
}

fn sym(
    file: &FileId,
    uri: &str,
    name: String,
    kind: SymbolKind,
    node: Node,
    name_node: Node,
    arity: Option<u32>,
    fqn: String,
    container: Option<String>,
) -> IndexedSymbol {
    IndexedSymbol {
        file: file.clone(),
        uri: uri.to_string(),
        name,
        kind,
        range: node_range(node),
        selection_range: node_range(name_node),
        arity,
        fqn,
        container,
    }
}

pub fn tokens_from_tree(source: &str, tree: &Tree) -> Vec<u32> {
    let mut toks = Vec::new();
    collect_tokens(tree.root_node(), source.as_bytes(), &mut toks);
    encode(&toks)
}

fn collect_tokens(node: Node, src: &[u8], out: &mut Vec<(u32, u32, u32, u32)>) {
    let ty = match node.kind() {
        "name" | "qualified_name" => Some(1u32),
        "class_declaration" => Some(2),
        "function_definition" | "method_declaration" => Some(5),
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
        collect_tokens(child, src, out);
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
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};
    use progressive_lsp_workspace::{ComposerAdapter, WorkspaceSource};

    fn parse(src: &str) -> Tree {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        p.parse(src, None).unwrap()
    }

    #[test]
    fn factory_and_extract() {
        let f = PhpLanguageFactory::new();
        assert_eq!(f.language_id().as_str(), "php");
        assert_eq!(f.grammar_id(), "tree-sitter-php");
        assert!(f.resolver_chain().is_empty());
        assert!(PhpLanguageFactory::default().resolver_chain().is_empty());
        assert_eq!(language_id().as_str(), "php");
        assert_eq!(grammar_id(), "tree-sitter-php");
        assert_eq!(PhpIndexer.language_id().as_str(), "php");
        assert_eq!(PhpIndexer.grammar_id(), "tree-sitter-php");
        let src = "<?php\nnamespace App;\nuse Lib\\Hello;\ninterface IFace {}\nclass Greeter extends Base { public function run($a, $b) { return Hello::hi(); } }\n";
        let tree = parse(src);
        let file = FileId::new("Greeter.php");
        let syms = PhpIndexer.extract(&file, "file:///Greeter.php", src, &tree);
        let greeter = syms
            .iter()
            .find(|s| s.name == "Greeter" && s.kind == SymbolKind::Class)
            .expect("class");
        assert_eq!(greeter.fqn, "App\\Greeter");
        assert_eq!(greeter.container.as_deref(), Some("App"));
        assert!(syms.iter().any(|s| s.name == "IFace" && s.kind == SymbolKind::Interface));
        let run = syms
            .iter()
            .find(|s| s.name == "run" && s.kind == SymbolKind::Method)
            .expect("method");
        assert_eq!(run.arity, Some(2));
        assert_eq!(run.fqn, "App\\Greeter\\run");
        let facts = PhpIndexer.extract_graph(&file, src, &tree);
        assert_eq!(facts.package.as_deref(), Some("App"));
        assert!(facts.imports.iter().any(|i| i.simple == "Hello" || i.path.contains("Hello")));
        assert!(
            facts
                .edges
                .iter()
                .any(|e| e.child_fqn.contains("Greeter") && e.parent_fqn.contains("Base")),
            "hierarchy {:?}",
            facts.edges
        );
        let toks = tokens_from_tree(src, &tree);
        assert!(!toks.is_empty());
        assert_eq!(toks.len() % 5, 0);
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&1), "name tokens {types:?}");
        assert!(types.contains(&2), "class tokens {types:?}");
        assert!(types.contains(&5), "method tokens {types:?}");
        assert_eq!(
            encode(&[(0, 1, 4, 1), (0, 8, 5, 2)]),
            vec![0, 1, 4, 1, 0, 0, 7, 5, 2, 0]
        );
        assert_eq!(encode(&[(0, 1, 2, 1), (1, 0, 3, 2)]), vec![0, 1, 2, 1, 0, 1, 0, 3, 2, 0]);
        let _ = tree_sitter_language();
        let braced = "<?php\nnamespace Braced { class Inner {} }\n";
        let braced_tree = parse(braced);
        let braced_syms = PhpIndexer.extract(
            &FileId::new("Inner.php"),
            "file:///Inner.php",
            braced,
            &braced_tree,
        );
        assert!(
            braced_syms
                .iter()
                .any(|s| s.name == "Inner" && s.fqn.contains("Braced")),
            "braced namespace {:?}",
            braced_syms.iter().map(|s| &s.fqn).collect::<Vec<_>>()
        );
        let braced_facts = PhpIndexer.extract_graph(&FileId::new("Inner.php"), braced, &braced_tree);
        assert_eq!(braced_facts.package.as_deref(), Some("Braced"));
        let mut svc = IndexService::new();
        svc.index_text(std::path::Path::new("Greeter.php"), src, &PhpIndexer, false);
        let factory = PhpLanguageFactory::with_graph(Arc::new(SharedIndex::new(svc)));
        assert_eq!(factory.resolver_chain().len(), 2);
    }

    #[test]
    fn composer_f12_across_namespaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/App/","Lib\\":"src/Lib/"}}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src/App")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/Lib")).unwrap();
        let greeter = dir.path().join("src/App/Greeter.php");
        let hello = dir.path().join("src/Lib/Hello.php");
        std::fs::write(
            &greeter,
            "<?php\nnamespace App;\nuse Lib\\Hello;\nclass Greeter { public function run() { return Hello::hi(); } }\n",
        )
        .unwrap();
        std::fs::write(
            &hello,
            "<?php\nnamespace Lib;\nclass Hello { public static function hi() { return \"hi\"; } }\n",
        )
        .unwrap();
        let model = ComposerAdapter.detect(dir.path()).unwrap();
        assert!(model.packages.len() >= 1);
        let mut svc = IndexService::new();
        for pkg in &model.packages {
            let files: Vec<_> = std::fs::read_dir(&pkg.root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("php"))
                .collect();
            let mut job = PackageIngest::new(pkg.id.as_str(), "php");
            for f in files {
                job = job.with_file(f);
            }
            svc.ingest_package(&job, &PhpIndexer);
        }
        let shared = SharedIndex::new(svc);
        let factory = PhpLanguageFactory::with_graph(Arc::new(shared));
        let src = std::fs::read_to_string(&greeter).unwrap();
        let pos = line_col(&src, "Hello");
        let q = ResolveQuery::new(
            FileId::new(greeter.to_string_lossy().as_ref()),
            pos,
            QueryKind::Definition,
        );
        match factory.resolver_chain().resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("Hello.php")),
                    "PHP F12 across Composer namespaces, got {:?}",
                    r.locations
                );
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

    #[test]
    fn php_t3_via_fake_phpantom_else_t2() {
        use progressive_lsp_core::{FakeClock, PrefixLayout, Tier};
        use progressive_lsp_engine::{EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind};
        use std::path::PathBuf;

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::phpantom();
        fake.set_answers(FakeEngineAdapter::typed_fixture("Hello", "file:///Hello.php"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "phpantom".into(),
            path: PathBuf::from("/p/phpantom"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "phpantom",
            &LanguageId::new("php"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let factory =
            PhpLanguageFactory::with_graph(Arc::new(SharedIndex::new(IndexService::new())))
                .with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 3);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("Greeter.php"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.iter().any(|l| l.uri.contains("Hello.php")));
            }
            other => panic!("{other:?}"),
        }
        let t2_only = PhpLanguageFactory::with_graph(Arc::new(SharedIndex::new(IndexService::new())));
        assert_eq!(t2_only.resolver_chain().len(), 2);
    }
}
