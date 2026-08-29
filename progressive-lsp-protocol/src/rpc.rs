//! JSON-RPC 2.0 request/response helpers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn default_jsonrpc() -> String {
    "2.0".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    pub fn parse_error(detail: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: format!("Parse error: {}", detail.into()),
        }
    }

    pub fn invalid_request(detail: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: format!("Invalid Request: {}", detail.into()),
        }
    }

    pub fn method_not_found(method: impl AsRef<str>) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method.as_ref()),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "code": self.code, "message": self.message })
    }
}

pub fn parse_request(bytes: &[u8]) -> Result<JsonRpcRequest, JsonRpcError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| JsonRpcError::parse_error(e.to_string()))?;
    if !value.is_object() {
        return Err(JsonRpcError::invalid_request("not an object"));
    }
    let req: JsonRpcRequest = serde_json::from_value(value)
        .map_err(|e| JsonRpcError::invalid_request(e.to_string()))?;
    if req.method.is_empty() {
        return Err(JsonRpcError::invalid_request("missing method"));
    }
    Ok(req)
}

pub fn success(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

pub fn failure(id: Option<Value>, error: JsonRpcError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error.to_value(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_round_trip() {
        let raw = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"x":true}}"#;
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(json!(1)));
        assert_eq!(req.params["x"], true);
    }

    #[test]
    fn parse_notification_without_id() {
        let req = parse_request(br#"{"method":"exit"}"#).unwrap();
        assert_eq!(req.method, "exit");
        assert_eq!(req.id, None);
        assert_eq!(req.jsonrpc, "2.0");
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse_request(b"[").unwrap_err().code, -32700);
        assert_eq!(parse_request(b"[]").unwrap_err().code, -32600);
        assert_eq!(parse_request(br#"{}"#).unwrap_err().code, -32600);
        assert_eq!(
            parse_request(br#"{"method":""}"#).unwrap_err().code,
            -32600
        );
    }

    #[test]
    fn success_and_failure_shapes() {
        let ok = success(Some(json!(1)), json!(null));
        assert_eq!(ok["jsonrpc"], "2.0");
        assert_eq!(ok["id"], 1);
        assert!(ok["result"].is_null());
        let err = failure(None, JsonRpcError::method_not_found("x"));
        assert_eq!(err["error"]["code"], -32601);
        assert!(err["id"].is_null());
        assert!(JsonRpcError::invalid_request("z").message.contains("z"));
        assert!(JsonRpcError::parse_error("p").message.contains("p"));
    }
}
