//! LSP Facade: JSON-RPC in, domain out. FilesSince is not a `$/` method.

pub mod framing;
pub mod intelligence;
pub mod mux;
pub mod progress;
pub mod rpc;

use std::io::{BufRead, Write};
use std::sync::Arc;

use progressive_lsp_core::{InitializeFailed, LogComponent, LogPort, LogScope, NullLog};
use progressive_lsp_resolve::{QueryKind, ResolveQuery};
use serde_json::{json, Value};

use crate::framing::{read_message, write_message};
use crate::intelligence::{
    file_id_from_uri, position_from_params, result_to_lsp, uri_from_params, SEMANTIC_TOKEN_TYPES,
};
use crate::rpc::{JsonRpcError, JsonRpcRequest};

pub use intelligence::LspIntelligence;
pub use mux::{
    decode_mux_frame, encode_mux_frame, read_mux_frame, write_mux_frame, MuxError, MuxFrame,
    CHANNEL_CONTROL, CHANNEL_LSP, MAX_MUX_PAYLOAD,
};
pub use progress::{WorkDoneProgress, PROGRESS_METHOD, WORK_DONE_CREATE};

pub const SERVER_NAME: &str = "progressive-lsp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROGRESSIVE_LSP_VERSION: &str = "v1";
pub const PROTO_PACKAGE: &str = "progressive.v1";

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
    log: Arc<dyn LogPort>,
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
            log: Arc::new(NullLog),
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    pub fn with_intelligence(mut self, intel: Arc<dyn LspIntelligence>) -> Self {
        self.intelligence = Some(intel);
        self
    }

    fn emit_protocol_warn(&self, message: &str) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("protocol")
                .component(LogComponent::protocol()),
        );
        self.log.warn(message);
    }

    fn emit_protocol_info(&self, message: &str) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("protocol")
                .component(LogComponent::protocol()),
        );
        self.log.info(message);
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
                        let _g = LogScope::enter(
                            LogScope::new()
                                .operation("initialize")
                                .component(LogComponent::protocol()),
                        );
                        self.log.warn(&format!("InitializeFailed: {e}"));
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
            "shutdown" => {
                let _g = LogScope::enter(
                    LogScope::new()
                        .operation("shutdown")
                        .component(LogComponent::protocol()),
                );
                self.log.debug("shutdown");
                Some(rpc::success(req.id.clone(), Value::Null))
            }
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
            | "textDocument/semanticTokens/full" => Some(rpc::success(
                req.id.clone(),
                self.dispatch_intelligence(req),
            )),
            other if req.id.is_some() => {
                self.emit_protocol_info(&format!("method not found: {other}"));
                Some(rpc::failure(
                    req.id.clone(),
                    JsonRpcError::method_not_found(other),
                ))
            }
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

    /// Serve until `exit` or EOF. Stock clients use stdio Content-Length only.
    pub fn serve<R, W>(&self, mut reader: R, mut writer: W) -> Result<(), InitializeFailed>
    where
        R: BufRead,
        W: Write,
    {
        if self.cap.mux {
            return self.serve_mux(
                &mut reader,
                &mut writer,
                None::<fn(&[u8]) -> Option<Vec<u8>>>,
            );
        }
        self.serve_lsp(&mut reader, &mut writer)
    }

    fn serve_lsp<R, W>(&self, reader: &mut R, writer: &mut W) -> Result<(), InitializeFailed>
    where
        R: BufRead,
        W: Write,
    {
        loop {
            let payload = match read_message(reader) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Ok(()),
                Err(e) => {
                    self.emit_protocol_warn(&format!("framing: {e}"));
                    return Err(InitializeFailed(e.to_string()));
                }
            };
            if self.dispatch_json_rpc(&payload, writer)? {
                return Ok(());
            }
        }
    }

    /// `--mux`: channel id + length + payload. LSP stays JSON-RPC; control is proto.
    pub fn serve_mux<R, W, C>(
        &self,
        reader: &mut R,
        writer: &mut W,
        mut on_control: Option<C>,
    ) -> Result<(), InitializeFailed>
    where
        R: std::io::Read,
        W: Write,
        C: FnMut(&[u8]) -> Option<Vec<u8>>,
    {
        loop {
            let frame = match crate::mux::read_mux_frame(reader) {
                Ok(Some(f)) => f,
                Ok(None) => return Ok(()),
                Err(e) => {
                    self.emit_protocol_warn(&format!("mux: {e}"));
                    return Err(InitializeFailed(e.to_string()));
                }
            };
            if frame.is_control() {
                if let Some(cb) = on_control.as_mut() {
                    if let Some(resp) = cb(&frame.payload) {
                        self.write_mux(writer, crate::mux::CHANNEL_CONTROL, &resp)?;
                    }
                }
                continue;
            }
            if !frame.is_lsp() {
                self.emit_protocol_warn(&format!("mux: unknown mux channel {}", frame.channel));
                return Err(InitializeFailed(format!(
                    "unknown mux channel {}",
                    frame.channel
                )));
            }
            if self.dispatch_json_rpc_mux(&frame.payload, writer)? {
                return Ok(());
            }
        }
    }

    fn dispatch_json_rpc<W: Write>(
        &self,
        payload: &[u8],
        writer: &mut W,
    ) -> Result<bool, InitializeFailed> {
        let req = match rpc::parse_request(payload) {
            Ok(req) => req,
            Err(e) => {
                self.emit_protocol_warn(&format!("JSON-RPC parse failed code={}", e.code));
                let resp = rpc::failure(None, e);
                Self::write_json(writer, &resp)?;
                return Ok(false);
            }
        };
        if req.method == "exit" {
            return Ok(true);
        }
        if let Some(resp) = self.handle_request(&req) {
            Self::write_json(writer, &resp)?;
        }
        if let Some(intel) = &self.intelligence {
            for ev in intel.drain_progress() {
                Self::write_json(writer, &ev.to_notification())?;
            }
        }
        Ok(false)
    }

    fn dispatch_json_rpc_mux<W: Write>(
        &self,
        payload: &[u8],
        writer: &mut W,
    ) -> Result<bool, InitializeFailed> {
        let req = match rpc::parse_request(payload) {
            Ok(req) => req,
            Err(e) => {
                self.emit_protocol_warn(&format!("JSON-RPC parse failed code={}", e.code));
                let resp = rpc::failure(None, e);
                let body =
                    serde_json::to_vec(&resp).map_err(|e| InitializeFailed(e.to_string()))?;
                self.write_mux(writer, crate::mux::CHANNEL_LSP, &body)?;
                return Ok(false);
            }
        };
        if req.method == "exit" {
            return Ok(true);
        }
        if let Some(resp) = self.handle_request(&req) {
            let body = serde_json::to_vec(&resp).map_err(|e| InitializeFailed(e.to_string()))?;
            self.write_mux(writer, crate::mux::CHANNEL_LSP, &body)?;
        }
        if let Some(intel) = &self.intelligence {
            for ev in intel.drain_progress() {
                let body = serde_json::to_vec(&ev.to_notification())
                    .map_err(|e| InitializeFailed(e.to_string()))?;
                self.write_mux(writer, crate::mux::CHANNEL_LSP, &body)?;
            }
        }
        Ok(false)
    }

    fn write_mux<W: Write>(
        &self,
        writer: &mut W,
        channel: u8,
        payload: &[u8],
    ) -> Result<(), InitializeFailed> {
        crate::mux::write_mux_frame(writer, channel, payload).map_err(|e| {
            self.emit_protocol_warn(&format!("mux: {e}"));
            InitializeFailed(e.to_string())
        })
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
        let cap =
            facade.initialize_result()["capabilities"]["experimental"]["progressiveLsp"].clone();
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
        assert_eq!(
            facade.initialize_result()["serverInfo"]["name"],
            SERVER_NAME
        );
        assert_eq!(
            facade.initialize_result()["serverInfo"]["version"],
            SERVER_VERSION
        );
        assert_eq!(SERVER_VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(PROTO_PACKAGE, "progressive.v1");
        assert_eq!(PROGRESSIVE_LSP_VERSION, "v1");
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
            serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "exit"})).unwrap(),
        );
        let mut stdin = Vec::new();
        stdin.extend_from_slice(&init);
        stdin.extend_from_slice(&initialized);
        stdin.extend_from_slice(&shutdown);
        stdin.extend_from_slice(&exit);

        let facade = LspFacade::new(None, false);
        let mut out = Vec::new();
        facade.serve(Cursor::new(stdin), &mut out).unwrap();

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
            err.0.contains("Content-Length") || err.0.contains("header") || err.0.contains("eof")
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
        fn resolve(
            &self,
            q: &progressive_lsp_resolve::ResolveQuery,
        ) -> progressive_lsp_resolve::ResolveResult {
            use progressive_lsp_core::Tier;
            use progressive_lsp_resolve::{LspLocation, Range, ResolveResult};
            if q.kind == progressive_lsp_resolve::QueryKind::Hover {
                let mut r = ResolveResult::empty(Tier::Syntax);
                r.hover = Some(progressive_lsp_resolve::Hover {
                    name: "n".into(),
                    arity: Some(0),
                    type_info: None,
                });
                return r;
            }
            ResolveResult::locations(
                Tier::Syntax,
                vec![LspLocation::new(
                    "file:///z",
                    Range::default(),
                    Tier::Syntax,
                )],
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

    #[test]
    fn serve_mux_initialize_is_jsonrpc_on_lsp_channel() {
        let init = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"capabilities": {}}
        }))
        .unwrap();
        let exit = serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"exit"})).unwrap();
        let mut stdin = crate::mux::encode_mux_frame(crate::mux::CHANNEL_LSP, &init).unwrap();
        stdin.extend_from_slice(
            &crate::mux::encode_mux_frame(crate::mux::CHANNEL_LSP, &exit).unwrap(),
        );
        let facade = LspFacade::new(None, true);
        assert!(facade.cap().mux);
        let mut out = Vec::new();
        facade.serve(Cursor::new(stdin), &mut out).unwrap();
        let (frame, _) = crate::mux::decode_mux_frame(&out).unwrap().unwrap();
        assert!(frame.is_lsp());
        let resp: Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(
            resp["result"]["capabilities"]["experimental"]["progressiveLsp"]["mux"],
            true
        );
        assert!(
            resp["result"]["capabilities"]["experimental"]["progressiveLsp"]["socket"].is_null()
        );
    }

    #[test]
    fn serve_mux_control_callback_and_bad_json() {
        let inner = b"\0\0\0\0";
        let mut stdin = crate::mux::encode_mux_frame(crate::mux::CHANNEL_CONTROL, inner).unwrap();
        stdin.extend_from_slice(
            &crate::mux::encode_mux_frame(crate::mux::CHANNEL_LSP, b"{").unwrap(),
        );
        stdin.extend_from_slice(
            &crate::mux::encode_mux_frame(
                crate::mux::CHANNEL_LSP,
                &serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"exit"})).unwrap(),
            )
            .unwrap(),
        );
        let facade = LspFacade::new(None, true);
        let mut out = Vec::new();
        let mut seen = false;
        facade
            .serve_mux(
                &mut Cursor::new(stdin),
                &mut out,
                Some(|p: &[u8]| {
                    seen = p == inner;
                    Some(b"ack".to_vec())
                }),
            )
            .unwrap();
        assert!(seen);
        let (ctrl, n) = crate::mux::decode_mux_frame(&out).unwrap().unwrap();
        assert!(ctrl.is_control());
        assert_eq!(ctrl.payload, b"ack");
        let (lsp, _) = crate::mux::decode_mux_frame(&out[n..]).unwrap().unwrap();
        let resp: Value = serde_json::from_slice(&lsp.payload).unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }

    #[test]
    fn stock_initialize_has_control_off() {
        let facade = LspFacade::new(None, false);
        let cap =
            facade.initialize_result()["capabilities"]["experimental"]["progressiveLsp"].clone();
        assert_eq!(cap["version"], "v1");
        assert!(cap["socket"].is_null());
        assert_eq!(cap["mux"], false);
    }

    const LEAK: &str = "LEAK_BODY_XYZ";

    fn assert_no_payload_leak(log: &progressive_lsp_core::FakeLog) {
        for r in log.records() {
            assert!(
                !r.message.contains(LEAK),
                "payload leaked in message: {}",
                r.message
            );
            if let Some(ex) = &r.extras {
                for v in ex.values() {
                    assert!(!v.contains(LEAK), "payload leaked in extras: {v}");
                }
            }
        }
    }

    fn protocol_rows(log: &progressive_lsp_core::FakeLog) -> Vec<progressive_lsp_core::LogRecord> {
        log.records()
            .into_iter()
            .filter(|r| r.operation.as_deref() == Some("protocol"))
            .collect()
    }

    #[test]
    fn parse_and_method_not_found_emit_protocol_without_payload() {
        let log = progressive_lsp_core::FakeLog::new();
        let facade = LspFacade::new(None, false).with_log(Arc::new(log.clone()));
        let secret = json!({ "text": LEAK, "body": LEAK });
        let framed = framing::encode_message(
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "params": secret,
            }))
            .unwrap(),
        );
        let mut out = Vec::new();
        facade.serve(Cursor::new(framed), &mut out).unwrap();
        let parse_rows = protocol_rows(&log);
        assert!(
            parse_rows
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.component.as_ref().map(|c| c.as_str()) == Some("protocol")
                    && r.message.contains("JSON-RPC parse failed")
                    && r.message.contains("-32600")),
            "{parse_rows:?}"
        );
        assert_no_payload_leak(&log);

        let log2 = progressive_lsp_core::FakeLog::new();
        let facade2 = LspFacade::new(None, false).with_log(Arc::new(log2.clone()));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "textDocument/codeAction".into(),
            params: json!({ "text": LEAK }),
        };
        let resp = facade2.handle_request(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        let rows = protocol_rows(&log2);
        assert!(
            rows.iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Info
                    && r.component.as_ref().map(|c| c.as_str()) == Some("protocol")
                    && r.message.contains("method not found")
                    && r.message.contains("textDocument/codeAction")),
            "{rows:?}"
        );
        assert_no_payload_leak(&log2);
    }

    #[test]
    fn framing_content_length_emits_protocol_warn() {
        let log = progressive_lsp_core::FakeLog::new();
        let facade = LspFacade::new(None, false).with_log(Arc::new(log.clone()));
        let err = facade
            .serve(Cursor::new(b"not-lsp"), Vec::new())
            .unwrap_err();
        assert!(
            err.0.contains("Content-Length") || err.0.contains("header") || err.0.contains("eof")
        );
        let rows = protocol_rows(&log);
        assert!(
            rows.iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("protocol")
                    && r.message.contains("framing")
                    && (r.message.contains("Content-Length") || r.message.contains("header"))),
            "{rows:?}"
        );

        let log2 = progressive_lsp_core::FakeLog::new();
        let facade2 = LspFacade::new(None, false).with_log(Arc::new(log2.clone()));
        let _ = facade2
            .serve(Cursor::new(b"Content-Length: xyz\r\n\r\n"), Vec::new())
            .unwrap_err();
        let rows2 = protocol_rows(&log2);
        assert!(
            rows2
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("Content-Length")),
            "{rows2:?}"
        );
    }

    #[test]
    fn mux_errors_emit_protocol_warn_without_payload() {
        let log = progressive_lsp_core::FakeLog::new();
        let facade = LspFacade::new(None, true).with_log(Arc::new(log.clone()));
        let err = facade
            .serve_mux(
                &mut Cursor::new(vec![9u8, 0, 0, 0, 0]),
                &mut Vec::new(),
                None::<fn(&[u8]) -> Option<Vec<u8>>>,
            )
            .unwrap_err();
        assert!(err.0.contains("unknown mux channel"));
        let rows = protocol_rows(&log);
        assert!(
            rows.iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.component.as_ref().map(|c| c.as_str()) == Some("protocol")
                    && r.message.contains("mux")
                    && r.message.contains("unknown mux channel")),
            "{rows:?}"
        );

        let log2 = progressive_lsp_core::FakeLog::new();
        let facade2 = LspFacade::new(None, true).with_log(Arc::new(log2.clone()));
        let too = crate::mux::MAX_MUX_PAYLOAD + 1;
        let mut hdr = vec![crate::mux::CHANNEL_LSP];
        hdr.extend_from_slice(&too.to_be_bytes());
        let err = facade2
            .serve_mux(
                &mut Cursor::new(hdr),
                &mut Vec::new(),
                None::<fn(&[u8]) -> Option<Vec<u8>>>,
            )
            .unwrap_err();
        assert!(err.0.contains("exceeds") || err.0.to_lowercase().contains("payload"));
        let rows2 = protocol_rows(&log2);
        assert!(
            rows2
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("mux")
                    && (r.message.contains("exceeds") || r.message.contains("payload"))),
            "{rows2:?}"
        );

        let log3 = progressive_lsp_core::FakeLog::new();
        let facade3 = LspFacade::new(None, true).with_log(Arc::new(log3.clone()));
        let mut short = vec![crate::mux::CHANNEL_LSP];
        short.extend_from_slice(&4u32.to_be_bytes());
        short.extend_from_slice(LEAK.as_bytes());
        short.truncate(6);
        let _ = facade3.serve_mux(
            &mut Cursor::new(short),
            &mut Vec::new(),
            None::<fn(&[u8]) -> Option<Vec<u8>>>,
        );
        assert_no_payload_leak(&log3);
        let rows3 = protocol_rows(&log3);
        assert!(
            rows3
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("mux")),
            "{rows3:?}"
        );

        let log4 = progressive_lsp_core::FakeLog::new();
        let facade4 = LspFacade::new(None, true).with_log(Arc::new(log4.clone()));
        let leak_json = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "params": { "text": LEAK }
        }))
        .unwrap();
        let mut stdin = crate::mux::encode_mux_frame(crate::mux::CHANNEL_LSP, &leak_json).unwrap();
        stdin.extend_from_slice(
            &crate::mux::encode_mux_frame(
                crate::mux::CHANNEL_LSP,
                &serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"exit"})).unwrap(),
            )
            .unwrap(),
        );
        let mut out = Vec::new();
        facade4
            .serve_mux(
                &mut Cursor::new(stdin),
                &mut out,
                None::<fn(&[u8]) -> Option<Vec<u8>>>,
            )
            .unwrap();
        let rows4 = protocol_rows(&log4);
        assert!(
            rows4
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.message.contains("JSON-RPC parse failed")),
            "{rows4:?}"
        );
        assert_no_payload_leak(&log4);
    }

    #[test]
    fn shutdown_debug_and_initialize_failed_warn_jsonrpc_32002() {
        let log = progressive_lsp_core::FakeLog::new();
        let facade = LspFacade::new(None, false).with_log(Arc::new(log.clone()));
        let shut = facade.handle_request(&JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "shutdown".into(),
            params: Value::Null,
        });
        assert!(shut.unwrap()["result"].is_null());
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Debug
                    && r.operation.as_deref() == Some("shutdown")
                    && r.component.as_ref().map(|c| c.as_str()) == Some("protocol")
                    && r.message.contains("shutdown")),
            "{:?}",
            log.records()
        );

        struct FailInit;
        impl LspIntelligence for FailInit {
            fn resolve(&self, _q: &ResolveQuery) -> progressive_lsp_resolve::ResolveResult {
                progressive_lsp_resolve::ResolveResult::empty(progressive_lsp_core::Tier::Syntax)
            }
            fn did_open(&self, _uri: &str, _language_id: &str, _text: &str) {}
            fn did_change(&self, _uri: &str, _text: &str) {}
            fn did_close(&self, _uri: &str) {}
            fn semantic_tokens(&self, _uri: &str) -> Vec<u32> {
                Vec::new()
            }
            fn on_initialize(
                &self,
                _params: &Value,
            ) -> Result<(), progressive_lsp_core::InitializeFailed> {
                Err(progressive_lsp_core::InitializeFailed("nope".into()))
            }
        }

        let log2 = progressive_lsp_core::FakeLog::new();
        let facade2 = LspFacade::new(None, false)
            .with_log(Arc::new(log2.clone()))
            .with_intelligence(Arc::new(FailInit));
        let resp = facade2
            .handle_request(&JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "initialize".into(),
                params: json!({}),
            })
            .unwrap();
        assert_eq!(resp["error"]["code"], -32002);
        assert!(
            log2.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("initialize")
                    && r.message.contains("InitializeFailed")),
            "{:?}",
            log2.records()
        );
    }
}
