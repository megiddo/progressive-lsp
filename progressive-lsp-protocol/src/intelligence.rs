//! Domain queries out of the LSP facade. No watch internals.

use progressive_lsp_resolve::{
    DocumentSymbol, Hover, LspLocation, Position, QueryKind, ResolveQuery, ResolveResult,
};
use serde_json::{json, Value};

/// Implemented by the composition root session — not a god LspServer.
pub trait LspIntelligence: Send + Sync {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult;
    fn did_open(&self, uri: &str, language_id: &str, text: &str);
    fn did_change(&self, uri: &str, text: &str);
    fn did_close(&self, uri: &str);
    fn semantic_tokens(&self, uri: &str) -> Vec<u32>;
    fn drain_progress(&self) -> Vec<crate::progress::WorkDoneProgress> {
        Vec::new()
    }
    fn on_initialize(&self, _params: &serde_json::Value) -> Result<(), progressive_lsp_core::InitializeFailed> {
        Ok(())
    }
}

pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
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

pub fn location_to_json(loc: &LspLocation) -> Value {
    json!({
        "uri": loc.uri,
        "range": {
            "start": { "line": loc.range.start.line, "character": loc.range.start.character },
            "end": { "line": loc.range.end.line, "character": loc.range.end.character }
        },
        "data": { "tier": loc.tier.as_str() }
    })
}

pub fn hover_to_json(hover: &Hover) -> Value {
    json!({
        "contents": { "kind": "plaintext", "value": hover.signature() }
    })
}

pub fn symbol_to_json(sym: &DocumentSymbol) -> Value {
    json!({
        "name": sym.name,
        "kind": sym.kind.lsp_number(),
        "range": {
            "start": { "line": sym.range.start.line, "character": sym.range.start.character },
            "end": { "line": sym.range.end.line, "character": sym.range.end.character }
        },
        "selectionRange": {
            "start": { "line": sym.selection_range.start.line, "character": sym.selection_range.start.character },
            "end": { "line": sym.selection_range.end.line, "character": sym.selection_range.end.character }
        }
    })
}

pub fn result_to_lsp(kind: QueryKind, result: &ResolveResult) -> Value {
    match kind {
        QueryKind::Hover => match &result.hover {
            Some(h) => hover_to_json(h),
            None => Value::Null,
        },
        QueryKind::DocumentSymbol => {
            Value::Array(result.symbols.iter().map(symbol_to_json).collect())
        }
        QueryKind::WorkspaceSymbol => Value::Array(
            result
                .locations
                .iter()
                .map(|l| {
                    json!({
                        "name": l.uri.rsplit('/').next().unwrap_or(""),
                        "kind": 5,
                        "location": location_to_json(l)
                    })
                })
                .collect(),
        ),
        _ => Value::Array(result.locations.iter().map(location_to_json).collect()),
    }
}

pub fn uri_from_params(params: &Value) -> String {
    params
        .get("textDocument")
        .and_then(|t| t.get("uri"))
        .and_then(|u| u.as_str())
        .or_else(|| params.get("uri").and_then(|u| u.as_str()))
        .unwrap_or("")
        .to_string()
}

pub fn position_from_params(params: &Value) -> Position {
    let pos = params.get("position");
    Position::new(
        pos.and_then(|p| p.get("line")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        pos.and_then(|p| p.get("character"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    )
}

pub fn file_id_from_uri(uri: &str) -> progressive_lsp_core::FileId {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    progressive_lsp_core::FileId::new(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::Tier;
    use progressive_lsp_resolve::Range;

    #[test]
    fn json_helpers_cover_kinds() {
        let loc = LspLocation::new("file:///a", Range::default(), Tier::Syntax);
        assert_eq!(location_to_json(&loc)["data"]["tier"], "syntax");
        let h = Hover {
            name: "m".into(),
            arity: Some(1),
            type_info: None,
        };
        assert_eq!(hover_to_json(&h)["contents"]["value"], "m(1)");
        let sym = DocumentSymbol {
            name: "A".into(),
            kind: progressive_lsp_resolve::SymbolKind::Class,
            range: Range::default(),
            selection_range: Range::default(),
            arity: None,
            children: Vec::new(),
        };
        assert_eq!(symbol_to_json(&sym)["kind"], 5);
        let res = ResolveResult::locations(Tier::Syntax, vec![loc.clone()]);
        assert!(result_to_lsp(QueryKind::Definition, &res).is_array());
        let mut hover_res = ResolveResult::empty(Tier::Syntax);
        hover_res.hover = Some(h);
        assert!(result_to_lsp(QueryKind::Hover, &hover_res).is_object());
        assert!(result_to_lsp(QueryKind::Hover, &ResolveResult::empty(Tier::Syntax)).is_null());
        let mut doc = ResolveResult::empty(Tier::Syntax);
        doc.symbols.push(sym);
        assert!(result_to_lsp(QueryKind::DocumentSymbol, &doc).is_array());
        assert!(result_to_lsp(QueryKind::WorkspaceSymbol, &res).is_array());
        let params = json!({
            "textDocument": { "uri": "file:///x.java" },
            "position": { "line": 2, "character": 4 }
        });
        assert_eq!(uri_from_params(&params), "file:///x.java");
        assert_eq!(position_from_params(&params), Position::new(2, 4));
        assert_eq!(file_id_from_uri("file:///tmp/a").as_str(), "/tmp/a");
        assert_eq!(file_id_from_uri("/abs").as_str(), "/abs");
        assert_eq!(uri_from_params(&json!({"uri": "u"})), "u");
        assert_eq!(uri_from_params(&json!({})), "");
        assert_eq!(position_from_params(&json!({})), Position::default());
        assert!(!SEMANTIC_TOKEN_TYPES.is_empty());
    }
}
