//! Minimal LSP client for pack engine children (stdio JSON-RPC).

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{path_to_file_uri, LanguageId, Tier};
use progressive_lsp_protocol::framing::{read_message, write_message};
use progressive_lsp_protocol::rpc::{self, success};
use progressive_lsp_resolve::{
    Hover, LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery, ResolveResult,
};

use crate::adapter::{ChildHandle, EngineMessage};

#[derive(Debug, thiserror::Error)]
pub enum LspChildError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("framing: {0}")]
    Framing(#[from] progressive_lsp_protocol::framing::FramingError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no os child stdio")]
    NoStdio,
    #[error("lsp error: {0}")]
    Rpc(String),
    #[error("unexpected eof")]
    Eof,
}

struct PendingLsp {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
    workspace: PathBuf,
    default_language: String,
}

struct LspSession {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
    next_id: i64,
    workspace: PathBuf,
    default_language: String,
    opened: HashSet<String>,
    doc_version: u32,
}

struct LspState {
    pending: Option<PendingLsp>,
    session: Option<LspSession>,
    /// Background [`schedule_warm`] already started for this proxy.
    warm_started: bool,
}

/// Thread-safe LSP proxy attached to a live pack child.
pub struct LspChildProxy {
    inner: Mutex<LspState>,
}

impl LspChildProxy {
    fn new_pending(
        reader: impl BufRead + Send + 'static,
        writer: impl Write + Send + 'static,
        workspace: impl AsRef<Path>,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            inner: Mutex::new(LspState {
                pending: Some(PendingLsp {
                    reader: Box::new(reader),
                    writer: Box::new(writer),
                    workspace: workspace.as_ref().to_path_buf(),
                    default_language: default_language.into(),
                }),
                session: None,
                warm_started: false,
            }),
        }
    }

    /// Child LSP handshake + inbox drain finished — safe to query without blocking the chain on init.
    pub fn is_warmed(&self) -> bool {
        self.inner
            .lock()
            .ok()
            .is_some_and(|s| s.session.is_some())
    }

    /// Initialize javacs/clangd/etc. off the mux/resolve thread after `didOpen` is queued.
    pub fn schedule_warm(self: &Arc<Self>, handle: ChildHandle, default_language: LanguageId) {
        let start = {
            let mut state = match self.inner.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            if state.session.is_some() || state.warm_started {
                return;
            }
            state.warm_started = true;
            true
        };
        if !start {
            return;
        }
        let lsp = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("plsp-warm-{}", handle.pack_name))
            .spawn(move || {
                let _ = lsp.with_session(|s| s.drain_inbox(&handle, &default_language));
            })
            .ok();
    }

    /// Eager init for unit tests. Production uses [`attach_os_child`] (lazy init).
    pub fn for_test(
        reader: impl BufRead + Send + 'static,
        writer: impl Write + Send + 'static,
        workspace: impl AsRef<Path>,
        default_language: impl Into<String>,
    ) -> Result<Self, LspChildError> {
        let proxy = Self::new_pending(reader, writer, workspace, default_language);
        proxy.with_session(|_| Ok(()))?;
        Ok(proxy)
    }

    fn with_session<R>(
        &self,
        f: impl FnOnce(&mut LspSession) -> Result<R, LspChildError>,
    ) -> Result<R, LspChildError> {
        let mut state = self.inner.lock().expect("lsp");
        let session = match state.session.as_mut() {
            Some(s) => s,
            None => {
                let pending = state
                    .pending
                    .take()
                    .ok_or(LspChildError::NoStdio)?;
                let mut session = LspSession {
                    reader: pending.reader,
                    writer: pending.writer,
                    next_id: 1,
                    workspace: pending.workspace,
                    default_language: pending.default_language,
                    opened: HashSet::new(),
                    doc_version: 0,
                };
                session.initialize()?;
                state.session = Some(session);
                state.session.as_mut().expect("session")
            }
        };
        f(session)
    }

    pub fn did_open(&self, uri: &str, language_id: &str, text: &str) -> Result<(), LspChildError> {
        self.with_session(|s| s.ensure_open(uri, language_id, text))
    }

    pub fn resolve_query(
        &self,
        handle: &ChildHandle,
        default_language: &LanguageId,
        q: &ResolveQuery,
    ) -> ResolveOutcome {
        if !self.is_warmed() {
            return ResolveOutcome::NotReady;
        }
        match self.with_session(|s| {
            s.drain_inbox(handle, default_language)?;
            let uri = s
                .file_uri(&q.file.as_str())
                .ok_or_else(|| LspChildError::Rpc("missing file uri".into()))?;
            if !s.opened.contains(&uri) {
                return Err(LspChildError::Rpc(
                    "document not open in engine (wait for warm/didOpen)".into(),
                ));
            }
            s.request_query(q.kind, &uri, q.position)
        }) {
            Ok(result) => ResolveOutcome::Ready(result),
            Err(_) => ResolveOutcome::NotReady,
        }
    }
}

/// Wire pack stdio for later LSP. Does **not** block on child `initialize` — serve must
/// answer the IDE `initialize` before engines are warmed up.
pub fn attach_os_child(
    handle: &mut ChildHandle,
    workspace: &Path,
    default_language: &LanguageId,
) -> Result<(), LspChildError> {
    let (stdin, stdout) = handle.take_lsp_pipes().ok_or(LspChildError::NoStdio)?;
    let proxy = LspChildProxy::new_pending(
        std::io::BufReader::new(stdout),
        std::io::BufWriter::new(stdin),
        workspace,
        default_language.as_str(),
    );
    handle.set_lsp(Arc::new(proxy));
    Ok(())
}

impl LspSession {
    fn initialize(&mut self) -> Result<(), LspChildError> {
        let root_uri = path_to_file_uri(&self.workspace);
        let folder_name = self
            .workspace
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("workspace");
        let workspace_folders = serde_json::json!([{
            "uri": root_uri,
            "name": folder_name,
        }]);
        let params = serde_json::json!({
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": workspace_folders,
            "capabilities": {
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "applyEdit": true,
                },
                "window": {
                    "workDoneProgress": true,
                },
                "textDocument": {
                    "publishDiagnostics": { "relatedInformation": true },
                },
            },
            "clientInfo": { "name": "progressive-lsp-engine", "version": "0.1.0" },
        });
        let _ = self.request("initialize", params)?;
        self.notify(
            "initialized",
            serde_json::json!({}),
        )?;
        Ok(())
    }

    fn drain_inbox(
        &mut self,
        handle: &ChildHandle,
        default_language: &LanguageId,
    ) -> Result<(), LspChildError> {
        for msg in handle.take_inbox() {
            match msg {
                EngineMessage::DidOpen {
                    uri,
                    language_id,
                    text,
                } => {
                    self.ensure_open(&uri, &language_id, &text)?;
                }
                EngineMessage::DidChange { uri, text } => {
                    self.did_change(&uri, &text)?;
                }
                EngineMessage::Watch { paths: _ } => {}
            }
        }
        let _ = default_language;
        Ok(())
    }

    fn ensure_open(&mut self, uri: &str, language_id: &str, text: &str) -> Result<(), LspChildError> {
        if self.opened.contains(uri) {
            if !text.is_empty() {
                return self.did_change(uri, text);
            }
            return Ok(());
        }
        self.doc_version = self.doc_version.saturating_add(1).max(1);
        let lang = if language_id.is_empty() {
            self.default_language.as_str()
        } else {
            language_id
        };
        self.notify(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": lang,
                    "version": self.doc_version,
                    "text": text,
                }
            }),
        )?;
        self.opened.insert(uri.to_string());
        Ok(())
    }

    fn did_change(&mut self, uri: &str, text: &str) -> Result<(), LspChildError> {
        if !self.opened.contains(uri) {
            let lang = self.default_language.clone();
            return self.ensure_open(uri, &lang, text);
        }
        self.doc_version = self.doc_version.saturating_add(1);
        self.notify(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": uri, "version": self.doc_version },
                "contentChanges": [{ "text": text }],
            }),
        )?;
        Ok(())
    }

    fn request_query(
        &mut self,
        kind: QueryKind,
        uri: &str,
        position: Position,
    ) -> Result<ResolveResult, LspChildError> {
        let method = match kind {
            QueryKind::Definition => "textDocument/definition",
            QueryKind::TypeDefinition => "textDocument/typeDefinition",
            QueryKind::References => "textDocument/references",
            QueryKind::Implementation => "textDocument/implementation",
            QueryKind::Hover => "textDocument/hover",
            QueryKind::DocumentSymbol | QueryKind::WorkspaceSymbol => {
                return Err(LspChildError::Rpc("unsupported query".into()));
            }
        };
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": position.line, "character": position.character },
        });
        let result = self.request(method, params)?;
        if kind == QueryKind::Hover {
            return Ok(hover_result(result));
        }
        let locations = parse_locations(&result)?;
        Ok(ResolveResult::locations(Tier::Types, locations))
    }

    fn file_uri(&self, file: &str) -> Option<String> {
        if file.starts_with("file:") {
            return Some(file.to_string());
        }
        let path = Path::new(file);
        if path.is_absolute() {
            return Some(path_to_file_uri(path));
        }
        Some(path_to_file_uri(&self.workspace.join(file)))
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, LspChildError> {
        let id = self.next_id;
        self.next_id += 1;
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, serde_json::to_vec(&body)?)?;
        self.read_response(id)
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), LspChildError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, serde_json::to_vec(&body)?)?;
        Ok(())
    }

    fn read_response(&mut self, id: i64) -> Result<serde_json::Value, LspChildError> {
        loop {
            let Some(bytes) = read_message(&mut self.reader)? else {
                return Err(LspChildError::Eof);
            };
            let v: serde_json::Value = serde_json::from_slice(&bytes)?;
            if v.get("method").is_some() && v.get("id").is_some() {
                self.reply_server_request(&v)?;
                continue;
            }
            if v.get("method").is_some() {
                continue;
            }
            if let Some(got) = v.get("id") {
                if !rpc::id_matches(id, got) {
                    continue;
                }
                if let Some(err) = v.get("error") {
                    return Err(LspChildError::Rpc(err.to_string()));
                }
                return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
            }
        }
    }

    fn reply_server_request(&mut self, msg: &serde_json::Value) -> Result<(), LspChildError> {
        let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
        let result = self.server_request_result(method, &params);
        let body = success(Some(id), result);
        write_message(&mut self.writer, serde_json::to_vec(&body)?)?;
        Ok(())
    }

    fn server_request_result(&self, method: &str, params: &serde_json::Value) -> serde_json::Value {
        match method {
            "workspace/configuration" => {
                let n = params
                    .get("items")
                    .and_then(|i| i.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                serde_json::Value::Array(vec![serde_json::Value::Null; n])
            }
            "workspace/workspaceFolders" => {
                let root_uri = path_to_file_uri(&self.workspace);
                let name = self
                    .workspace
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("workspace");
                serde_json::json!([{ "uri": root_uri, "name": name }])
            }
            "window/workDoneProgress/create" => serde_json::Value::Null,
            "workspace/applyEdit" => serde_json::json!({ "applied": true }),
            "window/showDocument" => serde_json::json!({ "success": true }),
            "client/registerCapability" | "client/unregisterCapability" => serde_json::Value::Null,
            "workspace/executeClientCommand" => serde_json::Value::Null,
            "window/showMessageRequest" => serde_json::Value::Null,
            "workspace/codeLens/refresh"
            | "workspace/semanticTokens/refresh"
            | "workspace/inlayHint/refresh"
            | "workspace/diagnostic/refresh"
            | "workspace/fileOperations/refresh" => serde_json::Value::Null,
            "client/registerFeature" | "client/unregisterFeature" => serde_json::Value::Null,
            _ => serde_json::Value::Null,
        }
    }
}

fn hover_result(value: serde_json::Value) -> ResolveResult {
    if value.is_null() {
        return ResolveResult {
            locations: Vec::new(),
            tier: Tier::Types,
            hover: None,
            symbols: Vec::new(),
        };
    }
    let text = value
        .get("contents")
        .map(hover_contents_text)
        .unwrap_or_default();
    let hover = if text.is_empty() {
        None
    } else {
        Some(Hover {
            name: text.clone(),
            arity: None,
            type_info: None,
        })
    };
    ResolveResult {
        locations: Vec::new(),
        tier: Tier::Types,
        hover,
        symbols: Vec::new(),
    }
}

fn hover_contents_text(contents: &serde_json::Value) -> String {
    if let Some(s) = contents.as_str() {
        return s.to_string();
    }
    if let Some(marked) = contents.as_object() {
        if let Some(v) = marked.get("value").and_then(|x| x.as_str()) {
            return v.to_string();
        }
    }
    if let Some(arr) = contents.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(String::from)
                    .or_else(|| item.get("value").and_then(|v| v.as_str()).map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn parse_locations(value: &serde_json::Value) -> Result<Vec<LspLocation>, LspChildError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(obj) = value.as_object() {
        if obj.contains_key("uri") {
            return Ok(vec![location_from_value(value)?]);
        }
    }
    if let Some(arr) = value.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            if item.get("targetUri").is_some() {
                out.push(location_link_from_value(item)?);
            } else {
                out.push(location_from_value(item)?);
            }
        }
        return Ok(out);
    }
    Ok(Vec::new())
}

fn location_from_value(v: &serde_json::Value) -> Result<LspLocation, LspChildError> {
    let uri = v
        .get("uri")
        .and_then(|u| u.as_str())
        .ok_or_else(|| LspChildError::Rpc("location missing uri".into()))?
        .to_string();
    let range = range_from_value(v.get("range").ok_or_else(|| LspChildError::Rpc("location missing range".into()))?)?;
    Ok(LspLocation::new(uri, range, Tier::Types))
}

fn location_link_from_value(v: &serde_json::Value) -> Result<LspLocation, LspChildError> {
    let uri = v
        .get("targetUri")
        .and_then(|u| u.as_str())
        .ok_or_else(|| LspChildError::Rpc("location link missing targetUri".into()))?
        .to_string();
    let range_key = if v.get("targetSelectionRange").is_some() {
        "targetSelectionRange"
    } else {
        "targetRange"
    };
    let range = range_from_value(v.get(range_key).ok_or_else(|| LspChildError::Rpc("location link missing range".into()))?)?;
    Ok(LspLocation::new(uri, range, Tier::Types))
}

fn range_from_value(v: &serde_json::Value) -> Result<Range, LspChildError> {
    let start = v.get("start").ok_or_else(|| LspChildError::Rpc("range missing start".into()))?;
    let end = v.get("end").ok_or_else(|| LspChildError::Rpc("range missing end".into()))?;
    Ok(Range::new(position_from_value(start)?, position_from_value(end)?))
}

fn position_from_value(v: &serde_json::Value) -> Result<Position, LspChildError> {
    Ok(Position::new(
        v.get("line").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        v.get("character").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FileId;
    use progressive_lsp_protocol::framing::encode_message;
    use std::io::{Cursor, Read, Write};

    #[cfg(unix)]
    fn mock_lsp_streams() -> (
        std::io::BufReader<impl Read>,
        impl Write,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::BufReader;
        use std::os::unix::net::UnixStream;

        let (client_read, server_write) = UnixStream::pair().expect("pair");
        let (client_write, server_read) = UnixStream::pair().expect("pair");
        client_read.set_nonblocking(false).ok();
        client_write.set_nonblocking(false).ok();
        server_read.set_nonblocking(false).ok();
        server_write.set_nonblocking(false).ok();

        let handle = std::thread::spawn(move || {
            let mut read = BufReader::new(server_read);
            let mut write = server_write;
            loop {
                let Some(body) = read_message(&mut read).unwrap_or(None) else {
                    break;
                };
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else {
                    continue;
                };
                if v.get("id").is_none() {
                    continue;
                }
                let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
                let result = match method {
                    "initialize" => serde_json::json!({ "capabilities": {} }),
                    "textDocument/definition" => serde_json::json!({
                        "uri": "file:///ws/Lib.java",
                        "range": {
                            "start": { "line": 1, "character": 4 },
                            "end": { "line": 1, "character": 7 }
                        }
                    }),
                    "textDocument/hover" => serde_json::json!({
                        "contents": { "kind": "markdown", "value": "int x" }
                    }),
                    _ => serde_json::Value::Null,
                };
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result,
                });
                write_message(&mut write, serde_json::to_vec(&resp).unwrap()).ok();
            }
        });

        (
            BufReader::new(client_read),
            client_write,
            handle,
        )
    }

    #[test]
    #[cfg(unix)]
    fn mock_lsp_definition_and_hover() {
        let (reader, writer, server) = mock_lsp_streams();
        let proxy = LspChildProxy::for_test(reader, writer, "/ws", "java").expect("connect");
        let handle = ChildHandle::new(1, "java", crate::capabilities::EngineCapabilities::types_full());
        handle.push_message(EngineMessage::DidOpen {
            uri: "file:///ws/T.java".into(),
            language_id: "java".into(),
            text: "class T {}".into(),
        });
        let q = ResolveQuery::new(
            FileId::new("T.java"),
            Position::new(0, 6),
            QueryKind::Definition,
        );
        match proxy.resolve_query(&handle, &LanguageId::new("java"), &q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.locations.len(), 1);
                assert_eq!(r.locations[0].uri, "file:///ws/Lib.java");
            }
            other => panic!("{other:?}"),
        }
        let hq = ResolveQuery::new(FileId::new("T.java"), Position::new(0, 6), QueryKind::Hover);
        match proxy.resolve_query(&handle, &LanguageId::new("java"), &hq) {
            ResolveOutcome::Ready(r) => assert_eq!(r.hover.unwrap().name, "int x"),
            other => panic!("{other:?}"),
        }
        drop(proxy);
        server.join().ok();
    }

    #[test]
    fn read_response_replies_to_workspace_configuration_and_apply_edit() {
        use std::sync::{Arc, Mutex};

        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("cap").extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let cfg = encode_message(
            br#"{"jsonrpc":"2.0","id":10,"method":"workspace/configuration","params":{"items":[{"section":"java"},{"section":"eclipse"}]}}"#,
        );
        let edit = encode_message(
            br#"{"jsonrpc":"2.0","id":11,"method":"workspace/applyEdit","params":{"edit":{"changes":{}}}}"#,
        );
        let def = encode_message(br#"{"jsonrpc":"2.0","id":2,"result":null}"#);
        let mut input = cfg;
        input.extend_from_slice(&edit);
        input.extend_from_slice(&def);
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut session = LspSession {
            reader: Box::new(Cursor::new(input)),
            writer: Box::new(Capture(Arc::clone(&out))),
            next_id: 1,
            workspace: PathBuf::from("/ws"),
            default_language: "java".into(),
            opened: HashSet::from(["file:///ws/T.java".to_string()]),
            doc_version: 1,
        };
        session.next_id = 2;
        session
            .request_query(
                QueryKind::Definition,
                "file:///ws/T.java",
                Position::default(),
            )
            .unwrap();
        let bytes = out.lock().expect("cap").clone();
        let written = String::from_utf8_lossy(&bytes);
        assert!(
            written.contains("\"id\":10") && written.contains("null"),
            "configuration: {written}"
        );
        assert!(
            written.contains("\"applied\":true"),
            "applyEdit: {written}"
        );
    }

    #[test]
    fn read_response_replies_to_server_register_capability() {
        use std::sync::{Arc, Mutex};

        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("cap").extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let reg = encode_message(
            br#"{"jsonrpc":"2.0","id":99,"method":"client/registerCapability","params":{"registrations":[]}}"#,
        );
        let def = encode_message(
            br#"{"jsonrpc":"2.0","id":2,"result":null}"#,
        );
        let mut input = reg;
        input.extend_from_slice(&def);
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut session = LspSession {
            reader: Box::new(Cursor::new(input)),
            writer: Box::new(Capture(Arc::clone(&out))),
            next_id: 1,
            workspace: PathBuf::from("/ws"),
            default_language: "java".into(),
            opened: HashSet::from(["file:///ws/T.java".to_string()]),
            doc_version: 1,
        };
        session.next_id = 2;
        let result = session
            .request_query(
                QueryKind::Definition,
                "file:///ws/T.java",
                Position::default(),
            )
            .unwrap();
        assert!(result.locations.is_empty());
        let written = out.lock().expect("cap");
        assert!(
            String::from_utf8_lossy(&written).contains("\"id\":99"),
            "must answer server request"
        );
    }

    #[test]
    fn cold_proxy_resolve_is_not_ready_without_blocking_on_initialize() {
        use std::io::{Cursor, Write};

        struct NullW;
        impl Write for NullW {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let proxy = LspChildProxy::new_pending(
            std::io::BufReader::new(Cursor::new(b"")),
            NullW,
            "/ws",
            "java",
        );
        assert!(!proxy.is_warmed());
        let handle = ChildHandle::new(1, "java", crate::capabilities::EngineCapabilities::types_full());
        let q = ResolveQuery::new(FileId::new("T.java"), Position::new(0, 0), QueryKind::Definition);
        match proxy.resolve_query(&handle, &LanguageId::new("java"), &q) {
            ResolveOutcome::NotReady => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_definition_is_ready_empty_list() {
        let def_resp = encode_message(br#"{"jsonrpc":"2.0","id":2,"result":null}"#);
        let mut session = LspSession {
            reader: Box::new(Cursor::new(def_resp)),
            writer: Box::new(Vec::new()),
            next_id: 1,
            workspace: PathBuf::from("/ws"),
            default_language: "java".into(),
            opened: HashSet::from(["file:///ws/T.java".to_string()]),
            doc_version: 1,
        };
        session.next_id = 2;
        let result = session
            .request_query(
                QueryKind::Definition,
                "file:///ws/T.java",
                Position::default(),
            )
            .unwrap();
        assert!(result.locations.is_empty());
    }

    #[test]
    fn parse_location_link_array() {
        let v = serde_json::json!([{
            "originSelectionRange": { "start": {"line":0,"character":0}, "end": {"line":0,"character":1} },
            "targetUri": "file:///b.rs",
            "targetRange": { "start": {"line":2,"character":0}, "end": {"line":2,"character":3} },
            "targetSelectionRange": { "start": {"line":2,"character":0}, "end": {"line":2,"character":1} }
        }]);
        let locs = parse_locations(&v).unwrap();
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].uri, "file:///b.rs");
        assert_eq!(locs[0].range.start.line, 2);
    }
}
