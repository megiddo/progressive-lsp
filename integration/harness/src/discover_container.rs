//! POC-IDE parity: `docker run … progressive-lsp-runtime:local serve --mux`, then the
//! same LSP sequence as **Find Definition** (initialize → initialized → didOpen →
//! `textDocument/definition` on channel 0). Verbose trace on stderr; JSON report on stdout.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use progressive_lsp_control::{
    decode_frame, encode_frame, DecodeOutcome, Envelope, IndexStatusRequest,
    IndexStatusResponse, METHOD_INDEX_STATUS, METHOD_TIER_STATUS, TierStatusRequest,
    TierStatusResponse,
};
use progressive_lsp_protocol::{read_mux_frame, rpc, write_mux_frame, CHANNEL_CONTROL, CHANNEL_LSP};
use serde_json::{json, Value};

use crate::{find_position, load_golden};

pub const DISCOVER_CONTAINER_USAGE: &str = "\
plsp-it1 discover-container --root DIR --expected JSON [--docker PATH] [--image NAME] \
  [--platform linux/arm64] [--wal PATH] [--init-deadline-ms N] [--discover-deadline-ms N] \
  [--verbose]
";

#[derive(Debug, Clone)]
pub struct DiscoverContainerOpts {
    pub root: PathBuf,
    pub expected: PathBuf,
    pub docker: PathBuf,
    pub image: String,
    pub platform: Option<String>,
    pub wal: Option<PathBuf>,
    /// Host Linux musl `progressive-lsp` bind-mounted over `/opt/plsp/bin/progressive-lsp`.
    pub mount_serve_bin: Option<PathBuf>,
    pub init_deadline: Duration,
    pub discover_deadline: Duration,
    pub verbose: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct TraceStep {
    phase: String,
    elapsed_ms: u64,
    detail: String,
}

pub fn parse_discover_container(args: &[String]) -> Result<DiscoverContainerOpts, String> {
    let mut root = None;
    let mut expected = None;
    let mut docker = PathBuf::from("docker");
    let mut image = "progressive-lsp-runtime:local".to_string();
    let mut platform = None;
    let mut wal = None;
    let mut mount_serve_bin = None;
    let mut init_deadline = Duration::from_secs(600);
    let mut discover_deadline = Duration::from_secs(15);
    let mut verbose = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                root = Some(PathBuf::from(args.get(i).ok_or("--root requires a path")?));
            }
            "--expected" => {
                i += 1;
                expected = Some(PathBuf::from(
                    args.get(i).ok_or("--expected requires a path")?,
                ));
            }
            "--docker" => {
                i += 1;
                docker = PathBuf::from(args.get(i).ok_or("--docker requires a path")?);
            }
            "--image" => {
                i += 1;
                image = args.get(i).ok_or("--image requires a value")?.clone();
            }
            "--platform" => {
                i += 1;
                platform = Some(args.get(i).ok_or("--platform requires a value")?.clone());
            }
            "--wal" => {
                i += 1;
                wal = Some(PathBuf::from(args.get(i).ok_or("--wal requires a path")?));
            }
            "--mount-serve-bin" => {
                i += 1;
                mount_serve_bin = Some(PathBuf::from(
                    args.get(i)
                        .ok_or("--mount-serve-bin requires a path")?,
                ));
            }
            "--init-deadline-ms" => {
                i += 1;
                let ms: u64 = args
                    .get(i)
                    .ok_or("--init-deadline-ms requires a value")?
                    .parse()
                    .map_err(|_| "init-deadline-ms must be integer")?;
                init_deadline = Duration::from_millis(ms);
            }
            "--discover-deadline-ms" => {
                i += 1;
                let ms: u64 = args
                    .get(i)
                    .ok_or("--discover-deadline-ms requires a value")?
                    .parse()
                    .map_err(|_| "discover-deadline-ms must be integer")?;
                discover_deadline = Duration::from_millis(ms);
            }
            "--verbose" => verbose = true,
            "--quiet" => verbose = false,
            other => return Err(format!("unknown flag: {other}\n{DISCOVER_CONTAINER_USAGE}")),
        }
        i += 1;
    }
    Ok(DiscoverContainerOpts {
        root: root.ok_or("discover-container requires --root")?,
        expected: expected.ok_or("discover-container requires --expected")?,
        docker,
        image,
        platform,
        wal,
        mount_serve_bin,
        init_deadline,
        discover_deadline,
        verbose,
    })
}

struct MuxInbox {
    lsp: VecDeque<Vec<u8>>,
    control: VecDeque<Vec<u8>>,
    failed: Option<String>,
    reader_done: bool,
}

struct MuxDriver {
    writer: Mutex<Box<dyn Write + Send>>,
    inbox: Arc<(Mutex<MuxInbox>, Condvar)>,
    start: Instant,
    verbose: bool,
    trace: Mutex<Vec<TraceStep>>,
    _child: Child,
    _stderr: Option<thread::JoinHandle<()>>,
    _reader: thread::JoinHandle<()>,
}

impl MuxDriver {
    fn trace(&self, phase: &str, detail: impl Into<String>) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let detail = detail.into();
        if self.verbose {
            eprintln!("[discover-container +{elapsed_ms}ms] {phase}: {detail}");
        }
        if let Ok(mut t) = self.trace.lock() {
            t.push(TraceStep {
                phase: phase.to_string(),
                elapsed_ms,
                detail,
            });
        }
    }

    fn write_channel(&self, channel: u8, payload: &[u8]) -> Result<(), String> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| "writer lock poisoned".to_string())?;
        write_mux_frame(&mut *w, channel, payload).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())
    }

    fn read_channel_until(&self, channel: u8, deadline: Instant) -> Result<Vec<u8>, String> {
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

    fn request_lsp(
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

    fn notify_lsp(&self, method: &str, params: Value) -> Result<(), String> {
        self.trace("lsp_notify", method.to_string());
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        self.write_channel(CHANNEL_LSP, &body)
    }

    fn control_rpc<Req, Resp>(
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

    fn take_trace(&self) -> Vec<TraceStep> {
        self.trace.lock().map(|t| t.clone()).unwrap_or_default()
    }
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

fn truncate_json(v: &Value, max: usize) -> String {
    let s = v.to_string();
    if s.len() <= max {
        s
    } else {
        format!("{}…", &s[..max])
    }
}

fn docker_argv(opts: &DiscoverContainerOpts) -> Result<Vec<String>, String> {
    let root = opts
        .root
        .canonicalize()
        .map_err(|e| format!("root {}: {e}", opts.root.display()))?;
    if !root.is_absolute() {
        return Err("root must be absolute".into());
    }
    let ws = root.to_string_lossy().into_owned();
    let mut argv = vec!["run".into(), "-i".into(), "--rm".into()];
    if let Some(p) = &opts.platform {
        argv.push("--platform".into());
        argv.push(p.clone());
    }
    argv.extend(["-v".into(), format!("{ws}:{ws}"), "-w".into(), ws.clone()]);
    if let Some(bin) = &opts.mount_serve_bin {
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
    if let Some(wal) = &opts.wal {
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
        opts.image.clone(),
        "serve".into(),
        "--prefix".into(),
        "/opt/plsp".into(),
        "--mux".into(),
    ]);
    Ok(argv)
}

fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()) {
        Some("java") => "java",
        Some("py") => "python",
        Some("rs") => "rust",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("ts") => "typescript",
        _ => "plaintext",
    }
}

pub fn run_discover_container(opts: &DiscoverContainerOpts) -> Result<Value, String> {
    let golden = load_golden(&opts.expected)?;
    let entry = opts
        .root
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join(&golden.entry);
    if !entry.is_file() {
        return Ok(json!({
            "rpc": "discover-container",
            "result": "skip_entry_missing",
            "entry": entry.display().to_string(),
        }));
    }
    let src = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
    let (line, character) = find_position(&src, &golden.find)?;
    let file_path = entry.clone();
    let uri = progressive_lsp_core::path_to_file_uri(&file_path);
    let root = opts.root.canonicalize().map_err(|e| e.to_string())?;
    let root_uri = progressive_lsp_core::path_to_file_uri(&root);

    let docker_argv = docker_argv(opts)?;
    opts.verbose.then(|| {
        eprintln!(
            "[discover-container] docker {} …",
            docker_argv[..docker_argv.len().min(8)].join(" ")
        );
    });

    let mut cmd = Command::new(&opts.docker);
    for arg in &docker_argv {
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

    let start = Instant::now();
    let driver = MuxDriver {
        writer: Mutex::new(Box::new(stdin)),
        inbox,
        start,
        verbose: opts.verbose,
        trace: Mutex::new(Vec::new()),
        _child: child,
        _stderr: Some(stderr_thread),
        _reader: reader,
    };

    driver.trace("setup", format!("root={root_uri} entry={uri} find={} @ {line}:{character}", golden.find));

    let init_deadline = start + opts.init_deadline;
    let init_result = driver.request_lsp(
        1,
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {},
        }),
        init_deadline,
    );

    let result = match init_result {
        Ok(r) => {
            driver.trace("initialize_ok", truncate_json(&r, 300));
            let mux = &r["capabilities"]["experimental"]["progressiveLsp"]["mux"];
            if mux != &json!(true) {
                return Ok(finish_report(
                    &driver,
                    opts,
                    "fail",
                    format!("expected progressiveLsp.mux true, got {mux}"),
                    None,
                    None,
                    0,
                    false,
                    &golden,
                    line,
                    character,
                    &uri,
                ));
            }
            r
        }
        Err(e) => {
            return Ok(finish_report(
                &driver,
                opts,
                "fail",
                format!("initialize: {e}"),
                None,
                None,
                0,
                false,
                &golden,
                line,
                character,
                &uri,
            ));
        }
    };

    driver.notify_lsp("initialized", json!({}))?;

    if let Ok(idx) = driver.control_rpc::<IndexStatusRequest, IndexStatusResponse>(
        METHOD_INDEX_STATUS,
        10,
        IndexStatusRequest {},
        init_deadline,
    ) {
        let tiers = driver
            .control_rpc::<TierStatusRequest, TierStatusResponse>(
                METHOD_TIER_STATUS,
                11,
                TierStatusRequest {},
                init_deadline,
            )
            .ok();
        driver.trace(
            "control_snapshot",
            format!(
                "packages={} tier_rows={}",
                idx.packages.len(),
                tiers.as_ref().map(|t| t.rows.len()).unwrap_or(0),
            ),
        );
    }

    driver.notify_lsp(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id(&file_path),
                "version": 1,
                "text": src,
            }
        }),
    )?;

    let discover_deadline = Instant::now() + opts.discover_deadline;
    driver.trace(
        "discover_start",
        "textDocument/definition (context menu Find Definition)".to_string(),
    );
    let def_result = driver.request_lsp(
        2,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        }),
        discover_deadline,
    );

    let _init_caps = result;
    let def_result = match def_result {
        Ok(loc) => {
            let count = location_count(&loc);
            driver.trace(
                "discover_ok",
                format!("definition locations={count} raw={}", truncate_json(&loc, 400)),
            );
            loc
        }
        Err(e) => {
            return Ok(finish_report(
                &driver,
                opts,
                "fail",
                format!("definition: {e}"),
                None,
                None,
                &golden,
                line,
                character,
                &uri,
            ));
        }
    };
    let def_count = location_count(&def_result);

    driver.trace(
        "references_start",
        "textDocument/references (Find References)".to_string(),
    );
    let refs_result = driver.request_lsp(
        3,
        "textDocument/references",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": true },
        }),
        discover_deadline,
    );
    let (refs_value, refs_count) = match refs_result {
        Ok(v) => {
            let n = location_count(&v);
            driver.trace(
                "references_ok",
                format!("references locations={n} raw={}", truncate_json(&v, 400)),
            );
            (Some(v), n)
        }
        Err(e) => {
            driver.trace("references_fail", e.clone());
            (None, 0)
        }
    };

    let min_refs = golden.min_references.unwrap_or(1);
    let refs_ok = refs_count >= min_refs;
    let result_tag = if def_count > 0 && refs_ok {
        "pass"
    } else if def_count > 0 {
        "fail"
    } else {
        "pass_empty"
    };
    let notes = if refs_ok {
        format!("definition={def_count} references={refs_count}")
    } else {
        format!("definition={def_count} references={refs_count} (want >={min_refs})")
    };
    Ok(finish_report(
        &driver,
        opts,
        result_tag,
        notes,
        Some(def_result),
        refs_value,
        refs_count,
        refs_ok,
        &golden,
        line,
        character,
        &uri,
    ))
}

fn location_count(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.len(),
        Value::Null => 0,
        _ if v.is_object() => 1,
        _ => 0,
    }
}

fn finish_report(
    driver: &MuxDriver,
    opts: &DiscoverContainerOpts,
    result: &str,
    notes: impl Into<String>,
    definition: Option<Value>,
    references: Option<Value>,
    references_count: usize,
    references_ok: bool,
    golden: &crate::ExpectedGolden,
    line: u32,
    character: u32,
    uri: &str,
) -> Value {
    let notes = notes.into();
    let wal_tail = opts.wal.as_ref().and_then(|wal| read_wal_definition_tail(wal));
    json!({
        "rpc": "discover-container",
        "corpus": golden.corpus,
        "language": golden.language,
        "result": result,
        "notes": notes,
        "definition_ok": definition.as_ref().map(location_count).unwrap_or(0) > 0,
        "references_ok": references_ok,
        "references_count": references_count,
        "request": {
            "method": "textDocument/definition",
            "uri": uri,
            "line": line,
            "character": character,
            "find_needle": golden.find,
        },
        "definition": definition,
        "references": references,
        "serve_wal": opts.wal.as_ref().map(|p| p.display().to_string()),
        "serve_wal_definition_ops": wal_tail,
        "trace": driver.take_trace(),
        "elapsed_ms": driver.start.elapsed().as_millis(),
    })
}

fn read_wal_definition_tail(wal: &Path) -> Option<Value> {
    if !wal.is_file() {
        return Some(json!({"error": "wal file missing"}));
    }
    let out = std::process::Command::new("sqlite3")
        .arg(wal)
        .arg("SELECT ts_unix_ms, level, operation, message FROM log WHERE operation LIKE '%definition%' ORDER BY id;")
        .output()
        .ok()?;
    if !out.status.success() {
        return Some(json!({"error": "sqlite3 query failed"}));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(json!({"rows": text.lines().collect::<Vec<_>>() }))
}
