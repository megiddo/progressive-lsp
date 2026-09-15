//! IT-TAM suite YAML types and JSON report (SEAMS-5 / TAM-2).

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamSuite {
    #[serde(default)]
    pub id: Option<String>,
    pub session: Option<TamSession>,
    pub server: Option<TamServer>,
    #[serde(default)]
    pub cases: Vec<TamCase>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamServer {
    pub docker_image: Option<String>,
    pub platform: Option<String>,
    #[serde(rename = "prefix_in_container")]
    pub prefix_in_container: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamSession {
    #[serde(rename = "progressive_lsp")]
    pub progressive_lsp: Option<TamProgressiveLsp>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct TamProgressiveLsp {
    #[serde(rename = "emitResultMeta")]
    pub emit_result_meta: Option<bool>,
    #[serde(rename = "emitTiming")]
    pub emit_timing: Option<bool>,
    #[serde(rename = "maxChainIter")]
    pub max_chain_iter: Option<u8>,
    #[serde(rename = "maxTier")]
    pub max_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamCase {
    #[serde(default)]
    pub id: Option<String>,
    pub file: Option<String>,
    pub anchor: Option<TamAnchor>,
    pub apis: Vec<TamApiCase>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamAnchor {
    pub find: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamApiCase {
    pub method: String,
    #[serde(rename = "progressive_lsp")]
    pub progressive_lsp: Option<TamProgressiveLsp>,
    pub expect: Option<TamExpect>,
    pub trace: Option<TamTrace>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamExpect {
    #[serde(rename = "progressive_meta")]
    pub progressive_meta: Option<TamExpectMeta>,
    pub timing: Option<TamExpectTiming>,
    #[serde(rename = "min_locations")]
    pub min_locations: Option<usize>,
    #[serde(rename = "non_empty")]
    pub non_empty: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamExpectMeta {
    pub tier_at_most: Option<String>,
    pub backend_language: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamExpectTiming {
    pub max_resolve_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamTrace {
    pub fetch: Option<bool>,
}

/// JSON report row for one API call (harness output).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct TamReportRow {
    pub method: String,
    pub trace_id: Option<String>,
    pub meta_tier: Option<String>,
    pub backend_version: Option<String>,
    pub resolve_ms: Option<u64>,
    pub trace_row_count: Option<u32>,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_rows: Option<serde_json::Value>,
}

pub fn load_suite(path: &Path) -> Result<TamSuite, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_yaml::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_junit4_fixture_shape() {
        let yaml = r#"
session:
  progressive_lsp:
    emitResultMeta: true
    emitTiming: true
cases:
  - apis:
      - method: textDocument/definition
        progressive_lsp:
          maxChainIter: 2
          maxTier: graph
        expect:
          progressive_meta:
            tier_at_most: graph
            backend_language: java
        trace:
          fetch: true
"#;
        let suite: TamSuite = serde_yaml::from_str(yaml).unwrap();
        assert!(suite.session.unwrap().progressive_lsp.unwrap().emit_result_meta.unwrap());
        assert_eq!(suite.cases[0].apis[0].method, "textDocument/definition");
    }
}
