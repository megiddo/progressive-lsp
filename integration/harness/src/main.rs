//! Tiny stdio LSP driver: initialize → initialized → shutdown → exit.
//! Integration only. No `$/` FilesSince. No engine packs required.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const USAGE: &str = "\
plsp-it1 handshake [--root-uri URI] [--deadline-ms N] [--assert-stock] -- <server> [args...]
";

struct HandshakeOpts {
    root_uri: Option<String>,
    deadline: Duration,
    assert_stock: bool,
    server: Vec<String>,
}

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
    if args[0] != "handshake" {
        return Err(format!("unknown command: {}\n{USAGE}", args[0]));
    }
    let opts = parse_handshake(&args[1..])?;
    let result = handshake(&opts)?;
    if opts.assert_stock {
        assert_stock(&result)?;
    }
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
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
    }

    #[test]
    fn assert_stock_accepts_control_off() {
        let ok = json!({
            "serverInfo": {"name": "progressive-lsp"},
            "capabilities": {
                "experimental": {
                    "progressiveLsp": {"version": "v1", "socket": null, "mux": false}
                }
            }
        });
        assert_stock(&ok).unwrap();
        assert!(assert_stock(&json!({"serverInfo":{"name":"nope"}})).is_err());
        assert!(assert_stock(&json!({
            "serverInfo":{"name":"progressive-lsp"},
            "capabilities":{"experimental":{"progressiveLsp":{"version":"nope","socket":null,"mux":false}}}
        }))
        .is_err());
        assert!(assert_stock(&json!({
            "serverInfo":{"name":"progressive-lsp"},
            "capabilities":{"experimental":{"progressiveLsp":{"version":"v1","socket":"/s","mux":false}}}
        }))
        .is_err());
        assert!(assert_stock(&json!({
            "serverInfo":{"name":"progressive-lsp"},
            "capabilities":{"experimental":{"progressiveLsp":{"version":"v1","socket":null,"mux":true}}}
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
        assert!(run(vec!["nope".into()]).is_err());
    }
}
