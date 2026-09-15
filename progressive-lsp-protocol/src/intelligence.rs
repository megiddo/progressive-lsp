//! Domain queries out of the LSP facade. No watch internals.

use progressive_lsp_resolve::{
    ChainPolicy, DocumentSymbol, Hover, LspLocation, Position, QueryKind, ResolveQuery,
    ResolveResult,
};

use crate::progressive_lsp::{ProgressiveLspRequestOptions, ProgressiveResultMeta};
use serde_json::{json, Value};

/// Full resolve observation for optional extended LSP results (SEAMS-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveReport {
    pub result: ResolveResult,
    pub meta: ProgressiveResultMeta,
}

/// Implemented by the composition root session — not a god LspServer.
pub trait LspIntelligence: Send + Sync {
    fn resolve_report(&self, q: &ResolveQuery) -> ResolveReport;

    fn resolve(&self, q: &ResolveQuery) -> ResolveResult {
        self.resolve_report(q).result
    }
    fn did_open(&self, uri: &str, language_id: &str, text: &str);
    fn did_change(&self, uri: &str, text: &str);
    fn did_close(&self, uri: &str);
    fn semantic_tokens(&self, uri: &str) -> Vec<u32>;
    fn drain_progress(&self) -> Vec<crate::progress::WorkDoneProgress> {
        Vec::new()
    }
    fn on_initialize(
        &self,
        _params: &serde_json::Value,
    ) -> Result<(), progressive_lsp_core::InitializeFailed> {
        Ok(())
    }

    /// Merge session defaults, test env, and per-request `progressiveLsp` chain fields.
    fn effective_chain_policy(&self, per_request: ChainPolicy) -> ChainPolicy {
        let _ = per_request;
        ChainPolicy::default()
    }

    fn effective_emit_options(
        &self,
        _per_request: &ProgressiveLspRequestOptions,
    ) -> (bool, bool) {
        (false, false)
    }

    /// Extra fields merged into `experimental.progressiveLsp` on initialize.
    fn progressive_cap_extension(&self) -> Option<serde_json::Value> {
        None
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
        pos.and_then(|p| p.get("line"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        pos.and_then(|p| p.get("character"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    )
}

pub fn file_id_from_uri(uri: &str) -> progressive_lsp_core::FileId {
    progressive_lsp_core::FileId::from_uri(uri)
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
        assert_eq!(
            file_id_from_uri("file:///Users/me/My%20Drive/a.java").as_str(),
            "/Users/me/My Drive/a.java"
        );
        assert_eq!(
            file_id_from_uri("file:///Users/me/GoogleDrive-en.gannim%40gmail.com/a.java").as_str(),
            "/Users/me/GoogleDrive-en.gannim@gmail.com/a.java"
        );
        assert_eq!(uri_from_params(&json!({"uri": "u"})), "u");
        assert_eq!(uri_from_params(&json!({})), "");
        assert_eq!(position_from_params(&json!({})), Position::default());
        assert!(!SEMANTIC_TOKEN_TYPES.is_empty());
    }

    #[test]
    fn extended_wrap_preserves_value_shape() {
        use crate::progressive_lsp::{wrap_extended_result, ProgressiveResultMeta};
        let loc = LspLocation::new("file:///a", Range::default(), Tier::Graph);
        let res = ResolveResult::locations(Tier::Graph, vec![loc]);
        let bare = result_to_lsp(QueryKind::Definition, &res);
        let meta = ProgressiveResultMeta {
            trace_id: "t".into(),
            tier: "graph".into(),
            backend_language: "java".into(),
            backend_version: "tree-sitter".into(),
            resolve_ms: Some(3),
            chain_steps: Some(2),
            cache_state: Some("miss".into()),
        };
        let wrapped = wrap_extended_result(bare.clone(), &meta, true);
        assert_eq!(wrapped["value"], bare);
        assert_eq!(wrapped["progressiveMeta"]["traceId"], "t");
        assert_eq!(wrapped["progressiveMeta"]["timing"]["resolve_ms"], 3);
        let no_timing = wrap_extended_result(bare, &meta, false);
        assert!(no_timing["progressiveMeta"]["timing"].as_object().unwrap().is_empty());
    }
}
