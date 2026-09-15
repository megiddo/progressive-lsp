//! Docker `serve --mux` stdin/stdout driver shared by discover-container and tam-run.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use progressive_lsp_control::{
    decode_frame, encode_frame, DecodeOutcome, Envelope, FetchTraceRequest, FetchTraceResponse,
    METHOD_FETCH_TRACE, METHOD_INDEX_STATUS, METHOD_TIER_STATUS, IndexStatusRequest,
    IndexStatusResponse, TierStatusRequest, TierStatusResponse,
};
use progressive_lsp_protocol::{read_mux_frame, rpc, write_mux_frame, CHANNEL_CONTROL, CHANNEL_LSP};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct DockerMuxOpts {
    pub docker: PathBuf,
    pub image: String,
    pub platform: Option<String>,
    pub wal: Option<PathBuf>,
    pub mount_serve_bin: Option<PathBuf>,
    pub prefix: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceStep {
    pub phase: String,
    pub elapsed_ms: u64,
    pub detail: String,
}

struct MuxInbox {
    lsp: VecDeque<Vec<u8>>,
    control: VecDeque<Vec<u8>>,
    failed: Option<String>,
    reader_done: bool,
}

pub struct MuxDriver {
    writer: Mutex<Box<dyn Write + Send>>,
    inbox: Arc<(Mutex<MuxInbox>, Condvar)>,
    pub start: Instant,
    verbose: bool,
    trace: Mutex<Vec<TraceStep>>,
    _child: Child,
    _stderr: Option<thread::JoinHandle<()>>,
    _reader: thread::JoinHandle<()>,
}

impl MuxDriver {
    pub fn trace(&self, phase: &str, detail: impl Into<String>) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let detail = detail.into();
        if self.verbose {
            eprintln!("[mux +{elapsed_ms}ms] {phase}: {detail}");
        }
        if let Ok(mut t) = self.trace.lock() {
            t.push(TraceStep {
                phase: phase.to_string(),
                elapsed_ms,
                detail,
            });
        }
    }

    pub fn write_channel(&self, channel: u8, payload: &[u8]) -> Result<(), String> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| "writer lock poisoned".to_string())?;
        write_mux_frame(&mut *w, channel, payload).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())
    }

    pub fn read_channel_until(&self, channel: u8, deadline: Instant) -> Result<Vec<u8>, String> {
        let (lock, cv) = &*self.inbox;
        let mut guard = lock
            .lock()
            .map_err(|_| "inbox lock poisoned".to_string())?;
        loop {
            if let Some(msg) = guard.failed.clone() {
                return Err(msg);
            }
            let q = if channel == CHANNEL_LSP {
                &mut guard.lsp
            } else {
                &mut guard.control
            };
            if let Some(p) = q.pop_front() {
                return Ok(p);
            }
            if guard.reader_done {
                return Err("mux peer eof".into());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(format!(
                    "deadline waiting for mux channel {channel} (cap {:?})",
                    deadline.saturating_duration_since(self.start)
                ));
            }
            let wait = deadline.saturating_duration_since(now);
            guard = cv
                .wait_timeout(guard, wait)
                .map_err(|_| "condvar poisoned".to_string())?
                .0;
        }
    }

    pub fn request_lsp(
        &self,
        id: i64,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, String> {
        self.trace("lsp_request", format!("id={id} method={method}"));
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        self.write_channel(CHANNEL_LSP, &body)?;
        loop {
            let raw = self.read_channel_until(CHANNEL_LSP, deadline)?;
            let v: Value = serde_json::from_slice(&raw).map_err(|e| format!("json: {e}"))?;
            if v.get("method").is_some() && v.get("id").is_none() {
                let m = v.get("method").and_then(|x| x.as_str()).unwrap_or("?");
                self.trace("lsp_notification", m.to_string());
                continue;
            }
            if let Some(got) = v.get("id") {
                if !rpc::id_matches(id, got) {
                    self.trace(
                        "lsp_skip_id",
                        format!("want {id} got {got} body={}", truncate_json(&v, 200)),
                    );
                    continue;
                }
            } else {
                continue;
            }
            if let Some(err) = v.get("error") {
                return Err(format!("LSP error on {method}: {err}"));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    pub fn notify_lsp(&self, method: &str, params: Value) -> Result<(), String> {
        self.trace("lsp_notify", method.to_string());
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        self.write_channel(CHANNEL_LSP, &body)
    }

    pub fn control_rpc<Req, Resp>(
        &self,
        method: &str,
        request_id: u64,
        req: Req,
        deadline: Instant,
    ) -> Result<Resp, String>
    where
        Req: progressive_lsp_control::prost::Message,
        Resp: progressive_lsp_control::prost::Message + Default,
    {
        self.trace("control_request", format!("id={request_id} method={method}"));
        let env = Envelope::request(method, request_id, req);
        let frame = encode_frame(&env.to_bytes()).map_err(|e| e.to_string())?;
        self.write_channel(CHANNEL_CONTROL, &frame)?;
        loop {
            if Instant::now() >= deadline {
                return Err("control rpc deadline exceeded".into());
            }
            let raw = self.read_channel_until(CHANNEL_CONTROL, deadline)?;
            let env = decode_envelope_payload(&raw)?;
            if env.request_id == 0 {
                self.trace("control_push", env.method.clone());
                continue;
            }
            if env.request_id == request_id && env.method == method {
                return env.decode_body::<Resp>().map_err(|e| e.to_string());
            }
            self.trace(
                "control_skip",
                format!("id={} method={}", env.request_id, env.method),
            );
        }
    }

    pub fn fetch_trace(
        &self,
        trace_id: &str,
        max_rows: u32,
        request_id: u64,
        deadline: Instant,
    ) -> Result<FetchTraceResponse, String> {
        self.control_rpc::<FetchTraceRequest, FetchTraceResponse>(
            METHOD_FETCH_TRACE,
            request_id,
            FetchTraceRequest {
                trace_id: trace_id.to_string(),
                max_rows,
            },
            deadline,
        )
    }

    pub fn index_status_with_meta(
        &self,
        emit_result_meta: bool,
        emit_timing: bool,
        request_id: u64,
        deadline: Instant,
    ) -> Result<IndexStatusResponse, String> {
        self.control_rpc::<IndexStatusRequest, IndexStatusResponse>(
            METHOD_INDEX_STATUS,
            request_id,
            IndexStatusRequest {
                emit_result_meta,
                emit_timing,
            },
            deadline,
        )
    }

    pub fn tier_status(
        &self,
        request_id: u64,
        deadline: Instant,
    ) -> Result<TierStatusResponse, String> {
        self.control_rpc::<TierStatusRequest, TierStatusResponse>(
            METHOD_TIER_STATUS,
            request_id,
            TierStatusRequest {},
            deadline,
        )
    }

    pub fn take_trace(&self) -> Vec<TraceStep> {
        self.trace.lock().map(|t| t.clone()).unwrap_or_default()
    }
}

pub fn spawn_mux_docker(
    workspace_root: &Path,
    docker: &DockerMuxOpts,
    verbose: bool,
) -> Result<MuxDriver, String> {
    let root = workspace_root
        .canonicalize()
        .map_err(|e| format!("root {}: {e}", workspace_root.display()))?;
    if !root.is_absolute() {
        return Err("workspace root must be absolute".into());
    }
    let ws = root.to_string_lossy().into_owned();
    let mut argv = vec!["run".into(), "-i".into(), "--rm".into()];
    if let Some(p) = &docker.platform {
        argv.push("--platform".into());
        argv.push(p.clone());
    }
    argv.extend(["-v".into(), format!("{ws}:{ws}"), "-w".into(), ws.clone()]);
    if let Some(bin) = &docker.mount_serve_bin {
        if bin.is_file() {
            let b = bin
                .canonicalize()
                .map_err(|e| format!("mount-serve-bin {}: {e}", bin.display()))?
                .to_string_lossy()
                .into_owned();
            argv.extend([
                "-v".into(),
                format!("{b}:/opt/plsp/bin/progressive-lsp:ro"),
            ]);
        }
    }
    if let Some(wal) = &docker.wal {
        if wal.is_absolute() {
            if let Some(dir) = wal.parent() {
                let dir_s = dir.to_string_lossy().into_owned();
                if dir_s != ws {
                    argv.extend(["-v".into(), format!("{dir_s}:{dir_s}")]);
                }
            }
            argv.extend([
                "-e".into(),
                format!("PROGRESSIVE_LSP_LOG={}", wal.display()),
                "-e".into(),
                "PROGRESSIVE_LSP_LOG_LEVEL=debug".into(),
            ]);
        }
    }
    argv.extend([
        docker.image.clone(),
        "serve".into(),
        "--prefix".into(),
        docker.prefix.clone(),
        "--mux".into(),
    ]);

    verbose.then(|| {
        eprintln!(
            "[mux] docker {} …",
            argv[..argv.len().min(8)].join(" ")
        );
    });

    let mut cmd = Command::new(&docker.docker);
    for arg in &argv {
        cmd.arg(arg);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("docker spawn: {e}"))?;

    let stdin = child.stdin.take().ok_or("stdin")?;
    let stdout = child.stdout.take().ok_or("stdout")?;
    let stderr = child.stderr.take().ok_or("stderr")?;

    let inbox = Arc::new((
        Mutex::new(MuxInbox {
            lsp: VecDeque::new(),
            control: VecDeque::new(),
            failed: None,
            reader_done: false,
        }),
        Condvar::new(),
    ));

    let reader_inbox = Arc::clone(&inbox);
    let reader = thread::spawn(move || {
        let mut reader = stdout;
        loop {
            match read_mux_frame(&mut reader) {
                Ok(Some(frame)) => {
                    let (lock, cv) = &*reader_inbox;
                    if let Ok(mut st) = lock.lock() {
                        if frame.channel == CHANNEL_LSP {
                            st.lsp.push_back(frame.payload);
                        } else if frame.channel == CHANNEL_CONTROL {
                            st.control.push_back(frame.payload);
                        }
                        cv.notify_all();
                    }
                }
                Ok(None) => {
                    let (lock, cv) = &*reader_inbox;
                    if let Ok(mut st) = lock.lock() {
                        st.reader_done = true;
                    }
                    cv.notify_all();
                    break;
                }
                Err(e) => {
                    let (lock, cv) = &*reader_inbox;
                    if let Ok(mut st) = lock.lock() {
                        st.failed = Some(e.to_string());
                    }
                    cv.notify_all();
                    break;
                }
            }
        }
    });

    let stderr_thread = thread::spawn(move || {
        let mut r = stderr;
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::stderr().write_all(&buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    Ok(MuxDriver {
        writer: Mutex::new(Box::new(stdin)),
        inbox,
        start: Instant::now(),
        verbose,
        trace: Mutex::new(Vec::new()),
        _child: child,
        _stderr: Some(stderr_thread),
        _reader: reader,
    })
}

pub fn docker_available(docker: &Path) -> bool {
    Command::new(docker)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn docker_image_present(docker: &Path, image: &str) -> bool {
    Command::new(docker)
        .args(["image", "inspect", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn decode_envelope_payload(raw: &[u8]) -> Result<Envelope, String> {
    match decode_frame(raw) {
        Ok(DecodeOutcome::Complete { payload, .. }) => {
            Envelope::from_bytes(&payload).map_err(|e| e.to_string())
        }
        Ok(DecodeOutcome::Incomplete { .. }) => Err("incomplete control frame in mux payload".into()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn truncate_json(v: &Value, max: usize) -> String {
    let s = v.to_string();
    if s.len() <= max {
        s
    } else {
        format!("{}…", &s[..max])
    }
}

pub fn progressive_lsp_json(opts: &crate::tam::TamProgressiveLsp) -> Value {
    let mut o = serde_json::Map::new();
    if let Some(v) = opts.max_chain_iter {
        o.insert("maxChainIter".into(), json!(v));
    }
    if let Some(ref t) = opts.max_tier {
        o.insert("maxTier".into(), json!(t));
    }
    if let Some(v) = opts.emit_result_meta {
        o.insert("emitResultMeta".into(), json!(v));
    }
    if let Some(v) = opts.emit_timing {
        o.insert("emitTiming".into(), json!(v));
    }
    Value::Object(o)
}

pub fn init_params_with_progressive(
    root_uri: &str,
    session: Option<&crate::tam::TamProgressiveLsp>,
) -> Value {
    let mut progressive = json!({});
    if let Some(s) = session {
        progressive = progressive_lsp_json(s);
    }
    json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {},
        "initializationOptions": {
            "progressiveLsp": progressive
        }
    })
}

pub fn trace_rows_to_json(rows: &[progressive_lsp_control::TraceRow], cap: usize) -> Value {
    let slice = if rows.len() > cap {
        &rows[rows.len() - cap..]
    } else {
        rows
    };
    json!(slice
        .iter()
        .map(|r| {
            json!({
                "unix_ms": r.unix_ms,
                "level": r.level,
                "component": r.component,
                "operation": r.operation,
                "message": r.message,
            })
        })
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_control::{TraceRow, METHOD_FETCH_TRACE};

    #[test]
    fn fetch_trace_request_encodes() {
        let req = FetchTraceRequest {
            trace_id: "abc".into(),
            max_rows: 10,
        };
        let env = Envelope::request(METHOD_FETCH_TRACE, 1, req);
        assert!(!env.to_bytes().is_empty());
    }

    #[test]
    fn trace_rows_json_caps() {
        let rows: Vec<TraceRow> = (0..5)
            .map(|i| TraceRow {
                unix_ms: i,
                level: "info".into(),
                component: "c".into(),
                operation: "op".into(),
                message: format!("m{i}"),
                extras: Default::default(),
            })
            .collect();
        let v = trace_rows_to_json(&rows, 2);
        assert_eq!(v.as_array().unwrap().len(), 2);
    }
}
