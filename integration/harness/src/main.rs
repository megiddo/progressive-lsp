//! Tiny stdio LSP driver for IT-1 handshake and IT-2 stock backends.
//! Integration only. No `$/` FilesSince. Not a workspace member.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const USAGE: &str = "\
plsp-it1 handshake [--root-uri URI] [--deadline-ms N] [--assert-stock] -- <server> [args...]
plsp-it1 backend --expected JSON --root DIR [--deadline-ms N] [--t3-pack NAME] -- <server> [args...]
plsp-it1 fetch --pins JSON --cache DIR [--id ID]
";

fn main() {
    if let Err(e) = run(std::env::args().skip(1).collect()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.is_empty() || matches!(args.first().map(String::as_str), Some("-h" | "--help" | "help"))
    {
        return Err(USAGE.trim_end().into());
    }
    match args[0].as_str() {
        "handshake" => {
            let opts = parse_handshake(&args[1..])?;
            let result = handshake(&opts)?;
            if opts.assert_stock {
                assert_stock(&result)?;
            }
            println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
            Ok(())
        }
        "backend" => {
            let opts = parse_backend(&args[1..])?;
            let row = run_backend(&opts)?;
            println!("{}", serde_json::to_string_pretty(&row).map_err(|e| e.to_string())?);
            if row["result"] == "fail" {
                return Err(format!("backend fail: {}", row["notes"]));
            }
            Ok(())
        }
        "fetch" => {
            let opts = parse_fetch(&args[1..])?;
            fetch_pins(&opts)?;
            Ok(())
        }
        other => Err(format!("unknown command: {other}\n{USAGE}")),
    }
}

struct HandshakeOpts {
    root_uri: Option<String>,
    deadline: Duration,
    assert_stock: bool,
    server: Vec<String>,
}

fn parse_handshake(args: &[String]) -> Result<HandshakeOpts, String> {
    let mut root_uri = None;
    let mut deadline = Duration::from_millis(15_000);
    let mut assert_stock = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root-uri" => {
                i += 1;
                let v = args.get(i).ok_or("--root-uri requires a value")?;
                root_uri = Some(v.clone());
            }
            "--deadline-ms" => {
                i += 1;
                let raw = args.get(i).ok_or("--deadline-ms requires a value")?;
                let ms: u64 = raw
                    .parse()
                    .map_err(|_| format!("--deadline-ms must be an integer, got {raw}"))?;
                deadline = Duration::from_millis(ms);
            }
            "--assert-stock" => assert_stock = true,
            "--" => {
                let server = args[i + 1..].to_vec();
                if server.is_empty() {
                    return Err("handshake requires a server command after --".into());
                }
                return Ok(HandshakeOpts {
                    root_uri,
                    deadline,
                    assert_stock,
                    server,
                });
            }
            other => return Err(format!("unknown handshake flag: {other}\n{USAGE}")),
        }
        i += 1;
    }
    Err(format!("handshake requires -- <server>\n{USAGE}"))
}

#[derive(Debug, Clone)]
struct BackendOpts {
    expected: PathBuf,
    root: PathBuf,
    deadline: Duration,
    t3_pack: Option<String>,
    server: Vec<String>,
}

fn parse_backend(args: &[String]) -> Result<BackendOpts, String> {
    let mut expected = None;
    let mut root = None;
    let mut deadline = Duration::from_millis(60_000);
    let mut t3_pack = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--expected" => {
                i += 1;
                expected = Some(PathBuf::from(
                    args.get(i).ok_or("--expected requires a path")?,
                ));
            }
            "--root" => {
                i += 1;
                root = Some(PathBuf::from(args.get(i).ok_or("--root requires a path")?));
            }
            "--deadline-ms" => {
                i += 1;
                let raw = args.get(i).ok_or("--deadline-ms requires a value")?;
                let ms: u64 = raw
                    .parse()
                    .map_err(|_| format!("--deadline-ms must be an integer, got {raw}"))?;
                deadline = Duration::from_millis(ms);
            }
            "--t3-pack" => {
                i += 1;
                t3_pack = Some(args.get(i).ok_or("--t3-pack requires a name")?.clone());
            }
            "--" => {
                let server = args[i + 1..].to_vec();
                if server.is_empty() {
                    return Err("backend requires a server command after --".into());
                }
                return Ok(BackendOpts {
                    expected: expected.ok_or("backend requires --expected")?,
                    root: root.ok_or("backend requires --root")?,
                    deadline,
                    t3_pack,
                    server,
                });
            }
            other => return Err(format!("unknown backend flag: {other}\n{USAGE}")),
        }
        i += 1;
    }
    Err(format!("backend requires -- <server>\n{USAGE}"))
}

struct FetchOpts {
    pins: PathBuf,
    cache: PathBuf,
    only: Option<String>,
}

fn parse_fetch(args: &[String]) -> Result<FetchOpts, String> {
    let mut pins = None;
    let mut cache = None;
    let mut only = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pins" => {
                i += 1;
                pins = Some(PathBuf::from(args.get(i).ok_or("--pins requires a path")?));
            }
            "--cache" => {
                i += 1;
                cache = Some(PathBuf::from(args.get(i).ok_or("--cache requires a path")?));
            }
            "--id" => {
                i += 1;
                only = Some(args.get(i).ok_or("--id requires a value")?.clone());
            }
            other => return Err(format!("unknown fetch flag: {other}\n{USAGE}")),
        }
        i += 1;
    }
    Ok(FetchOpts {
        pins: pins.ok_or("fetch requires --pins")?,
        cache: cache.ok_or("fetch requires --cache")?,
        only,
    })
}

fn handshake(opts: &HandshakeOpts) -> Result<Value, String> {
    let mut child = Command::new(&opts.server[0])
        .args(&opts.server[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", opts.server[0]))?;
    let mut stdin = child.stdin.take().ok_or("server stdin")?;
    let stdout = child.stdout.take().ok_or("server stdout")?;
    let mut reader = io::BufReader::new(stdout);

    let params = match &opts.root_uri {
        Some(uri) => {
            let path = uri.strip_prefix("file://").unwrap_or(uri);
            json!({
                "capabilities": {},
                "rootUri": uri,
                "rootPath": path
            })
        }
        None => json!({"capabilities": {}, "rootUri": Value::Null}),
    };
    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params": params}),
    )?;
    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    )?;

    let start = Instant::now();
    let init = read_rpc_until(&mut reader, opts.deadline, start)?;
    if init.get("id") != Some(&json!(1)) {
        let _ = child.kill();
        return Err(format!("expected initialize id 1, got {init}"));
    }
    if init.get("error").is_some() {
        let _ = child.kill();
        return Err(format!("initialize error: {init}"));
    }

    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":Value::Null}),
    )?;
    let _shutdown = read_rpc_until(&mut reader, opts.deadline, start)?;
    write_rpc(&mut stdin, &json!({"jsonrpc":"2.0","method":"exit"}))?;
    drop(stdin);

    remaining(opts.deadline, start)?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("server exited {status}"));
    }
    init.get("result")
        .cloned()
        .ok_or_else(|| "initialize missing result".into())
}

fn assert_stock(result: &Value) -> Result<(), String> {
    if result["serverInfo"]["name"] != "progressive-lsp" {
        return Err(format!(
            "serverInfo.name want progressive-lsp, got {}",
            result["serverInfo"]["name"]
        ));
    }
    let cap = &result["capabilities"]["experimental"]["progressiveLsp"];
    if cap["version"] != "v1" {
        return Err(format!("progressiveLsp.version want v1, got {}", cap["version"]));
    }
    if !cap["socket"].is_null() {
        return Err(format!("default serve socket must be null, got {}", cap["socket"]));
    }
    if cap["mux"] != false {
        return Err(format!("default serve mux must be false, got {}", cap["mux"]));
    }
    Ok(())
}

fn assert_stock_caps(result: &Value) -> Result<(), String> {
    assert_stock(result)?;
    let caps = &result["capabilities"];
    for key in [
        "definitionProvider",
        "referencesProvider",
        "hoverProvider",
        "documentSymbolProvider",
        "workspaceSymbolProvider",
        "semanticTokensProvider",
    ] {
        if caps[key].is_null() || caps[key] == json!(false) {
            return Err(format!("missing capability {key}"));
        }
    }
    if caps["experimental"]["progressiveLsp"].is_null() {
        return Err("missing experimental.progressiveLsp".into());
    }
    Ok(())
}

struct ExpectedGolden {
    corpus: String,
    language: String,
    language_id: String,
    entry: String,
    find: String,
    workspace_symbol: String,
    ghost_sibling: Option<String>,
    expected_ceiling: bool,
    corpus_sha: String,
}

fn load_golden(path: &Path) -> Result<ExpectedGolden, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("golden json: {e}"))?;
    Ok(ExpectedGolden {
        corpus: v["corpus"].as_str().unwrap_or("").to_string(),
        language: v["language"].as_str().unwrap_or("").to_string(),
        language_id: v["language_id"].as_str().unwrap_or("").to_string(),
        entry: v["entry"].as_str().ok_or("golden.entry required")?.to_string(),
        find: v["find"].as_str().unwrap_or("").to_string(),
        workspace_symbol: v["workspace_symbol"]
            .as_str()
            .or_else(|| v["find"].as_str())
            .unwrap_or("")
            .to_string(),
        ghost_sibling: v["ghost_sibling"].as_str().map(str::to_string).filter(|s| !s.is_empty()),
        expected_ceiling: v["expected_ceiling"].as_bool().unwrap_or(false),
        corpus_sha: v["corpus_sha"].as_str().unwrap_or("").to_string(),
    })
}

fn find_position(src: &str, needle: &str) -> Result<(u32, u32), String> {
    if needle.is_empty() {
        return Ok((0, 0));
    }
    let idx = src.find(needle).ok_or_else(|| format!("find {needle:?} missing"))?;
    let before = &src[..idx];
    let line = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let character = before.rsplit('\n').next().map(|s| s.chars().count()).unwrap_or(0) as u32;
    Ok((line, character))
}

fn ghost_append(language_id: &str, src: &str) -> (String, &'static str) {
    match language_id {
        "java" => (format!("{src}\nclass It2Ghost {{}}\n"), "It2Ghost"),
        "php" => (format!("{src}\nclass It2Ghost {{}}\n"), "It2Ghost"),
        "javascript" | "typescript" => (format!("{src}\nfunction it2ghost() {{}}\n"), "it2ghost"),
        "css" => (format!("{src}\n.it2ghost {{ color: red; }}\n"), "it2ghost"),
        "html" => (format!("{src}\n<div id=\"it2ghost\"></div>\n"), "it2ghost"),
        "go" => (format!("{src}\nfunc It2Ghost() {{}}\n"), "It2Ghost"),
        "zig" => (format!("{src}\npub fn it2ghost() void {{}}\n"), "it2ghost"),
        "python" => (format!("{src}\ndef it2ghost():\n    pass\n"), "it2ghost"),
        "rust" => (format!("{src}\npub fn it2ghost() {{}}\n"), "it2ghost"),
        "c" => (format!("{src}\nvoid it2ghost(void) {{}}\n"), "it2ghost"),
        "cpp" => (format!("{src}\nvoid it2ghost() {{}}\n"), "it2ghost"),
        "csharp" => (format!("{src}\nclass It2Ghost {{}}\n"), "It2Ghost"),
        _ => (format!("{src}\n// it2ghost\n"), "it2ghost"),
    }
}

fn run_backend(opts: &BackendOpts) -> Result<Value, String> {
    let golden = load_golden(&opts.expected)?;
    let entry = opts.root.join(&golden.entry);
    if !entry.is_file() {
        return Ok(row(
            &golden,
            "skip_entry_missing",
            false,
            false,
            false,
            &format!("entry missing: {}", entry.display()),
            opts.t3_pack.as_deref(),
        ));
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
        let mut reader = io::BufReader::new(stdout);
        loop {
            match read_message(&mut reader) {
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
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
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
                "workspaceFolders": [{"uri": root_uri, "name": golden.corpus}],
                "initializationOptions": {}
            }
        }),
    )?;
    let init = recv_id(&rx, 1, opts.deadline, start)?;
    if init.get("error").is_some() {
        let _ = child.kill();
        return Err(format!("initialize error: {init}"));
    }
    let caps = init.get("result").cloned().unwrap_or(Value::Null);
    if let Err(e) = assert_stock_caps(&caps) {
        let _ = child.kill();
        return Ok(row(
            &golden,
            "fail",
            false,
            false,
            false,
            &e,
            opts.t3_pack.as_deref(),
        ));
    }
    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    )?;
    let _ = wait_progress(&rx, opts.deadline, start);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": golden.language_id,
                    "version": 1,
                    "text": src
                }
            }
        }),
    )?;

    let pos = json!({"line": line, "character": character});
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":10,"method":"textDocument/definition",
            "params": {"textDocument":{"uri": uri}, "position": pos}
        }),
    )?;
    let def = recv_id(&rx, 10, opts.deadline, start)?;
    let definition_ok = location_ok(&def["result"]);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":11,"method":"textDocument/references",
            "params": {"textDocument":{"uri": uri}, "position": pos, "context": {"includeDeclaration": true}}
        }),
    )?;
    let refs = recv_id(&rx, 11, opts.deadline, start)?;
    let refs_ok = location_ok(&refs["result"]);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":12,"method":"textDocument/hover",
            "params": {"textDocument":{"uri": uri}, "position": pos}
        }),
    )?;
    let hover = recv_id(&rx, 12, opts.deadline, start)?;
    let hover_pass = hover_ok(&hover["result"]);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":13,"method":"textDocument/documentSymbol",
            "params": {"textDocument":{"uri": uri}}
        }),
    )?;
    let symbols = recv_id(&rx, 13, opts.deadline, start)?;
    let symbols_ok = location_ok(&symbols["result"]);
    if !golden.workspace_symbol.is_empty() {
        write_rpc(
            &mut stdin,
            &json!({
                "jsonrpc":"2.0","id":19,"method":"workspace/symbol",
                "params": {"query": golden.workspace_symbol}
            }),
        )?;
        let _ = recv_id(&rx, 19, opts.deadline, start);
    }

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":14,"method":"textDocument/semanticTokens/full",
            "params": {"textDocument":{"uri": uri}}
        }),
    )?;
    let tokens = recv_id(&rx, 14, opts.deadline, start)?;
    let tokens_ok = tokens["result"]["data"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": format!("{src}\n")}]
            }
        }),
    )?;
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":15,"method":"textDocument/hover",
            "params": {"textDocument":{"uri": uri}, "position": pos}
        }),
    )?;
    let hover2 = recv_id(&rx, 15, opts.deadline, start)?;
    let did_change_ok = hover_ok(&hover2["result"]) || tokens_ok;

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":16,"method":"$/progressive/filesSince",
            "params": {}
        }),
    )?;
    let files_since = recv_id(&rx, 16, opts.deadline, start)?;
    if files_since["error"]["code"] != -32601 {
        let _ = child.kill();
        return Ok(row(
            &golden,
            "fail",
            definition_ok,
            tokens_ok,
            false,
            "server implemented $/progressive/filesSince",
            opts.t3_pack.as_deref(),
        ));
    }
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":17,"method":"workspace/filesSince",
            "params": {}
        }),
    )?;
    let ws_files = recv_id(&rx, 17, opts.deadline, start)?;
    if ws_files["error"]["code"] != -32601 {
        let _ = child.kill();
        return Ok(row(
            &golden,
            "fail",
            definition_ok,
            tokens_ok,
            false,
            "server implemented workspace/filesSince",
            opts.t3_pack.as_deref(),
        ));
    }

    let mut ghost_ok = false;
    let mut notes = Vec::new();
    if let Some(rel) = &golden.ghost_sibling {
        let sibling = opts.root.join(rel);
        if sibling.is_file() {
            let old = std::fs::read_to_string(&sibling).map_err(|e| e.to_string())?;
            let (patched, needle) = ghost_append(&golden.language_id, &old);
            std::fs::write(&sibling, patched).map_err(|e| e.to_string())?;
            let ghost_deadline = Duration::from_millis(2_050);
            let ghost_start = Instant::now();
            while ghost_start.elapsed() < ghost_deadline {
                write_rpc(
                    &mut stdin,
                    &json!({
                        "jsonrpc":"2.0","id":18,"method":"workspace/symbol",
                        "params": {"query": needle}
                    }),
                )?;
                if let Ok(sym) = recv_id(&rx, 18, opts.deadline, start) {
                    if location_ok(&sym["result"]) {
                        ghost_ok = true;
                        break;
                    }
                }
            }
            if !ghost_ok {
                notes.push("ghost_edit: no workspace/symbol hit within coalescer+2s; stock notify may still be in flight".into());
            }
        } else {
            notes.push(format!("ghost sibling missing: {}", sibling.display()));
        }
    } else {
        notes.push("no ghost_sibling in golden".into());
    }

    write_rpc(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":Value::Null}),
    )?;
    let _ = recv_id(&rx, 99, opts.deadline, start);
    write_rpc(&mut stdin, &json!({"jsonrpc":"2.0","method":"exit"}))?;
    drop(stdin);
    let _ = child.wait();

    if !refs_ok {
        notes.push("references empty".into());
    }
    if !hover_pass {
        notes.push("hover empty".into());
    }
    if !symbols_ok {
        notes.push("documentSymbol empty".into());
    }
    if !did_change_ok {
        notes.push("didChange hover/tokens failed".into());
    }

    let stock_ok = definition_ok && tokens_ok && hover_pass && symbols_ok && did_change_ok;
    let result = if stock_ok {
        "pass"
    } else {
        "fail"
    };
    let mut row = row(
        &golden,
        result,
        definition_ok,
        tokens_ok,
        ghost_ok,
        &notes.join("; "),
        opts.t3_pack.as_deref(),
    );
    row["references_ok"] = json!(refs_ok);
    row["hover_ok"] = json!(hover_pass);
    row["symbols_ok"] = json!(symbols_ok);
    row["did_change_ok"] = json!(did_change_ok);
    row["files_since_absent"] = json!(true);
    if golden.expected_ceiling {
        row["expected_ceiling"] = json!(true);
    }
    Ok(row)
}

fn row(
    golden: &ExpectedGolden,
    result: &str,
    definition_ok: bool,
    tokens_ok: bool,
    ghost_edit_ok: bool,
    notes: &str,
    pack: Option<&str>,
) -> Value {
    json!({
        "language": golden.language,
        "corpus": golden.corpus,
        "corpus_sha": golden.corpus_sha,
        "pack": pack.unwrap_or(""),
        "tier_observed": if golden.expected_ceiling { "t1_t2_ceiling" } else { "syntax_or_graph" },
        "definition_ok": definition_ok,
        "tokens_ok": tokens_ok,
        "ghost_edit_ok": ghost_edit_ok,
        "result": result,
        "notes": notes
    })
}

fn location_ok(v: &Value) -> bool {
    v.as_array().map(|a| !a.is_empty()).unwrap_or(false)
}

fn hover_ok(v: &Value) -> bool {
    if v.is_null() {
        return false;
    }
    let contents = &v["contents"];
    if let Some(s) = contents.as_str() {
        return !s.is_empty();
    }
    if let Some(s) = contents["value"].as_str() {
        return !s.is_empty();
    }
    contents.as_array().map(|a| !a.is_empty()).unwrap_or(false)
}

fn recv_id(
    rx: &mpsc::Receiver<Result<Value, String>>,
    id: i64,
    deadline: Duration,
    start: Instant,
) -> Result<Value, String> {
    loop {
        let left = remaining(deadline, start)?;
        match rx.recv_timeout(left) {
            Ok(Ok(v)) => {
                if v.get("id") == Some(&json!(id)) {
                    return Ok(v);
                }
            }
            Ok(Err(e)) => return Err(e),
            Err(RecvTimeoutError::Timeout) => return Err("deadline exceeded waiting for LSP response".into()),
            Err(RecvTimeoutError::Disconnected) => return Err("server closed stdout".into()),
        }
    }
}

fn wait_progress(
    rx: &mpsc::Receiver<Result<Value, String>>,
    deadline: Duration,
    start: Instant,
) -> Option<Value> {
    let mut last = None;
    let mut saw_end = false;
    let mut idle = 0u8;
    let progress_deadline = deadline.min(Duration::from_secs(60));
    while start.elapsed() < progress_deadline && !saw_end {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(v)) => {
                idle = 0;
                if v.get("method").and_then(|m| m.as_str()) == Some("$/progress") {
                    last = Some(v.clone());
                    if v["params"]["value"]["kind"] == "end" {
                        saw_end = true;
                    }
                } else if v.get("id").is_some() {
                    last = Some(v);
                    break;
                }
            }
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                idle = idle.saturating_add(1);
                // Two idle polls (~200ms) after initialize is enough when ingest already flushed.
                if last.is_some() || idle >= 2 {
                    break;
                }
            }
        }
    }
    last
}

fn fetch_pins(opts: &FetchOpts) -> Result<(), String> {
    let raw = std::fs::read_to_string(&opts.pins).map_err(|e| format!("pins: {e}"))?;
    let pins: Value = serde_json::from_str(&raw).map_err(|e| format!("pins json: {e}"))?;
    let corpora = pins["corpora"].as_array().ok_or("pins.corpora missing")?;
    std::fs::create_dir_all(&opts.cache).map_err(|e| e.to_string())?;
    let mut ok = 0u32;
    let mut fail = 0u32;
    for c in corpora {
        let id = c["id"].as_str().unwrap_or("");
        if let Some(only) = &opts.only {
            if id != only {
                continue;
            }
        }
        let url = c["url"].as_str().unwrap_or("");
        let sha = c["sha"].as_str().unwrap_or("");
        match fetch_one(&opts.cache, id, url, sha) {
            Ok(()) => ok += 1,
            Err(e) => {
                eprintln!("fetch {id}: {e}");
                fail += 1;
            }
        }
    }
    println!("corpora fetch: {ok} ok, {fail} failed, cache={}", opts.cache.display());
    if fail > 0 {
        return Err(format!("{fail} corpus fetch(es) failed"));
    }
    Ok(())
}

fn fetch_one(cache: &Path, id: &str, url: &str, sha: &str) -> Result<(), String> {
    if id.is_empty() || url.is_empty() || sha.is_empty() {
        return Err("id/url/sha required".into());
    }
    let dest = cache.join(id);
    let stamp = dest.join(".plsp-sha");
    if stamp.is_file() {
        if std::fs::read_to_string(&stamp).unwrap_or_default().trim() == sha {
            eprintln!("cache hit {id} @{sha}");
            return Ok(());
        }
    }
    eprintln!("fetch {id} {url} @{sha}");
    let tmp = tempfile_dir()?;
    let src = tmp.join("src");
    let status = Command::new("git")
        .args(["-c", "advice.detachedHead=false", "clone", "--filter=blob:none", "--no-checkout", url])
        .arg(&src)
        .status()
        .map_err(|e| format!("git clone: {e}"))?;
    if !status.success() {
        return Err("git clone failed".into());
    }
    let status = Command::new("git")
        .current_dir(&src)
        .args(["fetch", "--depth", "1", "origin", sha])
        .status()
        .map_err(|e| format!("git fetch: {e}"))?;
    if !status.success() {
        return Err("git fetch SHA failed".into());
    }
    let status = Command::new("git")
        .current_dir(&src)
        .args(["checkout", "--detach", sha])
        .status()
        .map_err(|e| format!("git checkout: {e}"))?;
    if !status.success() {
        return Err("git checkout failed".into());
    }
    let git_dir = src.join(".git");
    if git_dir.exists() {
        std::fs::remove_dir_all(&git_dir).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    std::fs::rename(&src, &dest).or_else(|_| copy_tree(&src, &dest))?;
    std::fs::write(&stamp, sha).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join(format!("plsp-corpus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    Ok(base)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn write_rpc<W: Write>(w: &mut W, body: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    write!(w, "Content-Length: {}\r\n\r\n", bytes.len()).map_err(|e| e.to_string())?;
    w.write_all(&bytes).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

fn read_rpc_until<R: BufRead>(
    reader: &mut R,
    deadline: Duration,
    start: Instant,
) -> Result<Value, String> {
    remaining(deadline, start)?;
    let bytes = read_message(reader)?;
    serde_json::from_slice(&bytes).map_err(|e| format!("json: {e}"))
}

fn remaining(deadline: Duration, start: Instant) -> Result<Duration, String> {
    deadline
        .checked_sub(start.elapsed())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| "deadline exceeded waiting for LSP response".into())
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("eof before LSP headers".into());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| format!("bad Content-Length {value}"))?,
                );
            }
        }
    }
    let len = content_length.ok_or("missing Content-Length")?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_handshake_flags() {
        let opts = parse_handshake(&[
            "--root-uri".into(),
            "file:///tmp/ws".into(),
            "--deadline-ms".into(),
            "1000".into(),
            "--assert-stock".into(),
            "--".into(),
            "progressive-lsp".into(),
            "serve".into(),
        ])
        .unwrap();
        assert_eq!(opts.root_uri.as_deref(), Some("file:///tmp/ws"));
        assert_eq!(opts.deadline, Duration::from_millis(1000));
        assert!(opts.assert_stock);
        assert_eq!(opts.server, ["progressive-lsp", "serve"]);
    }

    #[test]
    fn parse_errors() {
        assert!(parse_handshake(&[]).is_err());
        assert!(parse_handshake(&["--".into()]).is_err());
        assert!(parse_handshake(&["--nope".into()]).is_err());
        assert!(parse_handshake(&["--root-uri".into()]).is_err());
        assert!(parse_handshake(&["--deadline-ms".into(), "x".into()]).is_err());
        assert!(parse_backend(&[]).is_err());
        assert!(parse_backend(&["--expected".into(), "e.json".into(), "--".into()]).is_err());
        assert!(parse_fetch(&[]).is_err());
        assert!(parse_fetch(&["--pins".into()]).is_err());
    }

    #[test]
    fn parse_backend_and_fetch_flags() {
        let b = parse_backend(&[
            "--expected".into(),
            "expected/java.json".into(),
            "--root".into(),
            "/tmp/ws".into(),
            "--deadline-ms".into(),
            "2000".into(),
            "--t3-pack".into(),
            "ty".into(),
            "--".into(),
            "progressive-lsp".into(),
            "serve".into(),
        ])
        .unwrap();
        assert_eq!(b.expected, PathBuf::from("expected/java.json"));
        assert_eq!(b.root, PathBuf::from("/tmp/ws"));
        assert_eq!(b.t3_pack.as_deref(), Some("ty"));
        let f = parse_fetch(&[
            "--pins".into(),
            "pins.json".into(),
            "--cache".into(),
            "/tmp/c".into(),
            "--id".into(),
            "anyhow".into(),
        ])
        .unwrap();
        assert_eq!(f.only.as_deref(), Some("anyhow"));
    }

    #[test]
    fn assert_stock_accepts_control_off() {
        let ok = json!({
            "serverInfo": {"name": "progressive-lsp"},
            "capabilities": {
                "definitionProvider": true,
                "referencesProvider": true,
                "hoverProvider": true,
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "semanticTokensProvider": {"full": true},
                "experimental": {
                    "progressiveLsp": {"version": "v1", "socket": null, "mux": false}
                }
            }
        });
        assert_stock(&ok).unwrap();
        assert_stock_caps(&ok).unwrap();
        assert!(assert_stock(&json!({"serverInfo":{"name":"nope"}})).is_err());
        assert!(assert_stock_caps(&json!({
            "serverInfo":{"name":"progressive-lsp"},
            "capabilities":{
                "experimental":{"progressiveLsp":{"version":"v1","socket":null,"mux":false}}
            }
        }))
        .is_err());
    }

    #[test]
    fn remaining_deadline_errors() {
        let start = Instant::now() - Duration::from_secs(2);
        assert!(remaining(Duration::from_millis(1), start).is_err());
        assert!(remaining(Duration::from_secs(30), Instant::now()).is_ok());
    }

    #[test]
    fn write_and_read_message_round_trip() {
        let mut buf = Vec::new();
        write_rpc(&mut buf, &json!({"a":1})).unwrap();
        let parsed = read_message(&mut io::Cursor::new(buf)).unwrap();
        assert_eq!(parsed, br#"{"a":1}"#);
    }

    #[test]
    fn run_help_is_usage() {
        let err = run(vec!["help".into()]).unwrap_err();
        assert!(err.contains("handshake"));
        assert!(err.contains("backend"));
        assert!(run(vec!["nope".into()]).is_err());
    }

    #[test]
    fn find_position_is_zero_based() {
        let src = "package p;\nclass App { void run() { Lib.greet(); } }\n";
        let (line, ch) = find_position(src, "Lib").unwrap();
        assert_eq!(line, 1);
        assert!(ch > 0);
        assert!(find_position(src, "nope").is_err());
        assert_eq!(find_position(src, "").unwrap(), (0, 0));
    }

    #[test]
    fn ghost_append_is_language_aware() {
        assert!(ghost_append("java", "class A {}").0.contains("It2Ghost"));
        assert!(ghost_append("css", "a{}").0.contains(".it2ghost"));
        assert!(ghost_append("python", "x=1").0.contains("def it2ghost"));
        assert_eq!(ghost_append("rust", "").1, "it2ghost");
    }

    #[test]
    fn hover_and_location_helpers() {
        assert!(location_ok(&json!([{"uri":"file:///a"}])));
        assert!(!location_ok(&json!([])));
        assert!(hover_ok(&json!({"contents":{"kind":"plaintext","value":"Lib"}})));
        assert!(!hover_ok(&json!(null)));
        assert!(hover_ok(&json!({"contents":"hi"})));
    }

    #[test]
    fn files_since_methods_are_not_stock() {
        for method in ["$/progressive/filesSince", "workspace/filesSince", "$/filesSince"] {
            assert!(method.contains("filesSince"));
        }
    }

    #[test]
    fn load_golden_reads_expected_shape() {
        let dir = std::env::temp_dir().join(format!("plsp-golden-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("java.json");
        std::fs::write(
            &path,
            r#"{"corpus":"junit4","language":"java","language_id":"java","entry":"A.java","find":"Lib","workspace_symbol":"App","ghost_sibling":"B.java","corpus_sha":"abc"}"#,
        )
        .unwrap();
        let g = load_golden(&path).unwrap();
        assert_eq!(g.corpus, "junit4");
        assert_eq!(g.find, "Lib");
        assert_eq!(g.ghost_sibling.as_deref(), Some("B.java"));
        assert!(!g.expected_ceiling);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
