//! SEAMS-6 smoke: extended definition result when emitResultMeta is on.

use std::sync::Arc;

use progressive_lsp_core::Tier;
use progressive_lsp_protocol::{
    progressive_lsp::ProgressiveLspSessionOptions,
    rpc::JsonRpcRequest,
    LspFacade, LspIntelligence, ResolveReport,
};
use progressive_lsp_resolve::{LspLocation, Range, ResolveQuery, ResolveResult};
use serde_json::json;

struct MetaIntel {
    session: ProgressiveLspSessionOptions,
}

impl LspIntelligence for MetaIntel {
    fn resolve_report(&self, _q: &ResolveQuery) -> ResolveReport {
        use progressive_lsp_protocol::progressive_lsp::{new_trace_id, ProgressiveResultMeta};
        let result = ResolveResult::locations(
            Tier::Graph,
            vec![LspLocation::new("file:///X.java", Range::default(), Tier::Graph)],
        );
        ResolveReport {
            result,
            meta: ProgressiveResultMeta {
                trace_id: new_trace_id(),
                tier: "graph".into(),
                backend_language: "java".into(),
                backend_version: "stub".into(),
                resolve_ms: Some(1),
                chain_steps: Some(2),
                cache_state: Some("miss".into()),
            },
        }
    }

    fn did_open(&self, _: &str, _: &str, _: &str) {}
    fn did_change(&self, _: &str, _: &str) {}
    fn did_close(&self, _: &str) {}
    fn semantic_tokens(&self, _: &str) -> Vec<u32> {
        Vec::new()
    }

    fn effective_emit_options(
        &self,
        per: &progressive_lsp_protocol::ProgressiveLspRequestOptions,
    ) -> (bool, bool) {
        (
            per.emit_result_meta
                .unwrap_or(self.session.emit_result_meta),
            per.emit_timing.unwrap_or(self.session.emit_timing),
        )
    }

    fn progressive_cap_extension(&self) -> Option<serde_json::Value> {
        Some(json!({ "extendedResults": true }))
    }
}

#[test]
fn definition_with_meta_wraps_value_and_meta() {
    let intel = MetaIntel {
        session: ProgressiveLspSessionOptions {
            emit_result_meta: true,
            emit_timing: true,
            ..Default::default()
        },
    };
    let facade = LspFacade::new(None, false).with_intelligence(Arc::new(intel));
    let req = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: "textDocument/definition".into(),
        params: json!({
            "textDocument": { "uri": "file:///X.java" },
            "position": { "line": 0, "character": 0 },
            "progressiveLsp": { "emitResultMeta": true, "emitTiming": true }
        }),
    };
    let result = facade.handle_request(&req).unwrap();
    assert!(result["result"]["value"].is_array());
    assert_eq!(result["result"]["progressiveMeta"]["tier"], "graph");
    assert!(result["result"]["progressiveMeta"]["traceId"].is_string());
}
