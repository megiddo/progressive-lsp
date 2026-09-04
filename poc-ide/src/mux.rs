//! `MuxStdio` Adapter: one stdio pipe, two channels. Reuses protocol `MuxFrame`.
//!
//! Channel 0 carries opaque JSON-RPC bytes (same body `serve_mux` writes).
//! Channel 1 carries the same length-prefixed Envelope as a Unix control socket.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use progressive_lsp_control::{decode_frame, encode_frame, DecodeOutcome, Envelope};
use progressive_lsp_protocol::{
    encode_mux_frame, read_mux_frame, write_mux_frame, MuxError, CHANNEL_CONTROL, CHANNEL_LSP,
    MAX_MUX_PAYLOAD,
};
use serde_json::{json, Value};

use crate::child_stderr::ChildStderrDrain;
use crate::error::IdeError;
use crate::lsp::StdioLsp;
use crate::ports::{ControlTransport, LspTransport};

const METHOD_NOT_FOUND: i64 = -32601;

fn map_mux(err: MuxError) -> IdeError {
    IdeError::lsp(err.to_string())
}

fn map_control_mux(err: MuxError) -> IdeError {
    IdeError::control(err.to_string())
}

struct MuxInner {
    writer: Mutex<Box<dyn Write + Send>>,
    reader: Mutex<Box<dyn Read + Send>>,
    lsp_inbox: Mutex<VecDeque<Vec<u8>>>,
    control_inbox: Mutex<VecDeque<Vec<u8>>>,
    failed: Mutex<Option<String>>,
}

impl MuxInner {
    fn new(writer: impl Write + Send + 'static, reader: impl Read + Send + 'static) -> Self {
        Self {
            writer: Mutex::new(Box::new(writer)),
            reader: Mutex::new(Box::new(reader)),
            lsp_inbox: Mutex::new(VecDeque::new()),
            control_inbox: Mutex::new(VecDeque::new()),
            failed: Mutex::new(None),
        }
    }

    fn fail(&self, err: IdeError) -> IdeError {
        if let Ok(mut slot) = self.failed.lock() {
            if slot.is_none() {
                *slot = Some(err.to_string());
            }
        }
        err
    }

    fn check_failed(&self) -> Result<(), IdeError> {
        match self.failed.lock() {
            Ok(slot) => match slot.as_ref() {
                Some(msg) => Err(IdeError::lsp(msg.clone())),
                None => Ok(()),
            },
            Err(_) => Err(IdeError::lsp("mux lock poisoned")),
        }
    }

    fn write_channel(&self, channel: u8, payload: &[u8]) -> Result<(), IdeError> {
        self.check_failed()?;
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| IdeError::lsp("mux write lock poisoned"))?;
        write_mux_frame(&mut *writer, channel, payload).map_err(map_mux)
    }

    fn take_inbox(inbox: &Mutex<VecDeque<Vec<u8>>>) -> Option<Vec<u8>> {
        inbox.lock().ok().and_then(|mut q| q.pop_front())
    }

    fn park(&self, channel: u8, payload: Vec<u8>) {
        let inbox = if channel == CHANNEL_LSP {
            &self.lsp_inbox
        } else {
            &self.control_inbox
        };
        if let Ok(mut q) = inbox.lock() {
            q.push_back(payload);
        }
    }

    fn read_channel(&self, want: u8) -> Result<Vec<u8>, IdeError> {
        self.check_failed()?;
        let inbox = if want == CHANNEL_LSP {
            &self.lsp_inbox
        } else {
            &self.control_inbox
        };
        if let Some(payload) = Self::take_inbox(inbox) {
            return Ok(payload);
        }
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| IdeError::lsp("mux read lock poisoned"))?;
        loop {
            if let Some(payload) = Self::take_inbox(inbox) {
                return Ok(payload);
            }
            let frame = match read_mux_frame(&mut *reader) {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    return Err(self.fail(IdeError::lsp("eof waiting for mux frame")));
                }
                Err(e) => return Err(self.fail(map_mux(e))),
            };
            if frame.channel == want {
                return Ok(frame.payload);
            }
            if frame.channel == CHANNEL_LSP || frame.channel == CHANNEL_CONTROL {
                self.park(frame.channel, frame.payload);
                continue;
            }
            return Err(self.fail(IdeError::lsp(format!(
                "unknown mux channel {}",
                frame.channel
            ))));
        }
    }

    fn try_read_channel(&self, want: u8) -> Result<Option<Vec<u8>>, IdeError> {
        self.check_failed()?;
        let inbox = if want == CHANNEL_LSP {
            &self.lsp_inbox
        } else {
            &self.control_inbox
        };
        if let Some(payload) = Self::take_inbox(inbox) {
            return Ok(Some(payload));
        }
        Ok(None)
    }
}

/// Owns the docker/child pipes and splits into [`MuxLsp`] + [`MuxControl`].
pub struct MuxStdio {
    inner: Arc<MuxInner>,
    child: Option<Child>,
    stderr_drain: Option<Arc<ChildStderrDrain>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl MuxStdio {
    pub fn from_command(cmd: Command) -> Result<Self, IdeError> {
        let stdio = StdioLsp::from_command(cmd)?;
        Ok(Self::from_stdio(stdio))
    }

    pub fn from_pair(
        writer: impl Write + Send + 'static,
        reader: impl Read + Send + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(MuxInner::new(writer, reader)),
            child: None,
            stderr_drain: None,
            stderr_thread: None,
        }
    }

    fn from_stdio(stdio: StdioLsp) -> Self {
        let (child, writer, reader, stderr_drain, stderr_thread) = stdio.into_io_parts();
        Self {
            inner: Arc::new(MuxInner::new(writer, reader)),
            child,
            stderr_drain,
            stderr_thread,
        }
    }

    pub fn split(self) -> (MuxLsp, MuxControl) {
        let inner = Arc::clone(&self.inner);
        let control = MuxControl {
            inner: Arc::clone(&inner),
            pushes: Vec::new(),
        };
        let lsp = MuxLsp {
            inner,
            next_id: 1,
            notifications: Vec::new(),
            stderr_drain: self.stderr_drain.clone(),
            _owner: MuxOwner {
                child: self.child,
                stderr_thread: self.stderr_thread,
            },
        };
        (lsp, control)
    }

    pub fn lsp(self) -> MuxLsp {
        self.split().0
    }

    pub fn control(&self) -> MuxControl {
        MuxControl {
            inner: Arc::clone(&self.inner),
            pushes: Vec::new(),
        }
    }
}

struct MuxOwner {
    child: Option<Child>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl Drop for MuxOwner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

/// LSP half of [`MuxStdio`]. Channel 0. Adapter.
pub struct MuxLsp {
    inner: Arc<MuxInner>,
    next_id: i64,
    notifications: Vec<Value>,
    stderr_drain: Option<Arc<ChildStderrDrain>>,
    _owner: MuxOwner,
}

impl MuxLsp {
    pub fn from_pair(
        writer: impl Write + Send + 'static,
        reader: impl Read + Send + 'static,
    ) -> Self {
        MuxStdio::from_pair(writer, reader).lsp()
    }

    pub fn next_id(&self) -> i64 {
        self.next_id
    }

    pub fn stderr_drain(&self) -> Option<Arc<ChildStderrDrain>> {
        self.stderr_drain.clone()
    }

    pub fn take_notifications(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.notifications)
    }

    pub fn notification_len(&self) -> usize {
        self.notifications.len()
    }

    fn write_json(&self, value: &Value) -> Result<(), IdeError> {
        let body = serde_json::to_vec(value).map_err(|e| IdeError::lsp(e.to_string()))?;
        self.inner.write_channel(CHANNEL_LSP, &body)
    }
}

impl std::fmt::Debug for MuxLsp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuxLsp")
            .field("next_id", &self.next_id)
            .field("has_stderr_drain", &self.stderr_drain.is_some())
            .finish()
    }
}

impl LspTransport for MuxLsp {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, IdeError> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_json(&msg)?;
        loop {
            let body = self.inner.read_channel(CHANNEL_LSP)?;
            let v: Value =
                serde_json::from_slice(&body).map_err(|e| IdeError::lsp(e.to_string()))?;
            if v.get("id").is_none() {
                self.notifications.push(v);
                continue;
            }
            if v.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(err) = v.get("error") {
                let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                if code == METHOD_NOT_FOUND {
                    return Err(IdeError::lsp_method_missing(method));
                }
                return Err(IdeError::lsp(err.to_string()));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), IdeError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_json(&msg)
    }
}

/// Control half of [`MuxStdio`]. Channel 1. Adapter.
pub struct MuxControl {
    inner: Arc<MuxInner>,
    pushes: Vec<Envelope>,
}

impl MuxControl {
    pub fn from_pair(
        writer: impl Write + Send + 'static,
        reader: impl Read + Send + 'static,
    ) -> Self {
        MuxStdio::from_pair(writer, reader).control()
    }

    fn write_envelope(&self, env: &Envelope) -> Result<(), IdeError> {
        let inner = encode_frame(&env.to_bytes()).map_err(|e| IdeError::control(e.to_string()))?;
        self.inner
            .write_channel(CHANNEL_CONTROL, &inner)
            .map_err(|e| IdeError::control(e.to_string()))
    }

    fn decode_control_payload(&self, payload: &[u8]) -> Result<Envelope, IdeError> {
        let proto = match decode_frame(payload) {
            Ok(DecodeOutcome::Complete {
                payload: proto,
                consumed,
            }) if consumed == payload.len() => proto,
            Ok(other) => {
                return Err(IdeError::control(format!(
                    "incomplete control frame {other:?}"
                )));
            }
            Err(e) => return Err(IdeError::control(e.to_string())),
        };
        Envelope::from_bytes(&proto).map_err(|e| IdeError::control(e.to_string()))
    }

    fn ingest(&mut self, env: Envelope, want: Option<u64>) -> Option<Envelope> {
        if env.request_id == 0 {
            self.pushes.push(env);
            return None;
        }
        if want == Some(env.request_id) {
            return Some(env);
        }
        None
    }
}

impl std::fmt::Debug for MuxControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuxControl")
            .field("pushes", &self.pushes.len())
            .finish()
    }
}

impl ControlTransport for MuxControl {
    fn send(&mut self, request: Envelope) -> Result<Envelope, IdeError> {
        if request.body.len() as u32 > MAX_MUX_PAYLOAD {
            return Err(IdeError::control(format!(
                "payload exceeds {MAX_MUX_PAYLOAD} bytes ({})",
                request.body.len()
            )));
        }
        let want = request.request_id;
        self.write_envelope(&request)?;
        loop {
            let payload = self
                .inner
                .read_channel(CHANNEL_CONTROL)
                .map_err(map_control_mux_io)?;
            let env = self.decode_control_payload(&payload)?;
            if let Some(reply) = self.ingest(env, Some(want)) {
                return Ok(reply);
            }
        }
    }

    fn take_pushes(&mut self) -> Vec<Envelope> {
        std::mem::take(&mut self.pushes)
    }

    fn poll(&mut self) -> Result<(), IdeError> {
        while let Some(payload) = self.inner.try_read_channel(CHANNEL_CONTROL)? {
            let env = self.decode_control_payload(&payload)?;
            let _ = self.ingest(env, None);
        }
        Ok(())
    }

    fn wait(&mut self) -> Result<(), IdeError> {
        if !self.pushes.is_empty() {
            return Ok(());
        }
        let payload = self
            .inner
            .read_channel(CHANNEL_CONTROL)
            .map_err(map_control_mux_io)?;
        let env = self.decode_control_payload(&payload)?;
        let _ = self.ingest(env, None);
        Ok(())
    }
}

fn map_control_mux_io(err: IdeError) -> IdeError {
    if err.is_lsp() {
        IdeError::control(err.to_string())
    } else {
        err
    }
}

/// Encode opaque JSON-RPC bytes as a channel-0 frame (tests + pair scripts).
pub fn encode_lsp_json(value: &Value) -> Result<Vec<u8>, IdeError> {
    let body = serde_json::to_vec(value).map_err(|e| IdeError::lsp(e.to_string()))?;
    encode_mux_frame(CHANNEL_LSP, &body).map_err(map_mux)
}

/// Encode a length-prefixed Envelope as a channel-1 frame.
pub fn encode_control_envelope(env: &Envelope) -> Result<Vec<u8>, IdeError> {
    let inner = encode_frame(&env.to_bytes()).map_err(|e| IdeError::control(e.to_string()))?;
    encode_mux_frame(CHANNEL_CONTROL, &inner).map_err(map_control_mux)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::ServeMode;
    use crate::lsp::LspClient;
    use progressive_lsp_control::{GetConfigResponse, METHOD_GET_CONFIG};
    use progressive_lsp_protocol::{decode_mux_frame, CHANNEL_CONTROL, CHANNEL_LSP};
    use std::io::Cursor;

    fn init_result_mux() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "capabilities": {
                    "experimental": {
                        "progressiveLsp": {
                            "version": "v1",
                            "socket": null,
                            "mux": true
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn mux_stdio_adapter_lsp_channel_zero_jsonrpc_without_content_length() {
        let framed = encode_lsp_json(&init_result_mux()).unwrap();
        assert_eq!(framed[0], CHANNEL_LSP);
        let (frame, _) = decode_mux_frame(&framed).unwrap().unwrap();
        assert!(frame.is_lsp());
        assert!(!frame.payload.starts_with(b"Content-Length"));
        assert!(std::str::from_utf8(&frame.payload)
            .unwrap()
            .contains("\"mux\":true"));

        let mut client = LspClient::new(MuxLsp::from_pair(Vec::new(), Cursor::new(framed)))
            .with_mode(ServeMode::Mux);
        client.initialize("/ws").unwrap();
        let cap = client.progressive_cap().unwrap();
        assert!(cap.mux());
        assert!(cap.socket().is_none());
        assert_eq!(cap.version(), "v1");
        assert_eq!(client.serve_mode(), ServeMode::Mux);
    }

    #[test]
    fn mux_control_adapter_channel_one_length_prefixed_proto() {
        let reply = Envelope::reply(METHOD_GET_CONFIG, 1, GetConfigResponse::default());
        let framed = encode_control_envelope(&reply).unwrap();
        assert_eq!(framed[0], CHANNEL_CONTROL);
        let (frame, _) = decode_mux_frame(&framed).unwrap().unwrap();
        assert!(frame.is_control());
        let proto = match decode_frame(&frame.payload).unwrap() {
            DecodeOutcome::Complete { payload, consumed } if consumed == frame.payload.len() => {
                payload
            }
            other => panic!("{other:?}"),
        };
        let env = Envelope::from_bytes(&proto).unwrap();
        assert_eq!(env.request_id, 1);
        assert_eq!(env.method, METHOD_GET_CONFIG);

        let mut ctl = MuxControl::from_pair(Vec::new(), Cursor::new(framed));
        let got = ctl
            .send(Envelope::request(
                METHOD_GET_CONFIG,
                1,
                GetConfigResponse::default(),
            ))
            .unwrap();
        assert_eq!(got.request_id, 1);
        assert_eq!(got.method, METHOD_GET_CONFIG);
        assert!(ctl.take_pushes().is_empty());
    }

    #[test]
    fn mux_stdio_adapter_parks_other_channel_without_sleep() {
        let lsp = encode_lsp_json(&init_result_mux()).unwrap();
        let push = Envelope::push(
            progressive_lsp_control::METHOD_WATCH_BATCH,
            progressive_lsp_control::WatchBatch::default(),
        );
        let control = encode_control_envelope(&push).unwrap();
        let mut stream = control;
        stream.extend_from_slice(&lsp);

        let mux = MuxStdio::from_pair(Vec::new(), Cursor::new(stream));
        let (mut lsp_half, mut ctl) = mux.split();
        let result = lsp_half
            .request(
                "initialize",
                json!({"rootUri": "file:///ws", "capabilities": {}}),
            )
            .unwrap();
        assert_eq!(
            result["capabilities"]["experimental"]["progressiveLsp"]["mux"],
            true
        );
        ctl.poll().unwrap();
        assert_eq!(ctl.take_pushes().len(), 1);
        assert_eq!(format!("{:?}", lsp_half).contains("MuxLsp"), true);
        assert_eq!(format!("{:?}", ctl).contains("MuxControl"), true);
        assert_eq!(lsp_half.next_id(), 2);
        assert_eq!(lsp_half.notification_len(), 0);
        assert!(lsp_half.stderr_drain().is_none());
        assert!(lsp_half.take_notifications().is_empty());
    }

    #[test]
    fn mux_stdio_adapter_fails_closed_unknown_channel_and_too_large() {
        let unknown = vec![9, 0, 0, 0, 0];
        let mut lsp = MuxLsp::from_pair(Vec::new(), Cursor::new(unknown));
        let err = lsp.request("initialize", json!({})).unwrap_err();
        assert!(err.is_lsp());
        assert!(err.to_string().contains("unknown mux channel"));

        let too = MAX_MUX_PAYLOAD + 1;
        let mut hdr = vec![CHANNEL_LSP];
        hdr.extend_from_slice(&too.to_be_bytes());
        let mut lsp2 = MuxLsp::from_pair(Vec::new(), Cursor::new(hdr));
        let big = lsp2.request("initialize", json!({})).unwrap_err();
        assert!(big.to_string().contains("exceeds"));

        let huge_env = Envelope {
            method: METHOD_GET_CONFIG.into(),
            request_id: 1,
            body: vec![0u8; (MAX_MUX_PAYLOAD as usize) + 1],
            ..Envelope::default()
        };
        let mut ctl = MuxControl::from_pair(Vec::new(), Cursor::new(Vec::<u8>::new()));
        assert!(ctl.send(huge_env).unwrap_err().is_control());
    }

    #[test]
    fn mux_lsp_notify_and_method_missing_and_eof() {
        let missing = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}});
        let mut lsp =
            MuxLsp::from_pair(Vec::new(), Cursor::new(encode_lsp_json(&missing).unwrap()));
        assert!(lsp
            .request("textDocument/implementation", json!({}))
            .unwrap_err()
            .is_lsp_method_missing());

        let note = json!({"jsonrpc":"2.0","method":"$/progress","params":{}});
        let ok = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
        let mut stream = encode_lsp_json(&note).unwrap();
        stream.extend_from_slice(&encode_lsp_json(&ok).unwrap());
        let mut lsp = MuxLsp::from_pair(Vec::new(), Cursor::new(stream));
        assert_eq!(lsp.request("ping", json!({})).unwrap()["ok"], true);
        assert_eq!(lsp.take_notifications().len(), 1);
        lsp.notify("initialized", json!({})).unwrap();

        let mut empty = MuxLsp::from_pair(Vec::new(), Cursor::new(Vec::<u8>::new()));
        assert!(empty.request("initialize", json!({})).unwrap_err().is_lsp());
    }

    #[test]
    fn mux_stdio_adapter_from_command_and_control_wait_without_sleep() {
        let true_bin = std::path::Path::new("/usr/bin/true");
        if true_bin.is_file() {
            let mut cmd = std::process::Command::new(true_bin);
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mux = MuxStdio::from_command(cmd).unwrap();
            let ctl = mux.control();
            let lsp = mux.lsp();
            drop(ctl);
            drop(lsp);
        }

        let push = Envelope::push(
            progressive_lsp_control::METHOD_WATCH_BATCH,
            progressive_lsp_control::WatchBatch::default(),
        );
        let framed = encode_control_envelope(&push).unwrap();
        let mut ctl = MuxControl::from_pair(Vec::new(), Cursor::new(framed));
        ctl.wait().unwrap();
        ctl.wait().unwrap();
        assert_eq!(ctl.take_pushes().len(), 1);
        assert!(ctl.poll().is_ok());
    }
}
