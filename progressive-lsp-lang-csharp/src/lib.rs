//! C# T1 (Tree-sitter) + T2 heuristics. T3 ceiling: no csharp-ls pack in v1.
//! Spike `spike/csharp-ls.md` produced no musl ELF on Darwin. No host `dotnet`.

use std::sync::Arc;

use progressive_lsp_core::{FileId, LanguageId};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphFacts, GraphIndex, HeuristicResolver, ImportDecl, IndexedSymbol, Position, Range,
    ResolverChain, SymbolKind, TreeSitterResolver,
};
use tree_sitter::{Node, Tree};

pub fn language_id() -> LanguageId {
    LanguageId::new("csharp")
}
pub fn grammar_id() -> &'static str {
    "tree-sitter-c-sharp"
}
pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_c_sharp::language()
}

/// v1 ships T1/T2 only. csharp-ls AOT did not close as a static pack.
pub fn t3_ceiling_reason() -> &'static str {
    "csharp-ls AOT produced no musl ELF; C# T1/T2 ceiling in v1"
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CSharpIndexer;

impl LanguageIndexer for CSharpIndexer {
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
pub struct CSharpLanguageFactory {
    graph: Option<Arc<dyn GraphIndex>>,
}

impl CSharpLanguageFactory {
    pub fn new() -> Self {
        Self { graph: None }
    }
    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self { graph: Some(graph) }
    }
}

impl Default for CSharpLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for CSharpLanguageFactory {
    fn language_id(&self) -> LanguageId {
        language_id()
    }
    fn grammar_id(&self) -> &str {
        grammar_id()
    }
    fn resolver_chain(&self) -> ResolverChain {
        match &self.graph {
            Some(g) => ResolverChain::new(vec![
                Box::new(HeuristicResolver::new(g.clone())),
                Box::new(TreeSitterResolver::new(g.clone())),
            ]),
            None => ResolverChain::empty(),
        }
    }
}

fn walk(node: Node, src: &[u8], file: &FileId, uri: &str, out: &mut Vec<IndexedSymbol>) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "method_declaration"
        | "constructor_declaration" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = name_n.utf8_text(src).unwrap_or("").to_string();
                let kind = if node.kind().contains("class") || node.kind().contains("interface") {
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

fn walk_graph(node: Node, src: &[u8], file: &FileId, facts: &mut GraphFacts) {
    if node.kind() == "using_directive" {
        let text = node.utf8_text(src).unwrap_or("");
        let path = text
            .trim()
            .trim_start_matches("using")
            .trim()
            .trim_end_matches(';')
            .trim();
        if !path.is_empty() {
            facts.imports.push(ImportDecl::new(file.clone(), path.replace("::", ".")));
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
    let named = match node.kind() {
        "class_declaration" => node.child_by_field_name("name").map(|n| (n, 1u32)),
        "method_declaration" => node.child_by_field_name("name").map(|n| (n, 5u32)),
        "identifier" => Some((node, 6u32)),
        _ => None,
    };
    if let Some((n, t)) = named {
        if n.start_position().row == n.end_position().row {
            let text = n.utf8_text(src).unwrap_or("");
            if !text.is_empty() {
                out.push((
                    n.start_position().row as u32,
                    n.start_position().column as u32,
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
    use progressive_lsp_core::Tier;
    use progressive_lsp_index::{IndexService, PackageIngest, SharedIndex};
    use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};
    use progressive_lsp_workspace::{CsprojAdapter, WorkspaceSource};

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
    fn csharp_t1_t2_ceiling_no_t3_pack() {
        assert_eq!(language_id().as_str(), "csharp");
        assert_eq!(grammar_id(), "tree-sitter-c-sharp");
        assert!(t3_ceiling_reason().contains("T1/T2 ceiling"));
        assert_eq!(CSharpIndexer.language_id().as_str(), "csharp");
        assert_eq!(CSharpIndexer.grammar_id(), "tree-sitter-c-sharp");
        assert_eq!(CSharpLanguageFactory::new().language_id().as_str(), "csharp");
        assert_eq!(CSharpLanguageFactory::new().grammar_id(), "tree-sitter-c-sharp");
        assert!(CSharpLanguageFactory::default().resolver_chain().is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("App.csproj"), "<Project></Project>\n").unwrap();
        let greet = dir.path().join("Greeter.cs");
        let main = dir.path().join("Program.cs");
        std::fs::write(
            &greet,
            "namespace Lib;\npublic interface IHi { void Hi(); }\npublic class Greeter { public Greeter() {} public static string Hi() { return \"hi\"; } }\n",
        )
        .unwrap();
        std::fs::write(
            &main,
            "using Lib;\nclass Program { static void Main() { Greeter.Hi(); } }\n",
        )
        .unwrap();
        assert_eq!(CsprojAdapter.detect(dir.path()).unwrap().kind, "csproj");
        let mut svc = IndexService::new();
        svc.ingest_package(
            &PackageIngest::new("App", "csharp").with_file(&greet).with_file(&main),
            &CSharpIndexer,
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
        let greet_toks = tokens_from_tree(&greet_src, &greet_tree);
        let greet_types: Vec<u32> = greet_toks.chunks(5).map(|c| c[3]).collect();
        assert!(greet_types.contains(&1), "class_declaration tokens");
        assert!(greet_types.contains(&5), "method_declaration tokens");
        assert!(greet_types.contains(&6), "identifier tokens");
        let greet_syms = CSharpIndexer.extract(
            &FileId::new(greet.to_string_lossy().as_ref()),
            "file:///Greeter.cs",
            &greet_src,
            &greet_tree,
        );
        assert!(greet_syms.iter().any(|s| s.name == "Greeter" && s.kind == SymbolKind::Class));
        assert!(greet_syms.iter().any(|s| s.name == "IHi" && s.kind == SymbolKind::Class));
        assert!(greet_syms.iter().any(|s| s.name == "Hi" && s.kind == SymbolKind::Method));
        assert!(greet_syms.iter().any(|s| s.name == "Greeter" && s.kind == SymbolKind::Method));
        assert!(greet_syms.iter().all(|s| !s.name.is_empty() && s.range.start.line == s.range.end.line));
        let types: Vec<u32> = toks.chunks(5).map(|c| c[3]).collect();
        assert!(types.contains(&1) || types.contains(&5) || types.contains(&6));
        assert_eq!(
            encode(&[(0, 1, 4, 1), (0, 8, 5, 5)]),
            vec![0, 1, 4, 1, 0, 0, 7, 5, 5, 0]
        );
        assert_eq!(
            encode(&[(0, 1, 2, 1), (1, 0, 3, 6)]),
            vec![0, 1, 2, 1, 0, 1, 0, 3, 6, 0]
        );
        let facts = CSharpIndexer.extract_graph(&FileId::new(main.to_string_lossy().as_ref()), &src, &tree);
        assert!(!facts.imports.is_empty(), "using_directive must fill GraphFacts");
        assert!(
            facts.imports.iter().any(|i| i.path.contains("Lib")),
            "{:?}",
            facts.imports
        );
        assert!(greet_syms.iter().any(|s| s.name == "Greeter" && s.kind == SymbolKind::Variable));
        let shared = SharedIndex::new(svc);
        let factory = CSharpLanguageFactory::with_graph(Arc::new(shared));
        assert_eq!(factory.resolver_chain().len(), 2);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new(main.to_string_lossy().as_ref()),
            line_col(&src, "Greeter"),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert!(r.tier == Tier::Graph || r.tier == Tier::Syntax);
                assert!(
                    r.locations.iter().any(|l| l.uri.contains("Greeter.cs") || l.uri.contains("Program.cs")),
                    "{:?}",
                    r.locations
                );
            }
            ResolveOutcome::NotReady => panic!("C# T1/T2 must answer without csharp-ls"),
        }
    }
}
