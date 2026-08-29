//! Semantic tokens from the Java Tree-sitter CST.

use tree_sitter::{Node, Tree};

pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "method",
    "variable",
    "parameter",
    "property",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensLegend {
    pub token_types: Vec<&'static str>,
    pub token_modifiers: Vec<&'static str>,
}

pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: Vec::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub token_type: u32,
}

impl SemanticToken {
    pub fn type_index(kind: &str) -> Option<u32> {
        TOKEN_TYPES.iter().position(|k| *k == kind).map(|i| i as u32)
    }
}

pub fn tokens_from_tree(source: &str, tree: &Tree) -> Vec<SemanticToken> {
    let mut out = Vec::new();
    collect(tree.root_node(), source.as_bytes(), &mut out);
    out.sort_by(|a, b| a.line.cmp(&b.line).then(a.start.cmp(&b.start)));
    out
}

/// LSP delta encoding: line, startChar, length, tokenType, tokenModifiers.
pub fn encode_lsp_data(tokens: &[SemanticToken]) -> Vec<u32> {
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for t in tokens {
        let d_line = t.line.saturating_sub(prev_line);
        let d_start = if d_line == 0 {
            t.start.saturating_sub(prev_start)
        } else {
            t.start
        };
        data.push(d_line);
        data.push(d_start);
        data.push(t.length);
        data.push(t.token_type);
        data.push(0);
        prev_line = t.line;
        prev_start = t.start;
    }
    data
}

fn collect(node: Node, src: &[u8], out: &mut Vec<SemanticToken>) {
    if let Some(ty) = map_kind(node.kind()) {
        if let Some(idx) = SemanticToken::type_index(ty) {
            let start = node.start_position();
            let end = node.end_position();
            if start.row == end.row {
                let text = node.utf8_text(src).unwrap_or("");
                if !text.is_empty() {
                    out.push(SemanticToken {
                        line: start.row as u32,
                        start: start.column as u32,
                        length: text.len() as u32,
                        token_type: idx,
                    });
                }
            }
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect(child, src, out);
    }
}

fn map_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "package_declaration" => Some("namespace"),
        "type_identifier" => Some("type"),
        "class_declaration" => Some("class"),
        "enum_declaration" => Some("enum"),
        "interface_declaration" => Some("interface"),
        "method_declaration" | "constructor_declaration" => Some("method"),
        "identifier" => Some("variable"),
        "formal_parameter" => Some("parameter"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter_language;
    use tree_sitter::Parser;

    #[test]
    fn tokens_and_legend_and_encoding() {
        let src = "package p;\nclass A { void m(int x) {} }\n";
        let mut p = Parser::new();
        p.set_language(&tree_sitter_language()).unwrap();
        let tree = p.parse(src, None).unwrap();
        let toks = tokens_from_tree(src, &tree);
        assert!(!toks.is_empty());
        assert!(toks.iter().any(|t| t.token_type == SemanticToken::type_index("class").unwrap()));
        assert!(toks.iter().any(|t| t.token_type == SemanticToken::type_index("variable").unwrap()));
        assert_eq!(map_kind("class_declaration"), Some("class"));
        assert_eq!(map_kind("identifier"), Some("variable"));
        let data = encode_lsp_data(&toks);
        assert_eq!(data.len() % 5, 0);
        assert_eq!(data.len() / 5, toks.len());
        assert_eq!(SemanticToken::type_index("nope"), None);
        assert_eq!(map_kind("type_identifier"), Some("type"));
        assert_eq!(map_kind("formal_parameter"), Some("parameter"));
        assert_eq!(map_kind("enum_declaration"), Some("enum"));
        assert_eq!(map_kind("interface_declaration"), Some("interface"));
        assert_eq!(map_kind("constructor_declaration"), Some("method"));
        assert_eq!(map_kind("package_declaration"), Some("namespace"));
        assert_eq!(map_kind("comment"), None);
        let empty = encode_lsp_data(&[]);
        assert!(empty.is_empty());
        let same_line = encode_lsp_data(&[
            SemanticToken { line: 3, start: 2, length: 1, token_type: 1 },
            SemanticToken { line: 3, start: 6, length: 2, token_type: 2 },
        ]);
        assert_eq!(same_line[0], 3);
        assert_eq!(same_line[1], 2);
        assert_eq!(same_line[5], 0);
        assert_eq!(same_line[6], 4);
        let legend = semantic_tokens_legend();
        assert!(legend.token_modifiers.is_empty());
    }
}
