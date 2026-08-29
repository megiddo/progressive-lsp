//! LSP Facade: JSON-RPC in, domain out. FilesSince is not a `$/` method.

pub mod framing;
pub mod rpc;

use std::io::{BufRead, Write};

use progressive_lsp_core::InitializeFailed;
use serde_json::{json, Value};

use crate::framing::{read_message, write_message};
use crate::rpc::{JsonRpcError, JsonRpcRequest};

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
#[derive(Clone, Debug)]
pub struct LspFacade {
    cap: ProgressiveLspCap,
}

impl LspFacade {
    pub fn new(socket: Option<String>, mux: bool) -> Self {
        Self {
            cap: ProgressiveLspCap::new(socket, mux),
        }
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
                        "tokenTypes": [],
                        "tokenModifiers": []
                    },
                    "full": true
                },
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
            "initialize" => Some(rpc::success(req.id.clone(), self.initialize_result())),
            "shutdown" => Some(rpc::success(req.id.clone(), Value::Null)),
            "exit" | "initialized" => None,
            other if req.id.is_some() => Some(rpc::failure(
                req.id.clone(),
                JsonRpcError::method_not_found(other),
            )),
            _ => None,
        }
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
            method: "textDocument/definition".into(),
            params: Value::Null,
        };
        let resp = facade.handle_request(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("textDocument/definition"));
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
