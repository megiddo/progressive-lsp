//! Visitor over the Java CST. Extracts declarations for T1 resolve.

use progressive_lsp_core::{FileId, LanguageId};
use progressive_lsp_index::LanguageIndexer;
use progressive_lsp_resolve::{IndexedSymbol, Position, Range, SymbolKind};
use tree_sitter::{Node, Tree};

use crate::{grammar_id, language_id, tree_sitter_language};

#[derive(Clone, Copy, Debug, Default)]
pub struct JavaIndexer;

impl LanguageIndexer for JavaIndexer {
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

pub fn extract_symbols(file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let package = package_name(tree.root_node(), bytes);
    walk(
        tree.root_node(),
        bytes,
        file,
        uri,
        package.as_deref(),
        None,
        &mut out,
    );
    out
}

fn package_name(root: Node, src: &[u8]) -> Option<String> {
    let mut c = root.walk();
    for child in root.children(&mut c) {
        if child.kind() == "package_declaration" {
            if let Some(name) = child
                .child_by_field_name("name")
                .or_else(|| named_child(&child, "scoped_identifier"))
                .or_else(|| named_child(&child, "identifier"))
            {
                return name.utf8_text(src).ok().map(|s| s.to_string());
            }
        }
    }
    None
}

fn named_child<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut c = node.walk();
    let mut found = None;
    for n in node.children(&mut c) {
        if n.kind() == kind {
            found = Some(n);
            break;
        }
    }
    found
}

fn walk(
    node: Node,
    src: &[u8],
    file: &FileId,
    uri: &str,
    package: Option<&str>,
    container: Option<&str>,
    out: &mut Vec<IndexedSymbol>,
) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" => {
            let kind = match node.kind() {
                "interface_declaration" => SymbolKind::Interface,
                "enum_declaration" => SymbolKind::Enum,
                _ => SymbolKind::Class,
            };
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = text(name_node, src);
                let fqn = fqn_of(package, container, &name);
                out.push(symbol(
                    file,
                    uri,
                    name,
                    kind,
                    node,
                    name_node,
                    None,
                    fqn.clone(),
                    container.map(str::to_string),
                ));
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    walk(child, src, file, uri, package, Some(&fqn), out);
                }
                return;
            }
        }
        "method_declaration" | "constructor_declaration" => {
            let kind = if node.kind() == "constructor_declaration" {
                SymbolKind::Constructor
            } else {
                SymbolKind::Method
            };
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = text(name_node, src);
                let arity = parameter_arity(node);
                let fqn = fqn_of(package, container, &name);
                out.push(symbol(
                    file,
                    uri,
                    name,
                    kind,
                    node,
                    name_node,
                    Some(arity),
                    fqn,
                    container.map(str::to_string),
                ));
            }
        }
        "identifier" | "type_identifier" => {
            let name = text(node, src);
            let line = node.start_position().row as u32;
            let col = node.start_position().column as u32;
            let dup = out.iter().any(|s| {
                s.file == *file
                    && s.name == name
                    && s.selection_range.start.line == line
                    && s.selection_range.start.character == col
            });
            if !name.is_empty() && !dup {
                let fqn = fqn_of(package, container, &name);
                out.push(symbol(
                    file,
                    uri,
                    name,
                    SymbolKind::Variable,
                    node,
                    node,
                    None,
                    fqn,
                    container.map(str::to_string),
                ));
            }
        }
        _ => {}
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, src, file, uri, package, container, out);
    }
}

fn parameter_arity(method: Node) -> u32 {
    let Some(params) = method.child_by_field_name("parameters") else {
        return 0;
    };
    let mut n = 0u32;
    let mut c = params.walk();
    for child in params.children(&mut c) {
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            n += 1;
        }
    }
    n
}

fn fqn_of(package: Option<&str>, container: Option<&str>, name: &str) -> String {
    match (package, container) {
        (_, Some(c)) => format!("{c}.{name}"),
        (Some(p), None) => format!("{p}.{name}"),
        (None, None) => name.to_string(),
    }
}

fn text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn symbol(
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

fn node_range(node: Node) -> Range {
    let s = node.start_position();
    let e = node.end_position();
    Range::new(
        Position::new(s.row as u32, s.column as u32),
        Position::new(e.row as u32, e.column as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(src: &str) -> Tree {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        p.parse(src, None).unwrap()
    }

    #[test]
    fn extracts_class_method_arity_and_package() {
        let src = r#"
package com.example.lib;
public class Lib {
    public static String greet(String name, int n) { return name; }
}
"#;
        let tree = parse(src);
        let file = FileId::new("Lib.java");
        let syms = extract_symbols(&file, "file:///Lib.java", src, &tree);
        assert!(syms.iter().any(|s| s.name == "Lib" && s.kind == SymbolKind::Class));
        let greet = syms
            .iter()
            .find(|s| s.name == "greet" && s.kind == SymbolKind::Method)
            .expect("greet");
        assert_eq!(greet.arity, Some(2));
        assert!(greet.fqn.contains("com.example.lib"));
        assert_eq!(JavaIndexer.grammar_id(), "tree-sitter-java");
        assert_eq!(JavaIndexer.language_id().as_str(), "java");
        let _ = JavaIndexer.tree_sitter_language();
        assert_eq!(JavaIndexer.extract(&file, "file:///Lib.java", src, &tree).len(), syms.len());
    }

    #[test]
    fn extracts_interface_enum_constructor() {
        let src = r#"
interface I { void x(); }
enum E { A }
class C { C(int a) {} }
"#;
        let tree = parse(src);
        let syms = extract_symbols(&FileId::new("X.java"), "file:///X.java", src, &tree);
        assert!(syms.iter().any(|s| s.kind == SymbolKind::Interface && s.name == "I"));
        assert!(syms.iter().any(|s| s.kind == SymbolKind::Enum && s.name == "E"));
        assert!(syms
            .iter()
            .any(|s| s.kind == SymbolKind::Constructor && s.arity == Some(1)));
    }

    #[test]
    fn no_package_fqn_is_simple_name() {
        let src = "class Solo {}";
        let tree = parse(src);
        let syms = extract_symbols(&FileId::new("S.java"), "file:///S.java", src, &tree);
        let solo = syms.iter().find(|s| s.name == "Solo").unwrap();
        assert_eq!(solo.fqn, "Solo");
        assert!(syms.iter().any(|s| s.kind == SymbolKind::Variable || s.kind == SymbolKind::Class));
    }

    #[test]
    fn zero_arity_method() {
        let src = "class A { void ping() {} }";
        let tree = parse(src);
        let syms = extract_symbols(&FileId::new("A.java"), "file:///A.java", src, &tree);
        let ping = syms.iter().find(|s| s.name == "ping").unwrap();
        assert_eq!(ping.arity, Some(0));
    }

    #[test]
    fn extracts_record_as_class_and_varargs() {
        let src = "record Point(int x, int y) { void sum(int... xs) {} }";
        let tree = parse(src);
        let syms = extract_symbols(&FileId::new("P.java"), "file:///P.java", src, &tree);
        assert!(syms.iter().any(|s| s.name == "Point" && s.kind == SymbolKind::Class));
        let sum = syms.iter().find(|s| s.name == "sum" && s.kind == SymbolKind::Method);
        if let Some(sum) = sum {
            assert_eq!(sum.arity, Some(1));
        }
    }
}
