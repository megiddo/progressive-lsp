//! LSP IO Command + Event mailbox. The UI thread submits [`LspIoRequest`] and
//! polls [`LspIoEvent`]. The IO thread (or a test pump) owns [`LspTransport`].
//! A UI-facing apply path never calls [`LspTransport::request`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::Value;

use crate::discover::DiscoverKind;
use crate::error::IdeError;
use crate::language::ServeMode;
use crate::lsp::{LspClient, LspLocation, ProgressiveLspCap, ServeSpawn, SpawnSpec, StdioLsp};
use crate::ports::LspTransport;
use crate::runtime::DockerRunPlan;

const METHOD_PROGRESS: &str = "$/progress";
const METHOD_LOG_MESSAGE: &str = "window/logMessage";

/// Command the UI sends to the LSP IO thread.
#[derive(Clone, Debug, PartialEq)]
pub enum LspIoRequest {
    Initialize {
        root: PathBuf,
    },
    DidOpen {
        path: PathBuf,
        text: String,
    },
    DidChange {
        path: PathBuf,
        old_text: String,
        new_text: String,
    },
    DidSave {
        path: PathBuf,
    },
    DidClose {
        path: PathBuf,
    },
    Discover {
        kind: DiscoverKind,
        path: PathBuf,
        line: u32,
        character: u32,
    },
    Shutdown,
}

impl LspIoRequest {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Initialize { .. } => "initialize",
            Self::DidOpen { .. } => "textDocument/didOpen",
            Self::DidChange { .. } => "textDocument/didChange",
            Self::DidSave { .. } => "textDocument/didSave",
            Self::DidClose { .. } => "textDocument/didClose",
            Self::Discover { kind, .. } => kind.lsp_method(),
            Self::Shutdown => "shutdown",
        }
    }

    pub fn is_discover(&self) -> bool {
        matches!(self, Self::Discover { .. })
    }

    pub fn is_notification(&self) -> bool {
        matches!(
            self,
            Self::DidOpen { .. }
                | Self::DidChange { .. }
                | Self::DidSave { .. }
                | Self::DidClose { .. }
        )
    }
}

/// `$/progress` begin / report / end. Distinct from the server `ProgressKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LspProgressKind {
    Begin,
    Report,
    End,
}

impl LspProgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Report => "report",
            Self::End => "end",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "begin" => Some(Self::Begin),
            "report" => Some(Self::Report),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

/// DTO for `$/progress`. The IO thread keeps these; it does not drop no-id messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressEvent {
    token: String,
    kind: LspProgressKind,
    message: Option<String>,
    percentage: Option<u32>,
}

impl ProgressEvent {
    pub fn new(
        token: impl Into<String>,
        kind: LspProgressKind,
        message: Option<String>,
        percentage: Option<u32>,
    ) -> Self {
        Self {
            token: token.into(),
            kind,
            message,
            percentage,
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn kind(&self) -> LspProgressKind {
        self.kind
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn percentage(&self) -> Option<u32> {
        self.percentage
    }

    pub fn from_params(params: &Value) -> Option<Self> {
        let value = params.get("value")?;
        let kind = LspProgressKind::parse(value.get("kind")?.as_str()?)?;
        let token = match params.get("token") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let message = value
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_string);
        let percentage = value
            .get("percentage")
            .and_then(|p| p.as_u64())
            .and_then(|n| u32::try_from(n).ok());
        Some(Self {
            token,
            kind,
            message,
            percentage,
        })
    }
}

/// DTO for `window/logMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogMessageEvent {
    typ: i64,
    message: String,
}

impl LogMessageEvent {
    pub fn new(typ: i64, message: impl Into<String>) -> Self {
        Self {
            typ,
            message: message.into(),
        }
    }

    pub fn typ(&self) -> i64 {
        self.typ
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn from_params(params: &Value) -> Option<Self> {
        let message = params.get("message")?.as_str()?.to_string();
        let typ = params.get("type").and_then(|t| t.as_i64()).unwrap_or(0);
        Some(Self { typ, message })
    }
}

/// Event the IO thread yields to the UI inbox.
#[derive(Clone, Debug, PartialEq)]
pub enum LspIoEvent {
    Initialized {
        cap: Option<ProgressiveLspCap>,
    },
    Opened {
        path: PathBuf,
        sent: bool,
    },
    Changed {
        path: PathBuf,
    },
    Saved {
        path: PathBuf,
    },
    Closed {
        path: PathBuf,
    },
    Discover {
        kind: DiscoverKind,
        path: PathBuf,
        line: u32,
        character: u32,
        locations: Result<Vec<LspLocation>, String>,
    },
    Progress(ProgressEvent),
    LogMessage(LogMessageEvent),
    ChildStderr {
        line: String,
    },
    ShutdownDone,
    Failed {
        method: String,
        error: String,
    },
}

impl LspIoEvent {
    pub fn is_progress(&self) -> bool {
        matches!(self, Self::Progress(_))
    }

    pub fn is_log_message(&self) -> bool {
        matches!(self, Self::LogMessage(_))
    }

    pub fn is_discover(&self) -> bool {
        matches!(self, Self::Discover { .. })
    }
}

/// Session / in-flight discover flag. Value object, not a Manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DiscoverFlight {
    kind: Option<DiscoverKind>,
}

impl DiscoverFlight {
    pub fn idle() -> Self {
        Self { kind: None }
    }

    pub fn begin(self, kind: DiscoverKind) -> Self {
        match self.kind {
            Some(_) => self,
            None => Self { kind: Some(kind) },
        }
    }

    pub fn finish(self) -> Self {
        Self { kind: None }
    }

    pub fn is_in_flight(self) -> bool {
        self.kind.is_some()
    }

    pub fn can_submit(self) -> bool {
        self.kind.is_none()
    }

    pub fn kind(self) -> Option<DiscoverKind> {
        self.kind
    }

    pub fn waiting_label(self) -> Option<&'static str> {
        self.kind.map(|_| "waiting for server")
    }
}

/// Command queue + Event inbox. `submit` / `poll` never call [`LspTransport::request`].
#[derive(Debug, Default)]
pub struct LspIoMailbox {
    requests: VecDeque<LspIoRequest>,
    events: VecDeque<LspIoEvent>,
}

impl LspIoMailbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&mut self, req: LspIoRequest) {
        self.requests.push_back(req);
    }

    pub fn take_requests(&mut self) -> Vec<LspIoRequest> {
        self.requests.drain(..).collect()
    }

    pub fn pending_len(&self) -> usize {
        self.requests.len()
    }

    pub fn push_event(&mut self, ev: LspIoEvent) {
        self.events.push_back(ev);
    }

    pub fn poll(&mut self) -> Vec<LspIoEvent> {
        self.events.drain(..).collect()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty() && self.events.is_empty()
    }
}

/// UI-facing channel ends. `submit` / `poll` never call [`LspTransport::request`].
#[derive(Debug)]
pub struct LspIoHandle {
    tx: mpsc::Sender<LspIoRequest>,
    rx: mpsc::Receiver<LspIoEvent>,
}

impl LspIoHandle {
    pub fn pair() -> (Self, mpsc::Receiver<LspIoRequest>, mpsc::Sender<LspIoEvent>) {
        let (req_tx, req_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        (
            Self {
                tx: req_tx,
                rx: ev_rx,
            },
            req_rx,
            ev_tx,
        )
    }

    pub fn submit(&self, req: LspIoRequest) -> Result<(), IdeError> {
        self.tx
            .send(req)
            .map_err(|_| IdeError::lsp("lsp io thread ended"))
    }

    pub fn poll(&self) -> Vec<LspIoEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

/// Classify a JSON-RPC notification (no `id`). `$/progress` and `window/logMessage` are kept.
pub fn classify_notification(v: &Value) -> Option<LspIoEvent> {
    if v.get("id").is_some() {
        return None;
    }
    let method = v.get("method")?.as_str()?;
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    match method {
        METHOD_PROGRESS => ProgressEvent::from_params(&params).map(LspIoEvent::Progress),
        METHOD_LOG_MESSAGE => LogMessageEvent::from_params(&params).map(LspIoEvent::LogMessage),
        _ => None,
    }
}

pub fn notifications_to_events(values: &[Value]) -> Vec<LspIoEvent> {
    values.iter().filter_map(classify_notification).collect()
}

/// IO-thread apply. Tests drive this with [`crate::ports::FakeLsp`]. This path may call `request`.
pub fn dispatch_lsp_io<T: LspTransport>(
    client: &mut LspClient<T>,
    req: LspIoRequest,
) -> Vec<LspIoEvent> {
    match req {
        LspIoRequest::Initialize { root } => match client.initialize(&root) {
            Ok(()) => vec![LspIoEvent::Initialized {
                cap: client.progressive_cap().cloned(),
            }],
            Err(e) => vec![LspIoEvent::Failed {
                method: "initialize".into(),
                error: e.to_string(),
            }],
        },
        LspIoRequest::DidOpen { path, text } => match client.did_open(&path, &text) {
            Ok(sent) => vec![LspIoEvent::Opened { path, sent }],
            Err(e) => vec![LspIoEvent::Failed {
                method: "textDocument/didOpen".into(),
                error: e.to_string(),
            }],
        },
        LspIoRequest::DidChange {
            path,
            old_text,
            new_text,
        } => match client.did_change(&path, &old_text, &new_text) {
            Ok(()) => vec![LspIoEvent::Changed { path }],
            Err(e) => vec![LspIoEvent::Failed {
                method: "textDocument/didChange".into(),
                error: e.to_string(),
            }],
        },
        LspIoRequest::DidSave { path } => match client.did_save(&path) {
            Ok(()) => vec![LspIoEvent::Saved { path }],
            Err(e) => vec![LspIoEvent::Failed {
                method: "textDocument/didSave".into(),
                error: e.to_string(),
            }],
        },
        LspIoRequest::DidClose { path } => match client.did_close(&path) {
            Ok(()) => vec![LspIoEvent::Closed { path }],
            Err(e) => vec![LspIoEvent::Failed {
                method: "textDocument/didClose".into(),
                error: e.to_string(),
            }],
        },
        LspIoRequest::Discover {
            kind,
            path,
            line,
            character,
        } => {
            let result = match kind {
                DiscoverKind::Definition => client.definition(&path, line, character),
                DiscoverKind::Implementation => client.implementation(&path, line, character),
                DiscoverKind::References => client.references(&path, line, character),
            };
            vec![LspIoEvent::Discover {
                kind,
                path,
                line,
                character,
                locations: result.map_err(|e| e.to_string()),
            }]
        }
        LspIoRequest::Shutdown => match client.shutdown() {
            Ok(()) => vec![LspIoEvent::ShutdownDone],
            Err(e) => vec![LspIoEvent::Failed {
                method: "shutdown".into(),
                error: e.to_string(),
            }],
        },
    }
}

/// Test / IO-thread pump: emit kept notifications, then dispatch queued Commands.
pub fn pump_lsp_io<T: LspTransport>(
    mailbox: &mut LspIoMailbox,
    client: &mut LspClient<T>,
    notifications: &[Value],
) {
    for ev in notifications_to_events(notifications) {
        mailbox.push_event(ev);
    }
    for req in mailbox.take_requests() {
        for ev in dispatch_lsp_io(client, req) {
            mailbox.push_event(ev);
        }
    }
}

/// Drain Commands until the request channel disconnects. Used by the IO thread
/// after initialize; tests drop the sender instead of sleeping.
pub fn run_lsp_io_ready<T: LspTransport>(
    client: &mut LspClient<T>,
    req_rx: mpsc::Receiver<LspIoRequest>,
    ev_tx: mpsc::Sender<LspIoEvent>,
) {
    while let Ok(req) = req_rx.recv() {
        for ev in dispatch_lsp_io(client, req) {
            if ev_tx.send(ev).is_err() {
                return;
            }
        }
    }
}

fn emit_stdio_sideband(client: &mut LspClient<StdioLsp>, ev_tx: &mpsc::Sender<LspIoEvent>) {
    let notes = client.transport_mut().take_notifications();
    for ev in notifications_to_events(&notes) {
        if ev_tx.send(ev).is_err() {
            return;
        }
    }
    if let Some(drain) = client.transport().stderr_drain() {
        for line in drain.drain() {
            if ev_tx.send(LspIoEvent::ChildStderr { line }).is_err() {
                return;
            }
        }
    }
}

/// Native Darwin/Linux serve vs container `docker run` stdio. Strategy.
/// Container is [`ServeMode::StockStdio`]; native keeps [`ServeMode::ControlSocket`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LspIoAttach {
    Native(ServeSpawn),
    Container(DockerRunPlan),
}

impl LspIoAttach {
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native(_))
    }

    pub fn is_container(&self) -> bool {
        matches!(self, Self::Container(_))
    }

    pub fn serve_mode(&self) -> ServeMode {
        match self {
            Self::Native(_) => ServeMode::ControlSocket,
            Self::Container(_) => ServeMode::StockStdio,
        }
    }
}

fn open_stdio_from_attach(attach: &LspIoAttach) -> Result<(StdioLsp, ServeMode), IdeError> {
    let mode = attach.serve_mode();
    let transport = match attach {
        LspIoAttach::Native(spawn) => {
            let spec = SpawnSpec::resolve()?;
            StdioLsp::spawn_plan(&spec, spawn)?
        }
        LspIoAttach::Container(plan) => StdioLsp::from_command(plan.command())?,
    };
    Ok((transport, mode))
}

/// One named thread owns child stdin/stdout and the stderr drain.
pub fn spawn_lsp_io(root: PathBuf, attach: LspIoAttach) -> LspIoHandle {
    let (handle, req_rx, ev_tx) = LspIoHandle::pair();
    let _ = std::thread::Builder::new()
        .name("poc-ide-lsp".into())
        .spawn(move || run_stdio_lsp_io(root, attach, req_rx, ev_tx));
    handle
}

fn run_stdio_lsp_io(
    root: PathBuf,
    attach: LspIoAttach,
    req_rx: mpsc::Receiver<LspIoRequest>,
    ev_tx: mpsc::Sender<LspIoEvent>,
) {
    let (transport, mode) = match open_stdio_from_attach(&attach) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = ev_tx.send(LspIoEvent::Failed {
                method: "initialize".into(),
                error: e.to_string(),
            });
            return;
        }
    };
    let mut client = LspClient::new(transport).with_mode(mode);
    match client.initialize(&root) {
        Ok(()) => {
            emit_stdio_sideband(&mut client, &ev_tx);
            if ev_tx
                .send(LspIoEvent::Initialized {
                    cap: client.progressive_cap().cloned(),
                })
                .is_err()
            {
                return;
            }
        }
        Err(e) => {
            emit_stdio_sideband(&mut client, &ev_tx);
            let _ = ev_tx.send(LspIoEvent::Failed {
                method: "initialize".into(),
                error: e.to_string(),
            });
            return;
        }
    }
    while let Ok(req) = req_rx.recv() {
        let shutdown = matches!(req, LspIoRequest::Shutdown);
        for ev in dispatch_lsp_io(&mut client, req) {
            emit_stdio_sideband(&mut client, &ev_tx);
            if ev_tx.send(ev).is_err() {
                return;
            }
        }
        if shutdown {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FakeLsp;
    use serde_json::json;

    fn init_result() -> Value {
        json!({
            "capabilities": {
                "experimental": {
                    "progressiveLsp": { "version": "v1", "socket": null, "mux": false }
                }
            }
        })
    }

    fn progress_json() -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": METHOD_PROGRESS,
            "params": {
                "token": "ingest",
                "value": {
                    "kind": "begin",
                    "title": "index",
                    "message": "syntax",
                    "percentage": 10
                }
            }
        })
    }

    fn log_message_json() -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": METHOD_LOG_MESSAGE,
            "params": { "type": 3, "message": "hi" }
        })
    }

    #[test]
    fn lsp_io_request_command_maps_method_and_discover_flag() {
        assert_eq!(
            LspIoRequest::Initialize { root: "/ws".into() }.method(),
            "initialize"
        );
        assert_eq!(
            LspIoRequest::DidOpen {
                path: "/ws/a.rs".into(),
                text: "fn x".into()
            }
            .method(),
            "textDocument/didOpen"
        );
        assert_eq!(
            LspIoRequest::DidChange {
                path: "/ws/a.rs".into(),
                old_text: "a".into(),
                new_text: "b".into()
            }
            .method(),
            "textDocument/didChange"
        );
        assert_eq!(
            LspIoRequest::DidSave {
                path: "/ws/a.rs".into()
            }
            .method(),
            "textDocument/didSave"
        );
        assert_eq!(
            LspIoRequest::DidClose {
                path: "/ws/a.rs".into()
            }
            .method(),
            "textDocument/didClose"
        );
        assert_eq!(
            LspIoRequest::Discover {
                kind: DiscoverKind::Definition,
                path: "/ws/a.rs".into(),
                line: 1,
                character: 2
            }
            .method(),
            "textDocument/definition"
        );
        assert_eq!(LspIoRequest::Shutdown.method(), "shutdown");
        assert!(LspIoRequest::Discover {
            kind: DiscoverKind::References,
            path: "/ws/a.rs".into(),
            line: 0,
            character: 0
        }
        .is_discover());
        assert!(!LspIoRequest::Shutdown.is_discover());
        assert!(LspIoRequest::DidOpen {
            path: "/ws/a.rs".into(),
            text: String::new()
        }
        .is_notification());
        assert!(!LspIoRequest::Initialize { root: "/ws".into() }.is_notification());
        assert!(!LspIoRequest::Discover {
            kind: DiscoverKind::Implementation,
            path: "/ws/a.rs".into(),
            line: 0,
            character: 0
        }
        .is_notification());
        assert_eq!(
            LspIoRequest::Initialize { root: "/ws".into() },
            LspIoRequest::Initialize { root: "/ws".into() }
        );
    }

    #[test]
    fn progress_event_dto_parses_begin_report_end_and_numeric_token() {
        assert_eq!(LspProgressKind::Begin.as_str(), "begin");
        assert_eq!(LspProgressKind::Report.as_str(), "report");
        assert_eq!(LspProgressKind::End.as_str(), "end");
        assert_eq!(
            LspProgressKind::parse("begin"),
            Some(LspProgressKind::Begin)
        );
        assert_eq!(
            LspProgressKind::parse("report"),
            Some(LspProgressKind::Report)
        );
        assert_eq!(LspProgressKind::parse("end"), Some(LspProgressKind::End));
        assert_eq!(LspProgressKind::parse("nope"), None);
        let begin = ProgressEvent::from_params(&progress_json()["params"]).unwrap();
        assert_eq!(begin.token(), "ingest");
        assert_eq!(begin.kind(), LspProgressKind::Begin);
        assert_eq!(begin.message(), Some("syntax"));
        assert_eq!(begin.percentage(), Some(10));
        let numeric = ProgressEvent::from_params(&json!({
            "token": 7,
            "value": { "kind": "report", "percentage": 50 }
        }))
        .unwrap();
        assert_eq!(numeric.token(), "7");
        assert_eq!(numeric.kind(), LspProgressKind::Report);
        assert_eq!(numeric.percentage(), Some(50));
        assert!(numeric.message().is_none());
        let end = ProgressEvent::from_params(&json!({
            "token": "t",
            "value": { "kind": "end" }
        }))
        .unwrap();
        assert_eq!(end.kind(), LspProgressKind::End);
        assert!(ProgressEvent::from_params(&json!({})).is_none());
        assert!(ProgressEvent::from_params(&json!({ "value": { "kind": "nope" } })).is_none());
        let constructed =
            ProgressEvent::new("x", LspProgressKind::Begin, Some("m".into()), Some(1));
        assert_eq!(constructed.token(), "x");
        assert_eq!(constructed.message(), Some("m"));
        assert_eq!(constructed.percentage(), Some(1));
        let missing_token = ProgressEvent::from_params(&json!({
            "value": { "kind": "begin" }
        }))
        .unwrap();
        assert!(missing_token.token().is_empty());
    }

    #[test]
    fn log_message_event_dto_parses_type_and_message() {
        let ev = LogMessageEvent::from_params(&log_message_json()["params"]).unwrap();
        assert_eq!(ev.typ(), 3);
        assert_eq!(ev.message(), "hi");
        assert!(LogMessageEvent::from_params(&json!({})).is_none());
        let no_type = LogMessageEvent::from_params(&json!({ "message": "x" })).unwrap();
        assert_eq!(no_type.typ(), 0);
        let built = LogMessageEvent::new(1, "err");
        assert_eq!(built.typ(), 1);
        assert_eq!(built.message(), "err");
    }

    #[test]
    fn classify_notification_keeps_progress_and_log_message() {
        let progress = classify_notification(&progress_json()).unwrap();
        assert!(progress.is_progress());
        let log = classify_notification(&log_message_json()).unwrap();
        assert!(log.is_log_message());
        assert!(classify_notification(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": null
        }))
        .is_none());
        assert!(classify_notification(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {}
        }))
        .is_none());
        assert!(classify_notification(&json!({ "jsonrpc": "2.0" })).is_none());
        let events = notifications_to_events(&[
            progress_json(),
            json!({"id": 1, "result": null}),
            log_message_json(),
        ]);
        assert_eq!(events.len(), 2);
        assert!(events[0].is_progress());
        assert!(events[1].is_log_message());
    }

    #[test]
    fn discover_flight_value_object_waiting_label_and_second_begin_is_noop() {
        let idle = DiscoverFlight::idle();
        assert!(!idle.is_in_flight());
        assert!(idle.can_submit());
        assert!(idle.kind().is_none());
        assert!(idle.waiting_label().is_none());
        assert_eq!(idle, DiscoverFlight::default());
        let waiting = idle.begin(DiscoverKind::Definition);
        assert!(waiting.is_in_flight());
        assert!(!waiting.can_submit());
        assert_eq!(waiting.kind(), Some(DiscoverKind::Definition));
        assert_eq!(waiting.waiting_label(), Some("waiting for server"));
        let still = waiting.begin(DiscoverKind::References);
        assert_eq!(still.kind(), Some(DiscoverKind::Definition));
        assert_eq!(still.waiting_label(), Some("waiting for server"));
        let done = still.finish();
        assert!(done.can_submit());
        assert!(!done.is_in_flight());
        assert_ne!(waiting, idle);
    }

    #[test]
    fn lsp_io_mailbox_command_event_ui_apply_never_calls_lsp_transport_request() {
        let mut fake = FakeLsp::new();
        fake.script("initialize", init_result());
        fake.script("textDocument/definition", Value::Null);
        fake.script("textDocument/didOpen", Value::Null);
        let mut client = LspClient::new(fake);
        let mut mailbox = LspIoMailbox::new();
        mailbox.submit(LspIoRequest::Initialize { root: "/ws".into() });
        mailbox.submit(LspIoRequest::DidOpen {
            path: "/ws/lib.rs".into(),
            text: "fn x() {}\n".into(),
        });
        mailbox.submit(LspIoRequest::Discover {
            kind: DiscoverKind::Definition,
            path: "/ws/lib.rs".into(),
            line: 0,
            character: 3,
        });
        assert_eq!(mailbox.pending_len(), 3);
        assert_eq!(mailbox.event_len(), 0);
        assert!(client.transport().sent().is_empty());
        assert!(mailbox.poll().is_empty());

        pump_lsp_io(
            &mut mailbox,
            &mut client,
            &[progress_json(), log_message_json()],
        );
        assert_eq!(mailbox.pending_len(), 0);
        let events = mailbox.poll();
        assert!(events.iter().any(|e| e.is_progress()));
        assert!(events.iter().any(|e| e.is_log_message()));
        assert!(events
            .iter()
            .any(|e| matches!(e, LspIoEvent::Initialized { .. })));
        assert!(events.iter().any(|e| e.is_discover()));
        assert!(events
            .iter()
            .any(|e| matches!(e, LspIoEvent::Opened { .. })));
        assert!(client.transport().sent_methods().contains(&"initialize"));
        assert!(client
            .transport()
            .sent_methods()
            .contains(&"textDocument/definition"));
        assert!(mailbox.is_empty());
    }

    #[test]
    fn lsp_io_handle_command_event_ui_apply_never_calls_lsp_transport_request() {
        let (handle, req_rx, ev_tx) = LspIoHandle::pair();
        let fake = FakeLsp::new();
        handle
            .submit(LspIoRequest::DidChange {
                path: "/ws/a.rs".into(),
                old_text: "a".into(),
                new_text: "b".into(),
            })
            .unwrap();
        let req = req_rx.try_recv().unwrap();
        assert!(req.is_notification());
        assert!(fake.sent().is_empty());
        ev_tx
            .send(LspIoEvent::Progress(ProgressEvent::new(
                "t",
                LspProgressKind::Begin,
                None,
                None,
            )))
            .unwrap();
        ev_tx
            .send(LspIoEvent::Discover {
                kind: DiscoverKind::Definition,
                path: "/ws/a.rs".into(),
                line: 0,
                character: 0,
                locations: Ok(vec![]),
            })
            .unwrap();
        let events = handle.poll();
        assert_eq!(events.len(), 2);
        assert!(events[0].is_progress());
        assert!(events[1].is_discover());
        assert!(fake.sent().is_empty());
        drop(req_rx);
        assert!(handle.submit(LspIoRequest::Shutdown).unwrap_err().is_lsp());
    }

    #[test]
    fn dispatch_lsp_io_covers_notify_discover_shutdown_and_errors() {
        let mut fake = FakeLsp::new();
        fake.script("initialize", init_result());
        fake.script("textDocument/implementation", json!([]));
        fake.script("textDocument/references", Value::Null);
        fake.script("shutdown", Value::Null);
        let mut client = LspClient::new(fake);
        let init = dispatch_lsp_io(&mut client, LspIoRequest::Initialize { root: "/ws".into() });
        assert!(matches!(init[0], LspIoEvent::Initialized { .. }));
        assert!(dispatch_lsp_io(
            &mut client,
            LspIoRequest::DidOpen {
                path: "/ws/lib.rs".into(),
                text: "fn x() {}\n".into()
            }
        )
        .iter()
        .any(|e| matches!(e, LspIoEvent::Opened { sent: true, .. })));
        assert!(dispatch_lsp_io(
            &mut client,
            LspIoRequest::DidChange {
                path: "/ws/lib.rs".into(),
                old_text: "fn x() {}\n".into(),
                new_text: "fn y() {}\n".into()
            }
        )
        .iter()
        .any(|e| matches!(e, LspIoEvent::Changed { .. })));
        assert!(dispatch_lsp_io(
            &mut client,
            LspIoRequest::DidSave {
                path: "/ws/lib.rs".into()
            }
        )
        .iter()
        .any(|e| matches!(e, LspIoEvent::Saved { .. })));
        let impls = dispatch_lsp_io(
            &mut client,
            LspIoRequest::Discover {
                kind: DiscoverKind::Implementation,
                path: "/ws/lib.rs".into(),
                line: 0,
                character: 0,
            },
        );
        assert!(matches!(
            &impls[0],
            LspIoEvent::Discover {
                kind: DiscoverKind::Implementation,
                locations: Ok(locs),
                ..
            } if locs.is_empty()
        ));
        let refs = dispatch_lsp_io(
            &mut client,
            LspIoRequest::Discover {
                kind: DiscoverKind::References,
                path: "/ws/lib.rs".into(),
                line: 0,
                character: 0,
            },
        );
        assert!(refs[0].is_discover());
        assert!(dispatch_lsp_io(
            &mut client,
            LspIoRequest::DidClose {
                path: "/ws/lib.rs".into()
            }
        )
        .iter()
        .any(|e| matches!(e, LspIoEvent::Closed { .. })));
        assert!(matches!(
            dispatch_lsp_io(&mut client, LspIoRequest::Shutdown)[0],
            LspIoEvent::ShutdownDone
        ));

        let mut missing = LspClient::new(FakeLsp::missing_binary());
        let fail = dispatch_lsp_io(
            &mut missing,
            LspIoRequest::Initialize { root: "/ws".into() },
        );
        assert!(matches!(fail[0], LspIoEvent::Failed { .. }));
        let mut not_ready = LspClient::new(FakeLsp::new());
        assert!(matches!(
            dispatch_lsp_io(
                &mut not_ready,
                LspIoRequest::DidOpen {
                    path: "/ws/a.rs".into(),
                    text: "x".into()
                }
            )[0],
            LspIoEvent::Failed { .. }
        ));
        assert!(matches!(
            dispatch_lsp_io(
                &mut not_ready,
                LspIoRequest::DidChange {
                    path: "/ws/a.rs".into(),
                    old_text: "x".into(),
                    new_text: "y".into()
                }
            )[0],
            LspIoEvent::Failed { .. }
        ));
        assert!(matches!(
            dispatch_lsp_io(
                &mut not_ready,
                LspIoRequest::DidSave {
                    path: "/ws/a.rs".into()
                }
            )[0],
            LspIoEvent::Failed { .. }
        ));
        assert!(matches!(
            dispatch_lsp_io(
                &mut not_ready,
                LspIoRequest::DidClose {
                    path: "/ws/a.rs".into()
                }
            )[0],
            LspIoEvent::Failed { .. }
        ));
        assert!(matches!(
            dispatch_lsp_io(
                &mut not_ready,
                LspIoRequest::Discover {
                    kind: DiscoverKind::Definition,
                    path: "/ws/a.rs".into(),
                    line: 0,
                    character: 0
                }
            )[0],
            LspIoEvent::Discover {
                locations: Err(_),
                ..
            }
        ));
        assert!(matches!(
            dispatch_lsp_io(&mut not_ready, LspIoRequest::Shutdown)[0],
            LspIoEvent::Failed { .. }
        ));
    }

    #[test]
    fn run_lsp_io_ready_drains_until_disconnect_without_sleep() {
        let mut fake = FakeLsp::new();
        fake.script("initialize", init_result());
        fake.script("textDocument/definition", Value::Null);
        let mut client = LspClient::new(fake);
        client.initialize("/ws").unwrap();
        let (req_tx, req_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        req_tx
            .send(LspIoRequest::Discover {
                kind: DiscoverKind::Definition,
                path: "/ws/lib.rs".into(),
                line: 0,
                character: 0,
            })
            .unwrap();
        drop(req_tx);
        run_lsp_io_ready(&mut client, req_rx, ev_tx);
        let events: Vec<_> = ev_rx.try_iter().collect();
        assert_eq!(events.len(), 1);
        assert!(events[0].is_discover());
    }

    #[test]
    fn lsp_io_mailbox_push_and_event_len() {
        let mut mailbox = LspIoMailbox::new();
        assert!(mailbox.is_empty());
        mailbox.push_event(LspIoEvent::ShutdownDone);
        mailbox.push_event(LspIoEvent::ChildStderr { line: "e".into() });
        mailbox.push_event(LspIoEvent::Failed {
            method: "x".into(),
            error: "y".into(),
        });
        assert_eq!(mailbox.event_len(), 3);
        assert_eq!(mailbox.poll().len(), 3);
        assert!(mailbox.is_empty());
        let debug = format!("{:?}", LspIoMailbox::new());
        assert!(debug.contains("LspIoMailbox"));
    }

    #[test]
    fn lsp_io_attach_strategy_container_is_stock_stdio_without_darwin_resolve() {
        let native =
            LspIoAttach::Native(ServeSpawn::new(ServeMode::StockStdio, None, None).unwrap());
        assert!(native.is_native());
        assert!(!native.is_container());
        assert_eq!(native.serve_mode(), ServeMode::ControlSocket);
        assert_ne!(
            native.serve_mode(),
            ServeMode::StockStdio,
            "native attach keeps ControlSocket even if ServeSpawn was stock"
        );

        let plan = DockerRunPlan::new("docker", std::path::Path::new("/ws")).unwrap();
        let container = LspIoAttach::Container(plan.clone());
        assert!(container.is_container());
        assert!(!container.is_native());
        assert_eq!(container.serve_mode(), ServeMode::StockStdio);
        assert_eq!(container, LspIoAttach::Container(plan));
        assert_ne!(container, native);

        let missing = DockerRunPlan::new("/no/such/docker-host5", std::path::Path::new("/ws"));
        assert!(missing.unwrap_err().is_runtime());

        let true_bin = std::path::Path::new("/usr/bin/true");
        if true_bin.is_file() {
            let plan = DockerRunPlan::new(true_bin, std::path::Path::new("/ws")).unwrap();
            let (lsp, mode) = open_stdio_from_attach(&LspIoAttach::Container(plan)).unwrap();
            assert_eq!(mode, ServeMode::StockStdio);
            drop(lsp);
        }
    }
}
