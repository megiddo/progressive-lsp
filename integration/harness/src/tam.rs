//! IT-TAM suite YAML types (SEAMS-5). Runner wiring is TAM-2.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamSuite {
    pub session: Option<TamSession>,
    pub cases: Vec<TamCase>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamSession {
    #[serde(rename = "progressive_lsp")]
    pub progressive_lsp: Option<TamProgressiveLsp>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
    pub apis: Vec<TamApiCase>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamApiCase {
    pub method: String,
    #[serde(rename = "progressive_lsp")]
    pub progressive_lsp: Option<TamProgressiveLsp>,
    pub expect: Option<TamExpect>,
    pub trace: Option<TamTrace>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TamExpect {
    #[serde(rename = "progressive_meta")]
    pub progressive_meta: Option<TamExpectMeta>,
    pub timing: Option<TamExpectTiming>,
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
