//! Parse extended LSP results and evaluate TAM expectations.

use progressive_lsp_core::Tier;
use serde_json::Value;

use crate::tam::{TamExpect, TamReportRow, TamTrace};

pub const TRACE_ROW_CAP: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProgressiveMeta {
    pub trace_id: Option<String>,
    pub tier: Option<String>,
    pub backend_language: Option<String>,
    pub backend_version: Option<String>,
    pub resolve_ms: Option<u64>,
}

/// Parse `progressiveMeta` from an LSP JSON-RPC `result` (extended or bare).
pub fn parse_progressive_meta(lsp_result: &Value) -> ParsedProgressiveMeta {
    let Some(meta) = lsp_result.get("progressiveMeta") else {
        return ParsedProgressiveMeta {
            trace_id: None,
            tier: None,
            backend_language: None,
            backend_version: None,
            resolve_ms: None,
        };
    };
    ParsedProgressiveMeta {
        trace_id: meta
            .get("traceId")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        tier: meta.get("tier").and_then(|v| v.as_str()).map(str::to_string),
        backend_language: meta
            .get("backendLanguage")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        backend_version: meta
            .get("backendVersion")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        resolve_ms: meta
            .get("timing")
            .and_then(|t| t.get("resolve_ms"))
            .and_then(|v| v.as_u64()),
    }
}

pub fn lsp_result_value(lsp_result: &Value) -> &Value {
    if lsp_result.get("value").is_some() {
        &lsp_result["value"]
    } else {
        lsp_result
    }
}

pub fn tier_at_most(observed: &str, ceiling: &str) -> bool {
    let Some(obs) = Tier::parse(observed) else {
        return false;
    };
    let Some(cap) = Tier::parse(ceiling) else {
        return true;
    };
    obs.rank() <= cap.rank()
}

fn location_count(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.len(),
        Value::Null => 0,
        _ if v.is_object() => 1,
        _ => 0,
    }
}

pub fn evaluate_expect(
    method: &str,
    lsp_result: &Value,
    expect: Option<&TamExpect>,
) -> (String, Option<String>) {
    let Some(exp) = expect else {
        return ("pass".into(), None);
    };
    let meta = parse_progressive_meta(lsp_result);
    if let Some(ref pm) = exp.progressive_meta {
        if let Some(ref want_lang) = pm.backend_language {
            if meta.backend_language.as_deref() != Some(want_lang.as_str()) {
                return (
                    "fail".into(),
                    Some(format!(
                        "{method}: backend_language want {want_lang}, got {:?}",
                        meta.backend_language
                    )),
                );
            }
        }
        if let Some(ref ceiling) = pm.tier_at_most {
            let tier = meta.tier.as_deref().unwrap_or("");
            if !tier.is_empty() && !tier_at_most(tier, ceiling) {
                return (
                    "fail".into(),
                    Some(format!(
                        "{method}: tier {tier} above ceiling {ceiling}"
                    )),
                );
            }
        }
    }
    if let Some(ref timing) = exp.timing {
        if let Some(max_ms) = timing.max_resolve_ms {
            let ms = meta.resolve_ms.unwrap_or(0);
            if ms > max_ms {
                return (
                    "fail".into(),
                    Some(format!(
                        "{method}: resolve_ms {ms} > max {max_ms}"
                    )),
                );
            }
        }
    }
    let val = lsp_result_value(lsp_result);
    if let Some(min) = exp.min_locations {
        if location_count(val) < min {
            return (
                "fail".into(),
                Some(format!(
                    "{method}: locations {} < min {min}",
                    location_count(val)
                )),
            );
        }
    }
    if exp.non_empty == Some(true) && val.is_null() {
        return (
            "fail".into(),
            Some(format!("{method}: expected non_empty, got null")),
        );
    }
    ("pass".into(), None)
}

pub fn build_report_row(
    method: &str,
    lsp_result: &Value,
    expect: Option<&TamExpect>,
    _trace: Option<&TamTrace>,
    fetch_failed: Option<String>,
) -> TamReportRow {
    let (mut result, note) = evaluate_expect(method, lsp_result, expect);
    if let Some(err) = fetch_failed {
        if result == "pass" {
            result = "fail".into();
        }
        let _ = note;
        let meta = parse_progressive_meta(lsp_result);
        return TamReportRow {
            method: method.to_string(),
            trace_id: meta.trace_id,
            meta_tier: meta.tier,
            backend_version: meta.backend_version,
            resolve_ms: meta.resolve_ms,
            trace_row_count: None,
            result,
            notes: Some(format!("FetchTrace: {err}")),
            trace_rows: None,
        };
    }
    let meta = parse_progressive_meta(lsp_result);
    TamReportRow {
        method: method.to_string(),
        trace_id: meta.trace_id.clone(),
        meta_tier: meta.tier.clone(),
        backend_version: meta.backend_version.clone(),
        resolve_ms: meta.resolve_ms,
        trace_row_count: None,
        result,
        notes: note,
        trace_rows: None,
    }
}

pub fn should_fetch_trace(trace: Option<&TamTrace>, row_result: &str) -> bool {
    trace.and_then(|t| t.fetch).unwrap_or(false) || row_result == "fail"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tam::{TamExpectMeta, TamExpectTiming};
    use serde_json::json;

    #[test]
    fn parses_wrapped_definition_meta() {
        let result = json!({
            "value": [{"uri": "file:///X.java"}],
            "progressiveMeta": {
                "traceId": "tid-1",
                "tier": "graph",
                "backendLanguage": "java",
                "backendVersion": "stub",
                "timing": { "resolve_ms": 4 }
            }
        });
        let m = parse_progressive_meta(&result);
        assert_eq!(m.trace_id.as_deref(), Some("tid-1"));
        assert_eq!(m.tier.as_deref(), Some("graph"));
        assert_eq!(m.resolve_ms, Some(4));
    }

    #[test]
    fn tier_at_most_respects_order() {
        assert!(tier_at_most("syntax", "graph"));
        assert!(tier_at_most("graph", "graph"));
        assert!(!tier_at_most("types", "graph"));
    }

    #[test]
    fn evaluate_meta_and_timing() {
        let result = json!({
            "value": [{}],
            "progressiveMeta": {
                "traceId": "t",
                "tier": "graph",
                "backendLanguage": "java",
                "timing": { "resolve_ms": 10 }
            }
        });
        let expect = TamExpect {
            progressive_meta: Some(TamExpectMeta {
                tier_at_most: Some("graph".into()),
                backend_language: Some("java".into()),
            }),
            timing: Some(TamExpectTiming {
                max_resolve_ms: Some(50),
            }),
            min_locations: Some(1),
            non_empty: None,
        };
        let (r, _) = evaluate_expect("textDocument/definition", &result, Some(&expect));
        assert_eq!(r, "pass");
    }
}
