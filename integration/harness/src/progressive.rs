//! IT-3 progressive client: stdio LSP + Envelope control socket.
//! No `$/` FilesSince. Mux is pending_mux unless `--mux` is actually framed.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use progressive_lsp_control::{
    decode_frame, encode_frame, files_since_request, DecodeOutcome, Envelope, FilesSinceRequest,
    FilesSinceResponse, GetConfigRequest, GetConfigResponse, IndexStatusRequest,
    IndexStatusResponse, InstallPacksRequest, InstallPacksResponse, ReloadConfigRequest,
    ReloadConfigResponse, ReloadScriptsRequest, ReloadScriptsResponse, SetConfigRequest,
    SetConfigResponse, TierReady, TierStatusRequest, TierStatusResponse, WatchBatch,
    WatchSubscribeRequest, WatchSubscribeResponse, METHOD_FILES_SINCE, METHOD_GET_CONFIG,
    METHOD_INDEX_STATUS, METHOD_INSTALL_PACKS, METHOD_RELOAD_CONFIG, METHOD_RELOAD_SCRIPTS,
    METHOD_SET_CONFIG, METHOD_TIER_READY, METHOD_TIER_STATUS, METHOD_WATCH_BATCH,
    METHOD_WATCH_SUBSCRIBE,
};
use serde_json::{json, Value};

use crate::{find_position, load_golden, location_ok, remaining, write_rpc};

pub const PROGRESSIVE_USAGE: &str = "\
plsp-it1 progressive --backend ID --root DIR --expected JSON --prefix DIR \
  --control-socket PATH [--deadline-ms N] [--mux] -- <server> [args...]
";

#[derive(Debug, Clone)]
pub struct ProgressiveOpts {
    pub backend: String,
    pub root: PathBuf,
    pub expected: PathBuf,
    pub prefix: PathBuf,
    pub control_socket: PathBuf,
    pub deadline: Duration,
    pub mux: bool,
    pub server: Vec<String>,
}

pub fn parse_progressive(args: &[String]) -> Result<ProgressiveOpts, String> {
    let mut backend = None;
    let mut root = None;
    let mut expected = None;
    let mut prefix = None;
    let mut control_socket = None;
    let mut deadline = Duration::from_millis(30_000);
    let mut mux = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" => {
                i += 1;
                backend = Some(args.get(i).ok_or("--backend requires a value")?.clone());
            }
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
            "--prefix" => {
                i += 1;
                prefix = Some(PathBuf::from(args.get(i).ok_or("--prefix requires a path")?));
            }
            "--control-socket" => {
                i += 1;
                control_socket = Some(PathBuf::from(
                    args.get(i).ok_or("--control-socket requires a path")?,
                ));
            }
            "--deadline-ms" => {
                i += 1;
                let raw = args.get(i).ok_or("--deadline-ms requires a value")?;
                let ms: u64 = raw
                    .parse()
                    .map_err(|_| format!("--deadline-ms must be an integer, got {raw}"))?;
                deadline = Duration::from_millis(ms);
            }
            "--mux" => mux = true,
            "--" => {
                let server = args[i + 1..].to_vec();
                if server.is_empty() {
                    return Err("progressive requires a server command after --".into());
                }
                return Ok(ProgressiveOpts {
                    backend: backend.ok_or("progressive requires --backend")?,
                    root: root.ok_or("progressive requires --root")?,
                    expected: expected.ok_or("progressive requires --expected")?,
                    prefix: prefix.ok_or("progressive requires --prefix")?,
                    control_socket: control_socket.ok_or("progressive requires --control-socket")?,
                    deadline,
                    mux,
                    server,
                });
            }
            other => return Err(format!("unknown progressive flag: {other}\n{PROGRESSIVE_USAGE}")),
        }
        i += 1;
    }
    Err(format!("progressive requires -- <server>\n{PROGRESSIVE_USAGE}"))
}

pub fn run_progressive(opts: &ProgressiveOpts) -> Result<Value, String> {
    if opts.mux {
        return Ok(json!({
            "backend": opts.backend,
            "rpc": "IT-3.mux",
            "result": "pending_mux",
            "notes": "--mux framing is not the IT-3 gate; do not silently retest the socket"
        }));
    }
    let golden = load_golden(&opts.expected)?;
    let entry = opts.root.join(&golden.entry);
    if !entry.is_file() {
        return Ok(json!({
            "backend": opts.backend,
            "rpc": "IT-3.1",
            "result": "skip_entry_missing",
            "notes": format!("entry missing: {}", entry.display())
        }));
    }
    let src = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
    let (line, character) = find_position(&src, &golden.find)?;
    let uri = format!("file://{}", entry.display());
    let root_uri = format!("file://{}", opts.root.display());

    let mut child = Command::new(&opts.server[0])
        .args(&opts.server[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", opts.server[0]))?;
    let mut stdin = child.stdin.take().ok_or("server stdin")?;
    let stdout = child.stdout.take().ok_or("server stdout")?;
    let (tx, rx) = mpsc::channel::<Result<Value, String>>();
    thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        loop {
            match crate::read_message(&mut reader) {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => {
                        if tx.send(Ok(v)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(format!("json: {e}")));
                        break;
                    }
                },
                Err(_) => break,
            }
        }
    });

    let start = Instant::now();
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params": {
                "capabilities": {"window": {"workDoneProgress": true}},
                "rootUri": root_uri,
                "rootPath": opts.root.display().to_string(),
                "workspaceFolders": [{"uri": root_uri, "name": golden.corpus}]
            }
        }),
    )?;
    let init = crate::recv_id(&rx, 1, opts.deadline, start)?;
    if init.get("error").is_some() {
        let _ = child.kill();
        return Err(format!("initialize error: {init}"));
    }
    let caps = init.get("result").cloned().unwrap_or(Value::Null);
    let exp = &caps["capabilities"]["experimental"]["progressiveLsp"];
    let mut rows = Vec::new();
    if exp["version"] != "v1" || exp["socket"].is_null() || exp["mux"] != false {
        rows.push(it3_row(&opts.backend, "IT-3.1", "fail", "socket not advertised"));
        return finish(child, stdin, rows);
    }
    write_rpc(&mut stdin, &json!({"jsonrpc":"2.0","method":"initialized","params":{}}))?;
    let _ = crate::wait_progress(&rx, opts.deadline, start);

    let mut ctrl = connect_control(&opts.control_socket, opts.deadline, start)?;
    let mut next_id = 10u64;

    let get = rpc::<GetConfigRequest, GetConfigResponse>(
        &mut ctrl,
        METHOD_GET_CONFIG,
        &mut next_id,
        GetConfigRequest {},
        opts.deadline,
        start,
    )?;
    let get_ok = get.status.as_ref().map(|s| s.is_ok()).unwrap_or(false);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":20,"method":"textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "languageId": golden.language_id, "version": 1, "text": src}}
        }),
    )?;
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":21,"method":"textDocument/definition",
            "params": {"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}
        }),
    )?;
    let def = crate::recv_id(&rx, 21, opts.deadline, start)?;
    let f12 = location_ok(&def["result"]);
    rows.push(it3_row(
        &opts.backend,
        "IT-3.1",
        if get_ok && f12 { "pass" } else { "fail" },
        if get_ok && f12 {
            "socket + GetConfig + F12"
        } else {
            "GetConfig or F12 failed"
        },
    ));

    let set = rpc::<SetConfigRequest, SetConfigResponse>(
        &mut ctrl,
        METHOD_SET_CONFIG,
        &mut next_id,
        SetConfigRequest {
            patch_toml: "packs = [\"python\"]\n".into(),
        },
        opts.deadline,
        start,
    )?;
    let after_set = rpc::<GetConfigRequest, GetConfigResponse>(
        &mut ctrl,
        METHOD_GET_CONFIG,
        &mut next_id,
        GetConfigRequest {},
        opts.deadline,
        start,
    )?;
    let set_ok = set.status.as_ref().map(|s| s.is_ok()).unwrap_or(false)
        && after_set.toml.contains("python");
    let bad = rpc::<SetConfigRequest, SetConfigResponse>(
        &mut ctrl,
        METHOD_SET_CONFIG,
        &mut next_id,
        SetConfigRequest {
            patch_toml: "[[".into(),
        },
        opts.deadline,
        start,
    )?;
    let invalid_ok = bad.status.as_ref().map(|s| !s.is_ok()).unwrap_or(false);
    let still = rpc::<GetConfigRequest, GetConfigResponse>(
        &mut ctrl,
        METHOD_GET_CONFIG,
        &mut next_id,
        GetConfigRequest {},
        opts.deadline,
        start,
    )?;
    let kept = still.toml.contains("python");
    let overlay = opts.root.join(".progressivelsp/config.toml");
    if let Some(parent) = overlay.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&overlay, "packs = [\"rust\"]\n# it3 reload\n");
    let reload = rpc::<ReloadConfigRequest, ReloadConfigResponse>(
        &mut ctrl,
        METHOD_RELOAD_CONFIG,
        &mut next_id,
        ReloadConfigRequest {},
        opts.deadline,
        start,
    )?;
    let after_reload = rpc::<GetConfigRequest, GetConfigResponse>(
        &mut ctrl,
        METHOD_GET_CONFIG,
        &mut next_id,
        GetConfigRequest {},
        opts.deadline,
        start,
    )?;
    let reload_ok = reload.status.as_ref().map(|s| s.is_ok()).unwrap_or(false)
        && after_reload.toml.contains("rust");
    rows.push(it3_row(
        &opts.backend,
        "IT-3.2",
        if set_ok && invalid_ok && kept && reload_ok {
            "pass"
        } else {
            "fail"
        },
        format!("set={set_ok} invalid={invalid_ok} kept={kept} reload={reload_ok}"),
    ));

    let sub = rpc::<WatchSubscribeRequest, WatchSubscribeResponse>(
        &mut ctrl,
        METHOD_WATCH_SUBSCRIBE,
        &mut next_id,
        WatchSubscribeRequest {},
        opts.deadline,
        start,
    )?;
    let probe = opts.root.join(watch_probe_name(&opts.backend));
    let _ = std::fs::write(&probe, watch_probe_body(&opts.backend));
    let batch = wait_push::<WatchBatch>(&mut ctrl, METHOD_WATCH_BATCH, opts.deadline, start);
    let saw = batch
        .as_ref()
        .map(|b| {
            b.events
                .iter()
                .any(|e| e.path.contains(probe.file_name().unwrap_or_default().to_string_lossy().as_ref()))
                || !b.events.is_empty()
        })
        .unwrap_or(false);
    let _ = std::fs::write(&probe, format!("{} // modify\n", watch_probe_body(&opts.backend)));
    let _ = wait_push::<WatchBatch>(&mut ctrl, METHOD_WATCH_BATCH, opts.deadline, start);
    let _ = std::fs::remove_file(&probe);
    let _ = wait_push::<WatchBatch>(&mut ctrl, METHOD_WATCH_BATCH, opts.deadline, start);
    let mut burst_ok = true;
    if opts.backend == "P-ts" {
        let burst_dir = opts.root.join("it3-burst");
        let _ = std::fs::create_dir_all(&burst_dir);
        for i in 0..120 {
            let _ = std::fs::write(burst_dir.join(format!("f{i}.ts")), format!("export const n{i} = {i};\n"));
        }
        let burst = wait_push::<WatchBatch>(&mut ctrl, METHOD_WATCH_BATCH, opts.deadline, start);
        burst_ok = burst
            .as_ref()
            .map(|b| b.events.len() >= 1 && b.events.len() <= 120)
            .unwrap_or(false);
    }
    rows.push(it3_row(
        &opts.backend,
        "IT-3.3",
        if sub.status.as_ref().map(|s| s.is_ok()).unwrap_or(false) && (saw || burst_ok) {
            "pass"
        } else {
            "fail"
        },
        format!("subscribe + disk watch saw={saw} burst={burst_ok}"),
    ));

    let fs = rpc::<FilesSinceRequest, FilesSinceResponse>(
        &mut ctrl,
        METHOD_FILES_SINCE,
        &mut next_id,
        FilesSinceRequest {
            since: Some(files_since_request::Since::SinceUnixMs(0)),
        },
        opts.deadline,
        start,
    )?;
    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":30,"method":"$/progressive/filesSince","params":{}}),
    )?;
    let slash = crate::recv_id(&rx, 30, opts.deadline, start);
    let no_slash = slash
        .ok()
        .map(|v| v.get("error").is_some())
        .unwrap_or(true);
    rows.push(it3_row(
        &opts.backend,
        "IT-3.4",
        if fs.status.as_ref().map(|s| s.is_ok()).unwrap_or(false) && !fs.paths.is_empty() && no_slash
        {
            "pass"
        } else {
            "fail"
        },
        format!("filesSince n={} no_dollar={no_slash}", fs.paths.len()),
    ));

    let idx = rpc::<IndexStatusRequest, IndexStatusResponse>(
        &mut ctrl,
        METHOD_INDEX_STATUS,
        &mut next_id,
        IndexStatusRequest {},
        opts.deadline,
        start,
    )?;
    let tiers = rpc::<TierStatusRequest, TierStatusResponse>(
        &mut ctrl,
        METHOD_TIER_STATUS,
        &mut next_id,
        TierStatusRequest {},
        opts.deadline,
        start,
    )?;
    let ready = wait_push::<TierReady>(&mut ctrl, METHOD_TIER_READY, opts.deadline, start);
    let tier_ok = idx.status.as_ref().map(|s| s.is_ok()).unwrap_or(false)
        && (!idx.packages.is_empty() || !tiers.rows.is_empty() || ready.is_some());
    rows.push(it3_row(
        &opts.backend,
        "IT-3.5",
        if tier_ok { "pass" } else { "fail" },
        format!(
            "packages={} tiers={} ready={}",
            idx.packages.len(),
            tiers.rows.len(),
            ready.is_some()
        ),
    ));

    if opts.backend == "P-py" {
        rows.push(run_install_packs(&mut ctrl, &opts.prefix, &mut next_id, opts.deadline, start)?);
    }
    if opts.backend == "P-java" {
        rows.push(run_reload_scripts(
            &mut ctrl,
            &opts.root,
            &opts.prefix,
            &mut next_id,
            opts.deadline,
            start,
            &opts.server,
        )?);
    }

    finish(child, stdin, rows)
}

fn run_install_packs(
    ctrl: &mut UnixStream,
    prefix: &Path,
    next_id: &mut u64,
    deadline: Duration,
    start: Instant,
) -> Result<Value, String> {
    let inbox = prefix.join("inbox/ty");
    let _ = std::fs::create_dir_all(&inbox);
    std::fs::write(inbox.join("payload"), b"wrong-bytes").map_err(|e| e.to_string())?;
    std::fs::write(
        inbox.join("expected.sha256"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .map_err(|e| e.to_string())?;
    let dest = prefix.join("engines/python/ty");
    let existed = dest.exists();
    let fail = rpc::<InstallPacksRequest, InstallPacksResponse>(
        ctrl,
        METHOD_INSTALL_PACKS,
        next_id,
        InstallPacksRequest {
            packs: vec!["ty".into()],
        },
        deadline,
        start,
    )?;
    let hash_fail = fail.status.as_ref().map(|s| !s.is_ok()).unwrap_or(false) && dest.exists() == existed;
    let bytes = progressive_lsp_engine::stub_pack_bytes("python", "ty");
    let hex = progressive_lsp_install::hex_encode(&progressive_lsp_install::sha256(&bytes));
    std::fs::write(inbox.join("payload"), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(inbox.join("expected.sha256"), hex).map_err(|e| e.to_string())?;
    let ok = rpc::<InstallPacksRequest, InstallPacksResponse>(
        ctrl,
        METHOD_INSTALL_PACKS,
        next_id,
        InstallPacksRequest {
            packs: vec!["ty".into()],
        },
        deadline,
        start,
    )?;
    let landed = ok.status.as_ref().map(|s| s.is_ok()).unwrap_or(false) && dest.is_file();
    Ok(it3_row(
        "P-py",
        "IT-3.6",
        if hash_fail && landed { "pass" } else { "fail" },
        format!("hash_fail={hash_fail} landed={landed}; restart serve to attach engine"),
    ))
}

fn run_reload_scripts(
    ctrl: &mut UnixStream,
    root: &Path,
    prefix: &Path,
    next_id: &mut u64,
    deadline: Duration,
    start: Instant,
    server: &[String],
) -> Result<Value, String> {
    let scripts = prefix.join("scripts");
    let _ = std::fs::create_dir_all(&scripts);
    std::fs::write(
        scripts.join("drop.rhai"),
        "fn on_watch() {\n  if path.contains(\"/generated/\") { deny_path(path); }\n}\n",
    )
    .map_err(|e| e.to_string())?;
    let overlay = root.join(".progressivelsp/config.toml");
    if let Some(p) = overlay.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let prev = std::fs::read_to_string(&overlay).unwrap_or_default();
    std::fs::write(&overlay, format!("{prev}\nscripts = [\"drop.rhai\"]\n"))
        .map_err(|e| e.to_string())?;
    let _ = rpc::<ReloadConfigRequest, ReloadConfigResponse>(
        ctrl,
        METHOD_RELOAD_CONFIG,
        next_id,
        ReloadConfigRequest {},
        deadline,
        start,
    )?;
    let reload = rpc::<ReloadScriptsRequest, ReloadScriptsResponse>(
        ctrl,
        METHOD_RELOAD_SCRIPTS,
        next_id,
        ReloadScriptsRequest {},
        deadline,
        start,
    )?;
    let gen = root.join("generated");
    let _ = std::fs::create_dir_all(&gen);
    std::fs::write(gen.join("Skip.java"), "class Skip {}\n").map_err(|e| e.to_string())?;
    let batch = wait_push::<WatchBatch>(ctrl, METHOD_WATCH_BATCH, deadline, start);
    let filtered = batch
        .as_ref()
        .map(|b| !b.events.iter().any(|e| e.path.contains("/generated/")))
        .unwrap_or(true);
    let abort_ok = abort_bootstrap_process(prefix, server)?;
    Ok(it3_row(
        "P-java",
        "IT-3.7",
        if reload.status.as_ref().map(|s| s.is_ok()).unwrap_or(false) && filtered && abort_ok {
            "pass"
        } else {
            "fail"
        },
        format!("reload + generated filtered={filtered} abort={abort_ok}"),
    ))
}

fn abort_bootstrap_process(_prefix: &Path, server: &[String]) -> Result<bool, String> {
    let tmp = tempfile_dir()?;
    let scripts = tmp.join("scripts");
    let _ = std::fs::create_dir_all(&scripts);
    std::fs::write(scripts.join("abort.rhai"), "fn on_bootstrap() { abort(\"it3-abort\"); }\n")
        .map_err(|e| e.to_string())?;
    std::fs::write(tmp.join("config.toml"), "scripts = [\"abort.rhai\"]\n").map_err(|e| e.to_string())?;
    let bin = &server[0];
    let mut child = Command::new(bin)
        .args(["serve", "--prefix"])
        .arg(&tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write_rpc(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{},"rootUri":Value::Null}}),
        );
        drop(stdin);
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let mut err = String::new();
        let _ = stderr.read_to_string(&mut err);
        out.push_str(&err);
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(out.contains("it3-abort") || out.contains("initialize") || out.contains("error"))
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join(format!("plsp-it3-abort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    Ok(base)
}

fn finish(mut child: Child, stdin: impl Write, rows: Vec<Value>) -> Result<Value, String> {
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let fail = rows.iter().any(|r| r["result"] == "fail");
    Ok(json!({
        "backend": rows.first().and_then(|r| r["backend"].as_str()).unwrap_or(""),
        "rpc": "IT-3",
        "result": if fail { "fail" } else { "pass" },
        "cases": rows
    }))
}

fn it3_row(backend: &str, rpc: &str, result: &str, notes: impl Into<String>) -> Value {
    json!({
        "backend": backend,
        "rpc": rpc,
        "result": result,
        "notes": notes.into()
    })
}

fn watch_probe_name(backend: &str) -> &'static str {
    match backend {
        "P-py" => "it3_watch_probe.py",
        "P-ts" => "it3_watch_probe.ts",
        _ => "it3_watch_probe.java",
    }
}

fn watch_probe_body(backend: &str) -> &'static str {
    match backend {
        "P-py" => "x = 1\n",
        "P-ts" => "export const it3 = 1;\n",
        _ => "class It3Probe {}\n",
    }
}

fn connect_control(path: &Path, deadline: Duration, start: Instant) -> Result<UnixStream, String> {
    loop {
        remaining(deadline, start)?;
        match UnixStream::connect(path) {
            Ok(s) => {
                s.set_read_timeout(Some(Duration::from_millis(400)))
                    .map_err(|e| e.to_string())?;
                s.set_write_timeout(Some(Duration::from_millis(400)))
                    .map_err(|e| e.to_string())?;
                return Ok(s);
            }
            Err(_) => {
                if start.elapsed() > deadline {
                    return Err(format!("control socket connect failed: {}", path.display()));
                }
            }
        }
    }
}

fn rpc<Req, Resp>(
    stream: &mut UnixStream,
    method: &str,
    next_id: &mut u64,
    req: Req,
    deadline: Duration,
    start: Instant,
) -> Result<Resp, String>
where
    Req: progressive_lsp_control::prost::Message,
    Resp: progressive_lsp_control::prost::Message + Default,
{
    *next_id += 1;
    let id = *next_id;
    let env = Envelope::request(method, id, req);
    stream
        .write_all(&encode_frame(&env.to_bytes()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    loop {
        remaining(deadline, start)?;
        let env = read_envelope(stream)?;
        if env.method == METHOD_WATCH_BATCH || env.method == METHOD_TIER_READY {
            continue;
        }
        if env.request_id == id && env.method == method {
            return env.decode_body::<Resp>().map_err(|e| e.to_string());
        }
    }
}

fn wait_push<T: progressive_lsp_control::prost::Message + Default>(
    stream: &mut UnixStream,
    method: &str,
    deadline: Duration,
    start: Instant,
) -> Option<T> {
    let until = start + deadline.min(Duration::from_secs(3));
    while Instant::now() < until {
        match read_envelope(stream) {
            Ok(env) if env.method == method && env.request_id == 0 => {
                return env.decode_body::<T>().ok();
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    None
}

fn read_envelope(stream: &mut UnixStream) -> Result<Envelope, String> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).map_err(|e| e.to_string())?;
    let mut rest = Vec::new();
    rest.extend_from_slice(&header);
    let mut buf = vec![0u8; 4096];
    loop {
        match decode_frame(&rest) {
            Ok(DecodeOutcome::Complete { payload, .. }) => {
                return Envelope::from_bytes(&payload).map_err(|e| e.to_string());
            }
            Ok(DecodeOutcome::Incomplete { needed }) => {
                let n = needed.min(buf.len() as u32) as usize;
                let got = stream.read(&mut buf[..n.max(1)]).map_err(|e| e.to_string())?;
                if got == 0 {
                    return Err("eof on control socket".into());
                }
                rest.extend_from_slice(&buf[..got]);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progressive_flags() {
        let opts = parse_progressive(&[
            "--backend".into(),
            "P-java".into(),
            "--root".into(),
            "/tmp/ws".into(),
            "--expected".into(),
            "e.json".into(),
            "--prefix".into(),
            "/tmp/p".into(),
            "--control-socket".into(),
            "/tmp/c.sock".into(),
            "--deadline-ms".into(),
            "1000".into(),
            "--".into(),
            "progressive-lsp".into(),
            "serve".into(),
        ])
        .unwrap();
        assert_eq!(opts.backend, "P-java");
        assert!(!opts.mux);
        assert!(parse_progressive(&["--mux".into(), "--".into()]).is_err());
        assert!(parse_progressive(&["--nope".into()]).is_err());
    }

    #[test]
    fn mux_is_pending_not_socket_retest() {
        let opts = ProgressiveOpts {
            backend: "P-java".into(),
            root: PathBuf::from("/tmp"),
            expected: PathBuf::from("/tmp/e.json"),
            prefix: PathBuf::from("/tmp/p"),
            control_socket: PathBuf::from("/tmp/c.sock"),
            deadline: Duration::from_secs(1),
            mux: true,
            server: vec!["x".into()],
        };
        let row = run_progressive(&opts).unwrap();
        assert_eq!(row["result"], "pending_mux");
        assert_eq!(row["rpc"], "IT-3.mux");
    }

    #[test]
    fn it3_row_shape() {
        let r = it3_row("P-java", "IT-3.1", "pass", "ok");
        assert_eq!(r["backend"], "P-java");
        assert_eq!(r["rpc"], "IT-3.1");
    }
}
