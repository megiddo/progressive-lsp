//! LSP Facade: JSON-RPC in, domain out. FilesSince is not a `$/` method.

pub mod framing;
pub mod intelligence;
pub mod progress;
pub mod rpc;

use std::io::{BufRead, Write};
use std::sync::Arc;

use progressive_lsp_core::InitializeFailed;
use progressive_lsp_resolve::{QueryKind, ResolveQuery};
use serde_json::{json, Value};

use crate::framing::{read_message, write_message};
use crate::intelligence::{
    file_id_from_uri, position_from_params, result_to_lsp, uri_from_params, SEMANTIC_TOKEN_TYPES,
};
use crate::rpc::{JsonRpcError, JsonRpcRequest};

pub use intelligence::LspIntelligence;
pub use progress::{WorkDoneProgress, PROGRESS_METHOD, WORK_DONE_CREATE};

pub const SERVER_NAME: &str = "progressive-lsp";
pub const SERVER_VERSION: &str = "0.0.0";
pub const PROGRESSIVE_LSP_VERSION: &str = "v1";

/// Advertised `experimental.progressiveLsp` capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressiveLspCap {
    pub version: String,
    pub socket: Option<String>,
    pub mux: bool,
}

impl ProgressiveLspCap {
    pub fn new(socket: Option<String>, mux: bool) -> Self {
        Self {
            version: PROGRESSIVE_LSP_VERSION.to_string(),
            socket,
            mux,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": self.version,
            "socket": self.socket,
            "mux": self.mux,
        })
    }
}

/// JSON-RPC facade over stdio (or any reader/writer).
#[derive(Clone)]
pub struct LspFacade {
    cap: ProgressiveLspCap,
    intelligence: Option<Arc<dyn LspIntelligence>>,
}

impl std::fmt::Debug for LspFacade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspFacade")
            .field("cap", &self.cap)
            .field("has_intelligence", &self.intelligence.is_some())
            .finish()
    }
}

impl LspFacade {
    pub fn new(socket: Option<String>, mux: bool) -> Self {
        Self {
            cap: ProgressiveLspCap::new(socket, mux),
            intelligence: None,
        }
    }

    pub fn with_intelligence(mut self, intel: Arc<dyn LspIntelligence>) -> Self {
        self.intelligence = Some(intel);
        self
    }

    pub fn cap(&self) -> &ProgressiveLspCap {
        &self.cap
    }

    pub fn initialize_result(&self) -> Value {
        json!({
            "capabilities": {
                "textDocumentSync": 2,
                "definitionProvider": true,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "hoverProvider": true,
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": SEMANTIC_TOKEN_TYPES,
                        "tokenModifiers": []
                    },
                    "full": true
                },
                "workspace": { "workspaceFolders": { "supported": true } },
                "window": { "workDoneProgress": true },
                "experimental": {
                    "progressiveLsp": self.cap.to_json()
                }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        })
    }

    pub fn handle_request(&self, req: &JsonRpcRequest) -> Option<Value> {
        match req.method.as_str() {
            "initialize" => {
                if let Some(intel) = &self.intelligence {
                    if let Err(e) = intel.on_initialize(&req.params) {
                        return Some(rpc::failure(
                            req.id.clone(),
                            JsonRpcError {
                                code: -32002,
                                message: e.to_string(),
                            },
                        ));
                    }
                }
                Some(rpc::success(req.id.clone(), self.initialize_result()))
            }
            "shutdown" => Some(rpc::success(req.id.clone(), Value::Null)),
            "exit" | "initialized" => None,
            "textDocument/didOpen" => {
                if let Some(intel) = &self.intelligence {
                    let uri = uri_from_params(&req.params);
                    let text = req.params["textDocument"]["text"].as_str().unwrap_or("");
                    let lang = req.params["textDocument"]["languageId"]
                        .as_str()
                        .unwrap_or("");
                    intel.did_open(&uri, lang, text);
                }
                None
            }
            "textDocument/didChange" => {
                if let Some(intel) = &self.intelligence {
                    let uri = uri_from_params(&req.params);
                    let text = req.params["contentChanges"]
                        .as_array()
                        .and_then(|a| a.last())
                        .and_then(|c| c.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    intel.did_change(&uri, text);
                }
                None
            }
            "textDocument/didClose" => {
                if let Some(intel) = &self.intelligence {
                    intel.did_close(&uri_from_params(&req.params));
                }
                None
            }
            "textDocument/definition"
            | "textDocument/references"
            | "textDocument/hover"
            | "textDocument/documentSymbol"
            | "textDocument/typeDefinition"
            | "textDocument/implementation"
            | "workspace/symbol"
            | "textDocument/semanticTokens/full" => {
                Some(rpc::success(req.id.clone(), self.dispatch_intelligence(req)))
            }
            other if req.id.is_some() => Some(rpc::failure(
                req.id.clone(),
                JsonRpcError::method_not_found(other),
            )),
            _ => None,
        }
    }

    fn dispatch_intelligence(&self, req: &JsonRpcRequest) -> Value {
        let Some(intel) = &self.intelligence else {
            return if req.method == "textDocument/hover" {
                Value::Null
            } else if req.method == "textDocument/semanticTokens/full" {
                json!({ "data": [] })
            } else {
                json!([])
            };
        };
        if req.method == "textDocument/semanticTokens/full" {
            let uri = uri_from_params(&req.params);
            return json!({ "data": intel.semantic_tokens(&uri) });
        }
        let kind = match req.method.as_str() {
            "textDocument/definition" => QueryKind::Definition,
            "textDocument/references" => QueryKind::References,
            "textDocument/hover" => QueryKind::Hover,
            "textDocument/documentSymbol" => QueryKind::DocumentSymbol,
            "textDocument/typeDefinition" => QueryKind::TypeDefinition,
            "textDocument/implementation" => QueryKind::Implementation,
            "workspace/symbol" => QueryKind::WorkspaceSymbol,
            _ => QueryKind::Definition,
        };
        let uri = uri_from_params(&req.params);
        let q = if kind == QueryKind::WorkspaceSymbol {
            ResolveQuery::workspace_symbol(req.params["query"].as_str().unwrap_or(""))
        } else {
            ResolveQuery::new(
                file_id_from_uri(&uri),
                position_from_params(&req.params),
                kind,
            )
        };
        result_to_lsp(kind, &intel.resolve(&q))
    }

    fn write_json<W: Write>(writer: &mut W, value: &Value) -> Result<(), InitializeFailed> {
        let body = serde_json::to_vec(value).map_err(|e| InitializeFailed(e.to_string()))?;
        write_message(writer, body).map_err(|e| InitializeFailed(e.to_string()))
    }

    /// Serve until `exit` or EOF. Stock clients use stdio only.
    pub fn serve<R, W>(&self, mut reader: R, mut writer: W) -> Result<(), InitializeFailed>
    where
        R: BufRead,
        W: Write,
    {
        loop {
            let payload = match read_message(&mut reader) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Ok(()),
                Err(e) => return Err(InitializeFailed(e.to_string())),
            };
            let req = match rpc::parse_request(&payload) {
                Ok(req) => req,
                Err(e) => {
                    let resp = rpc::failure(None, e);
                    Self::write_json(&mut writer, &resp)?;
                    continue;
                }
            };
            if req.method == "exit" {
                return Ok(());
            }
            if let Some(resp) = self.handle_request(&req) {
                Self::write_json(&mut writer, &resp)?;
            }
            if let Some(intel) = &self.intelligence {
                for ev in intel.drain_progress() {
                    Self::write_json(&mut writer, &ev.to_notification())?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn rpc(id: u64, method: &str, params: Value) -> Vec<u8> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        framing::encode_message(serde_json::to_vec(&body).unwrap())
    }

    #[test]
    fn initialize_advertises_experimental_with_null_socket() {
        let facade = LspFacade::new(None, false);
        let cap = facade.initialize_result()["capabilities"]["experimental"]["progressiveLsp"]
            .clone();
        assert_eq!(cap["version"], "v1");
        assert!(cap["socket"].is_null());
        assert_eq!(cap["mux"], false);
        assert_eq!(facade.cap().socket, None);
        assert_eq!(
            facade.initialize_result()["capabilities"]["window"]["workDoneProgress"],
            true
        );
    }

    #[test]
    fn initialize_includes_socket_and_mux() {
        let facade = LspFacade::new(Some("/tmp/control.sock".into()), true);
        let cap = facade.cap().to_json();
        assert_eq!(cap["socket"], "/tmp/control.sock");
        assert_eq!(cap["mux"], true);
        assert_eq!(facade.initialize_result()["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn serve_initialize_shutdown_exit_round_trip() {
        let init = rpc(1, "initialize", json!({"capabilities": {}}));
        let initialized = framing::encode_message(
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }))
            .unwrap(),
        );
        let shutdown = rpc(2, "shutdown", Value::Null);
        let exit = framing::encode_message(
            serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "exit"}))
                .unwrap(),
        );
        let mut stdin = Vec::new();
        stdin.extend_from_slice(&init);
        stdin.extend_from_slice(&initialized);
        stdin.extend_from_slice(&shutdown);
        stdin.extend_from_slice(&exit);

        let facade = LspFacade::new(None, false);
        let mut out = Vec::new();
        facade
            .serve(Cursor::new(stdin), &mut out)
            .unwrap();

        let texts = framing::decode_all(&out).unwrap();
        assert_eq!(texts.len(), 2);
        let init_resp: Value = serde_json::from_slice(&texts[0]).unwrap();
        assert_eq!(init_resp["id"], 1);
        assert_eq!(
            init_resp["result"]["capabilities"]["experimental"]["progressiveLsp"]["version"],
            "v1"
        );
        let shut: Value = serde_json::from_slice(&texts[1]).unwrap();
        assert_eq!(shut["id"], 2);
        assert!(shut["result"].is_null());
    }

    #[test]
    fn unknown_method_with_id_is_method_not_found() {
        let facade = LspFacade::new(None, false);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "textDocument/codeAction".into(),
            params: Value::Null,
        };
        let resp = facade.handle_request(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("textDocument/codeAction"));
        let def = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(4)),
            method: "textDocument/definition".into(),
            params: json!({"textDocument":{"uri":"file:///a.java"},"position":{"line":0,"character":0}}),
        };
        let def_resp = facade.handle_request(&def).unwrap();
        assert!(def_resp["result"].is_array());
        let hover = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(5)),
            method: "textDocument/hover".into(),
            params: json!({}),
        };
        assert!(facade.handle_request(&hover).unwrap()["result"].is_null());
        let toks = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(6)),
            method: "textDocument/semanticTokens/full".into(),
            params: json!({}),
        };
        assert_eq!(
            facade.handle_request(&toks).unwrap()["result"]["data"],
            json!([])
        );
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didOpen".into(),
                params: json!({"textDocument":{"uri":"u","languageId":"java","text":"class A {}"}}),
            })
            .is_none());
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didChange".into(),
                params: json!({"textDocument":{"uri":"u"},"contentChanges":[{"text":"class B {}"}]}),
            })
            .is_none());
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didClose".into(),
                params: json!({"textDocument":{"uri":"u"}}),
            })
            .is_none());
        let _ = format!("{:?}", facade);
    }

    #[test]
    fn files_since_is_not_an_lsp_method() {
        let facade = LspFacade::new(None, false);
        for method in [
            "$/progressive/filesSince",
            "workspace/filesSince",
            "$/filesSince",
        ] {
            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: method.into(),
                params: Value::Null,
            };
            let resp = facade.handle_request(&req).unwrap();
            assert_eq!(resp["error"]["code"], -32601, "{method}");
        }
    }

    #[test]
    fn serve_eof_is_ok() {
        let facade = LspFacade::new(None, false);
        facade.serve(Cursor::new(Vec::new()), Vec::new()).unwrap();
    }

    #[test]
    fn serve_reports_bad_frame() {
        let facade = LspFacade::new(None, false);
        let err = facade
            .serve(Cursor::new(b"not-lsp"), Vec::new())
            .unwrap_err();
        assert!(
            err.0.contains("Content-Length")
                || err.0.contains("header")
                || err.0.contains("eof")
        );
    }

    #[test]
    fn serve_writes_parse_error_for_bad_json() {
        let facade = LspFacade::new(None, false);
        let framed = framing::encode_message(b"{");
        let mut out = Vec::new();
        facade.serve(Cursor::new(framed), &mut out).unwrap();
        let texts = framing::decode_all(&out).unwrap();
        assert_eq!(texts.len(), 1);
        let resp: Value = serde_json::from_slice(&texts[0]).unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }

    struct StubIntel;

    impl LspIntelligence for StubIntel {
        fn resolve(&self, q: &progressive_lsp_resolve::ResolveQuery) -> progressive_lsp_resolve::ResolveResult {
            use progressive_lsp_core::Tier;
            use progressive_lsp_resolve::{LspLocation, Range, ResolveResult};
            if q.kind == progressive_lsp_resolve::QueryKind::Hover {
                let mut r = ResolveResult::empty(Tier::Syntax);
                r.hover = Some(progressive_lsp_resolve::Hover {
                    name: "n".into(),
                    arity: Some(0),
                });
                return r;
            }
            ResolveResult::locations(
                Tier::Syntax,
                vec![LspLocation::new("file:///z", Range::default(), Tier::Syntax)],
            )
        }
        fn did_open(&self, _uri: &str, _language_id: &str, _text: &str) {}
        fn did_change(&self, _uri: &str, _text: &str) {}
        fn did_close(&self, _uri: &str) {}
        fn semantic_tokens(&self, _uri: &str) -> Vec<u32> {
            vec![0, 0, 1, 0, 0]
        }
    }

    #[test]
    fn intelligence_dispatches_methods() {
        let facade = LspFacade::new(None, false).with_intelligence(Arc::new(StubIntel));
        let def = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "textDocument/definition".into(),
            params: json!({"textDocument":{"uri":"file:///z"},"position":{"line":0,"character":0}}),
        };
        let resp = facade.handle_request(&def).unwrap();
        assert_eq!(resp["result"][0]["uri"], "file:///z");
        let hover = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "textDocument/hover".into(),
            params: json!({}),
        };
        assert_eq!(
            facade.handle_request(&hover).unwrap()["result"]["contents"]["value"],
            "n(0)"
        );
        let ws = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "workspace/symbol".into(),
            params: json!({"query": "z"}),
        };
        assert!(facade.handle_request(&ws).unwrap()["result"].is_array());
        let toks = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(4)),
            method: "textDocument/semanticTokens/full".into(),
            params: json!({"textDocument":{"uri":"file:///z"}}),
        };
        assert_eq!(
            facade.handle_request(&toks).unwrap()["result"]["data"][0],
            0
        );
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didOpen".into(),
                params: json!({"textDocument":{"uri":"u","languageId":"java","text":"x"}}),
            })
            .is_none());
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didChange".into(),
                params: json!({"textDocument":{"uri":"u"},"contentChanges":[{"text":"y"}]}),
            })
            .is_none());
        assert!(facade
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: None,
                method: "textDocument/didClose".into(),
                params: json!({"textDocument":{"uri":"u"}}),
            })
            .is_none());
        for method in [
            "textDocument/references",
            "textDocument/documentSymbol",
            "textDocument/typeDefinition",
            "textDocument/implementation",
        ] {
            let r = facade
                .handle_request(&JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(9)),
                    method: method.into(),
                    params: json!({"textDocument":{"uri":"file:///z"},"position":{"line":0,"character":0}}),
                })
                .unwrap();
            assert!(r.get("result").is_some(), "{method}");
        }
    }

    #[test]
    fn notification_without_id_is_ignored() {
        let facade = LspFacade::new(None, false);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "telemetry/event".into(),
            params: Value::Null,
        };
        assert!(facade.handle_request(&req).is_none());
    }
}
