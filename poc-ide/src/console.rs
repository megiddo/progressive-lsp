//! `ProtocolConsole` Facade and `TranscriptEntry` DTO. Append-only.

use progressive_lsp_control::prost::Message;
use progressive_lsp_control::{
    files_since_request, Envelope, FilesSinceRequest, InstallPacksRequest, SetConfigRequest,
    METHOD_FILES_SINCE, METHOD_INSTALL_PACKS, METHOD_SET_CONFIG,
};
use serde_json::Value;

use crate::control::{ControlClient, ControlPush, CONTROL_UNARY_METHODS};
use crate::error::IdeError;
use crate::ports::{ControlTransport, LspTransport};

/// Stock JSON-RPC methods the console picker offers. Unknown methods still send.
pub const STOCK_LSP_METHODS: &[&str] = &[
    "textDocument/definition",
    "textDocument/implementation",
    "textDocument/references",
    "initialize",
    "shutdown",
];

/// Lsp vs Control vs error. Push iff [`TranscriptKind::ControlPush`] and `request_id == 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TranscriptKind {
    LspRequest,
    LspReply,
    LspError,
    ControlRequest,
    ControlReply,
    ControlPush,
    ControlError,
}

impl TranscriptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LspRequest => "lsp-request",
            Self::LspReply => "lsp-reply",
            Self::LspError => "lsp-error",
            Self::ControlRequest => "control-request",
            Self::ControlReply => "control-reply",
            Self::ControlPush => "control-push",
            Self::ControlError => "control-error",
        }
    }

    pub fn is_lsp(self) -> bool {
        matches!(self, Self::LspRequest | Self::LspReply | Self::LspError)
    }

    pub fn is_control(self) -> bool {
        matches!(
            self,
            Self::ControlRequest | Self::ControlReply | Self::ControlPush | Self::ControlError
        )
    }

    pub fn is_error(self) -> bool {
        matches!(self, Self::LspError | Self::ControlError)
    }

    pub fn is_push(self) -> bool {
        matches!(self, Self::ControlPush)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lsp-request" => Some(Self::LspRequest),
            "lsp-reply" => Some(Self::LspReply),
            "lsp-error" => Some(Self::LspError),
            "control-request" => Some(Self::ControlRequest),
            "control-reply" => Some(Self::ControlReply),
            "control-push" => Some(Self::ControlPush),
            "control-error" => Some(Self::ControlError),
            _ => None,
        }
    }
}

/// One append-only transcript row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptEntry {
    kind: TranscriptKind,
    method: String,
    request_id: u64,
    body: String,
}

impl TranscriptEntry {
    pub fn new(
        kind: TranscriptKind,
        method: impl Into<String>,
        request_id: u64,
        body: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            method: method.into(),
            request_id,
            body: body.into(),
        }
    }

    pub fn kind(&self) -> TranscriptKind {
        self.kind
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn is_push(&self) -> bool {
        self.kind.is_push() && self.request_id == 0
    }

    fn lsp_request(method: &str, params: &Value) -> Self {
        Self::new(TranscriptKind::LspRequest, method, 0, params.to_string())
    }

    fn lsp_reply(method: &str, result: &Value) -> Self {
        Self::new(TranscriptKind::LspReply, method, 0, result.to_string())
    }

    fn lsp_error(method: &str, err: &IdeError) -> Self {
        Self::new(TranscriptKind::LspError, method, 0, err.to_string())
    }

    fn control_request(method: &str, request_id: u64, body: &[u8]) -> Self {
        Self::new(
            TranscriptKind::ControlRequest,
            method,
            request_id,
            format!("{} bytes", body.len()),
        )
    }

    fn control_reply(env: &Envelope) -> Self {
        Self::new(
            TranscriptKind::ControlReply,
            env.method.clone(),
            env.request_id,
            format!("{} bytes", env.body.len()),
        )
    }

    fn control_error(method: &str, err: &IdeError) -> Self {
        Self::new(TranscriptKind::ControlError, method, 0, err.to_string())
    }

    fn control_push(push: &ControlPush) -> Self {
        let body = match push {
            ControlPush::WatchBatch(b) => {
                format!("generation={} overflow={}", b.generation, b.overflow)
            }
            ControlPush::TierReady(t) => format!("{} {}", t.package_id, t.tier),
        };
        Self::new(TranscriptKind::ControlPush, push.method(), 0, body)
    }
}

/// Append-only JSON-RPC + Envelope inspector. Send does not panic on server error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProtocolConsole {
    entries: Vec<TranscriptEntry>,
}

impl ProtocolConsole {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn encode_body(method: &str, text: &str) -> Vec<u8> {
        match method {
            METHOD_SET_CONFIG => SetConfigRequest {
                patch_toml: text.to_string(),
            }
            .encode_to_vec(),
            METHOD_INSTALL_PACKS => InstallPacksRequest {
                packs: text
                    .split_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            }
            .encode_to_vec(),
            METHOD_FILES_SINCE => {
                if let Ok(n) = text.parse::<u64>() {
                    FilesSinceRequest {
                        since: Some(files_since_request::Since::SinceGeneration(n)),
                    }
                    .encode_to_vec()
                } else {
                    FilesSinceRequest::default().encode_to_vec()
                }
            }
            _ => Vec::new(),
        }
    }

    pub fn send_lsp<T: LspTransport>(
        &mut self,
        transport: &mut T,
        method: &str,
        params: Value,
    ) -> Result<Value, IdeError> {
        self.entries
            .push(TranscriptEntry::lsp_request(method, &params));
        match transport.request(method, params) {
            Ok(v) => {
                self.entries.push(TranscriptEntry::lsp_reply(method, &v));
                Ok(v)
            }
            Err(e) => {
                self.entries.push(TranscriptEntry::lsp_error(method, &e));
                Err(e)
            }
        }
    }

    pub fn send_control<T: ControlTransport>(
        &mut self,
        client: &mut ControlClient<T>,
        method: &str,
        body: Vec<u8>,
    ) -> Result<Envelope, IdeError> {
        let id = client.next_request_id();
        self.entries
            .push(TranscriptEntry::control_request(method, id, &body));
        match client.invoke(method, body) {
            Ok(reply) => {
                self.entries.push(TranscriptEntry::control_reply(&reply));
                Ok(reply)
            }
            Err(e) => {
                self.entries
                    .push(TranscriptEntry::control_error(method, &e));
                Err(e)
            }
        }
    }

    pub fn record_control_unavailable(&mut self, err: &IdeError) {
        self.entries.push(TranscriptEntry::control_error("", err));
    }

    pub fn drain_pushes<T: ControlTransport>(
        &mut self,
        client: &mut ControlClient<T>,
    ) -> Result<usize, IdeError> {
        let pushes = client.poll_pushes()?;
        let n = pushes.len();
        for push in &pushes {
            self.entries.push(TranscriptEntry::control_push(push));
        }
        Ok(n)
    }

    pub fn stock_lsp_methods() -> &'static [&'static str] {
        STOCK_LSP_METHODS
    }

    pub fn control_unary_methods() -> &'static [&'static str] {
        CONTROL_UNARY_METHODS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeControl, FakeLsp};
    use progressive_lsp_control::{
        GetConfigRequest, GetConfigResponse, Status, TierReady, WatchBatch, METHOD_GET_CONFIG,
        METHOD_TIER_READY, METHOD_WATCH_BATCH,
    };
    use serde_json::json;

    fn location_json() -> Value {
        json!({
            "uri": "file:///ws/lib.rs",
            "range": {
                "start": { "line": 0, "character": 3 },
                "end": { "line": 0, "character": 4 }
            }
        })
    }

    #[test]
    fn transcript_kind_value_object_names() {
        assert_eq!(TranscriptKind::LspRequest.as_str(), "lsp-request");
        assert_eq!(TranscriptKind::LspReply.as_str(), "lsp-reply");
        assert_eq!(TranscriptKind::LspError.as_str(), "lsp-error");
        assert_eq!(TranscriptKind::ControlRequest.as_str(), "control-request");
        assert_eq!(TranscriptKind::ControlReply.as_str(), "control-reply");
        assert_eq!(TranscriptKind::ControlPush.as_str(), "control-push");
        assert_eq!(TranscriptKind::ControlError.as_str(), "control-error");
        assert!(TranscriptKind::LspRequest.is_lsp());
        assert!(!TranscriptKind::LspRequest.is_control());
        assert!(TranscriptKind::ControlPush.is_control());
        assert!(TranscriptKind::ControlPush.is_push());
        assert!(!TranscriptKind::ControlReply.is_push());
        assert!(TranscriptKind::LspError.is_error());
        assert!(TranscriptKind::ControlError.is_error());
        assert!(!TranscriptKind::LspReply.is_error());
        assert_eq!(
            TranscriptKind::parse("control-push"),
            Some(TranscriptKind::ControlPush)
        );
        assert_eq!(
            TranscriptKind::parse("lsp-request"),
            Some(TranscriptKind::LspRequest)
        );
        assert_eq!(
            TranscriptKind::parse("lsp-reply"),
            Some(TranscriptKind::LspReply)
        );
        assert_eq!(
            TranscriptKind::parse("lsp-error"),
            Some(TranscriptKind::LspError)
        );
        assert_eq!(
            TranscriptKind::parse("control-request"),
            Some(TranscriptKind::ControlRequest)
        );
        assert_eq!(
            TranscriptKind::parse("control-reply"),
            Some(TranscriptKind::ControlReply)
        );
        assert_eq!(
            TranscriptKind::parse("control-error"),
            Some(TranscriptKind::ControlError)
        );
        assert_eq!(TranscriptKind::parse("mux"), None);
        assert_eq!(TranscriptKind::parse(""), None);
        let entry = TranscriptEntry::new(TranscriptKind::ControlPush, "WatchBatch", 0, "x");
        assert!(entry.is_push());
        assert_eq!(entry.kind(), TranscriptKind::ControlPush);
        assert_eq!(entry.method(), "WatchBatch");
        assert_eq!(entry.request_id(), 0);
        assert_eq!(entry.body(), "x");
        let not_push = TranscriptEntry::new(TranscriptKind::ControlPush, "WatchBatch", 1, "x");
        assert!(!not_push.is_push());
    }

    #[test]
    fn protocol_console_facade_append_only_lsp_and_envelope() {
        let mut lsp = FakeLsp::new();
        lsp.script("textDocument/definition", location_json());
        let mut fake = FakeControl::new();
        fake.queue_push(Envelope::push(
            METHOD_WATCH_BATCH,
            WatchBatch {
                events: vec![],
                overflow: true,
                need_rescan: false,
                generation: 8,
            },
        ));
        fake.queue_push(Envelope::push(
            METHOD_TIER_READY,
            TierReady {
                package_id: "pkg".into(),
                tier: "graph".into(),
            },
        ));
        let mut client = ControlClient::new(fake);
        let mut console = ProtocolConsole::new();
        assert!(console.is_empty());
        assert_eq!(console.len(), 0);
        assert_eq!(ProtocolConsole::default(), ProtocolConsole::new());

        let def = console
            .send_lsp(
                &mut lsp,
                "textDocument/definition",
                json!({"textDocument": {"uri": "file:///ws/lib.rs"}}),
            )
            .unwrap();
        assert_eq!(def["uri"], "file:///ws/lib.rs");
        assert_eq!(console.len(), 2);
        assert_eq!(console.entries()[0].kind(), TranscriptKind::LspRequest);
        assert_eq!(console.entries()[0].method(), "textDocument/definition");
        assert_eq!(console.entries()[1].kind(), TranscriptKind::LspReply);

        let reply = console
            .send_control(&mut client, METHOD_GET_CONFIG, vec![])
            .unwrap();
        assert_eq!(reply.method, METHOD_GET_CONFIG);
        assert_eq!(reply.request_id, 1);
        assert_ne!(reply.request_id, 0);
        let cfg = reply.decode_body::<GetConfigResponse>().unwrap();
        assert!(cfg.status.unwrap().is_ok());
        assert_eq!(console.len(), 4);
        assert_eq!(console.entries()[2].kind(), TranscriptKind::ControlRequest);
        assert_eq!(console.entries()[2].request_id(), 1);
        assert_eq!(console.entries()[3].kind(), TranscriptKind::ControlReply);
        assert_eq!(console.entries()[3].request_id(), 1);

        let n = console.drain_pushes(&mut client).unwrap();
        assert_eq!(n, 2);
        assert_eq!(console.len(), 6);
        assert!(console.entries()[4].is_push());
        assert_eq!(console.entries()[4].request_id(), 0);
        assert_eq!(console.entries()[4].method(), METHOD_WATCH_BATCH);
        assert!(console.entries()[4].body().contains("generation=8"));
        assert!(console.entries()[5].is_push());
        assert_eq!(console.entries()[5].request_id(), 0);
        assert_eq!(console.entries()[5].method(), METHOD_TIER_READY);
        assert!(console.entries()[5].body().contains("pkg"));
        let before = console.len();
        assert_eq!(console.drain_pushes(&mut client).unwrap(), 0);
        assert_eq!(console.len(), before);

        assert!(STOCK_LSP_METHODS.contains(&"textDocument/definition"));
        assert_eq!(ProtocolConsole::stock_lsp_methods(), STOCK_LSP_METHODS);
        assert_eq!(
            ProtocolConsole::control_unary_methods(),
            CONTROL_UNARY_METHODS
        );
        assert!(!console
            .entries()
            .iter()
            .any(|e| e.method().contains("$/") || e.method() == "workspace/filesSince"));
    }

    #[test]
    fn protocol_console_facade_server_error_does_not_panic() {
        let mut lsp = FakeLsp::new();
        lsp.script_error("textDocument/definition", IdeError::lsp("gone"));
        let mut console = ProtocolConsole::new();
        let err = console
            .send_lsp(&mut lsp, "textDocument/definition", json!({}))
            .unwrap_err();
        assert!(err.is_lsp());
        assert_eq!(console.len(), 2);
        assert_eq!(console.entries()[1].kind(), TranscriptKind::LspError);
        assert!(console.entries()[1].body().contains("gone"));

        let mut fake = FakeControl::new();
        fake.script_error(METHOD_GET_CONFIG, IdeError::control("boom"));
        let mut client = ControlClient::new(fake);
        let err = console
            .send_control(&mut client, METHOD_GET_CONFIG, vec![])
            .unwrap_err();
        assert!(err.is_control());
        assert_eq!(
            console.entries().last().unwrap().kind(),
            TranscriptKind::ControlError
        );
        assert!(console.entries().last().unwrap().body().contains("boom"));
        assert_eq!(console.len(), 4);
    }

    #[test]
    fn protocol_console_facade_missing_socket_is_domain_error() {
        let mut lsp = FakeLsp::new();
        lsp.script("textDocument/definition", location_json());
        let mut console = ProtocolConsole::new();
        let mut client = ControlClient::new(FakeControl::missing_socket());
        let err = console
            .send_control(&mut client, METHOD_GET_CONFIG, vec![])
            .unwrap_err();
        assert!(err.is_control_socket_missing());
        assert_eq!(
            console.entries().last().unwrap().kind(),
            TranscriptKind::ControlError
        );

        console.record_control_unavailable(&IdeError::control_socket_missing());
        assert!(console
            .entries()
            .iter()
            .any(|e| e.kind() == TranscriptKind::ControlError
                && e.body().contains("control socket missing")));

        let def = console
            .send_lsp(&mut lsp, "textDocument/definition", json!({}))
            .unwrap();
        assert_eq!(def["uri"], "file:///ws/lib.rs");
        assert!(console.entries().iter().any(
            |e| e.method() == "textDocument/definition" && e.kind() == TranscriptKind::LspReply
        ));
    }

    #[test]
    fn protocol_console_facade_encode_body_and_forbidden_files_since() {
        let set = ProtocolConsole::encode_body(METHOD_SET_CONFIG, "packs = []");
        assert_eq!(
            SetConfigRequest::decode(set.as_slice()).unwrap().patch_toml,
            "packs = []"
        );
        let packs = ProtocolConsole::encode_body(METHOD_INSTALL_PACKS, "python rust");
        assert_eq!(
            InstallPacksRequest::decode(packs.as_slice()).unwrap().packs,
            vec!["python", "rust"]
        );
        let empty_packs = ProtocolConsole::encode_body(METHOD_INSTALL_PACKS, "   ");
        assert!(InstallPacksRequest::decode(empty_packs.as_slice())
            .unwrap()
            .packs
            .is_empty());
        let gen = ProtocolConsole::encode_body(METHOD_FILES_SINCE, "7");
        assert_eq!(
            FilesSinceRequest::decode(gen.as_slice()).unwrap().since,
            Some(files_since_request::Since::SinceGeneration(7))
        );
        let none = ProtocolConsole::encode_body(METHOD_FILES_SINCE, "nope");
        assert!(FilesSinceRequest::decode(none.as_slice())
            .unwrap()
            .since
            .is_none());
        assert!(ProtocolConsole::encode_body(METHOD_GET_CONFIG, "x").is_empty());
        assert!(ProtocolConsole::encode_body("NoSuch", "x").is_empty());

        let mut console = ProtocolConsole::new();
        let mut client = ControlClient::new(FakeControl::new());
        assert!(console
            .send_control(&mut client, "$/progressive/filesSince", vec![])
            .unwrap_err()
            .is_control());
        assert_eq!(
            console.entries().last().unwrap().kind(),
            TranscriptKind::ControlError
        );
        assert!(GetConfigRequest {}.encode_to_vec().is_empty());
        let _ = Status::ok();
    }
}
