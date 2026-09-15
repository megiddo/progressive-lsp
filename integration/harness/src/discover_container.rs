//! POC-IDE parity: `docker run … progressive-lsp-runtime:local serve --mux`, then the
//! same LSP sequence as **Find Definition** (initialize → initialized → didOpen →
//! `textDocument/definition` on channel 0). Verbose trace on stderr; JSON report on stdout.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use progressive_lsp_core::path_to_file_uri;
use serde_json::{json, Value};

use crate::mux_driver::{
    init_params_with_progressive, progressive_lsp_json, spawn_mux_docker, trace_rows_to_json,
    truncate_json, DockerMuxOpts, MuxDriver,
};
use crate::progressive_harness::{
    parse_progressive_meta, should_fetch_trace, TRACE_ROW_CAP,
};
use crate::tam::TamProgressiveLsp;

pub const DISCOVER_CONTAINER_USAGE: &str = "\
plsp-it1 discover-container --root DIR --expected JSON [--docker PATH] [--image NAME] \
  [--platform linux/arm64] [--wal PATH] [--init-deadline-ms N] [--discover-deadline-ms N] \
  [--emit-result-meta] [--emit-timing] [--fetch-trace-on-fail] [--verbose]
";

#[derive(Debug, Clone)]
pub struct DiscoverContainerOpts {
    pub root: PathBuf,
    pub expected: PathBuf,
    pub docker: PathBuf,
    pub image: String,
    pub platform: Option<String>,
    pub wal: Option<PathBuf>,
    pub mount_serve_bin: Option<PathBuf>,
    pub init_deadline: Duration,
    pub discover_deadline: Duration,
    pub emit_result_meta: bool,
    pub emit_timing: bool,
    pub fetch_trace_on_fail: bool,
    pub verbose: bool,
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
    let mut emit_result_meta = false;
    let mut emit_timing = false;
    let mut fetch_trace_on_fail = true;
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
            "--emit-result-meta" => emit_result_meta = true,
            "--emit-timing" => emit_timing = true,
            "--fetch-trace-on-fail" => fetch_trace_on_fail = true,
            "--no-fetch-trace-on-fail" => fetch_trace_on_fail = false,
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
        emit_result_meta,
        emit_timing,
        fetch_trace_on_fail,
        verbose,
    })
}

pub fn run_discover_container(opts: &DiscoverContainerOpts) -> Result<Value, String> {
    let golden = crate::load_golden(&opts.expected)?;
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
    let (line, character) = crate::find_position(&src, &golden.find)?;
    let file_path = entry.clone();
    let uri = path_to_file_uri(&file_path);
    let root = opts.root.canonicalize().map_err(|e| e.to_string())?;
    let root_uri = path_to_file_uri(&root);

    let docker_mux = DockerMuxOpts {
        docker: opts.docker.clone(),
        image: opts.image.clone(),
        platform: opts.platform.clone(),
        wal: opts.wal.clone(),
        mount_serve_bin: opts.mount_serve_bin.clone(),
        prefix: "/opt/plsp".into(),
    };

    let driver = spawn_mux_docker(&root, &docker_mux, opts.verbose)?;
    driver.trace(
        "setup",
        format!(
            "root={root_uri} entry={uri} find={} @ {line}:{character}",
            golden.find
        ),
    );

    let session_plsp = if opts.emit_result_meta || opts.emit_timing {
        Some(TamProgressiveLsp {
            emit_result_meta: Some(opts.emit_result_meta),
            emit_timing: Some(opts.emit_timing),
            ..Default::default()
        })
    } else {
        None
    };

    let init_deadline = driver.start + opts.init_deadline;
    let init_result = driver.request_lsp(
        1,
        "initialize",
        init_params_with_progressive(&root_uri, session_plsp.as_ref()),
        init_deadline,
    );

    let init_result = match init_result {
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
                    None,
                    None,
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
                None,
                None,
                &golden,
                line,
                character,
                &uri,
            ));
        }
    };

    driver.notify_lsp("initialized", json!({}))?;

    if opts.emit_result_meta {
        if let Ok(idx) = driver.index_status_with_meta(
            opts.emit_result_meta,
            opts.emit_timing,
            10,
            init_deadline,
        ) {
            let tiers = driver.tier_status(11, init_deadline).ok();
            driver.trace(
                "control_snapshot",
                format!(
                    "packages={} tier_rows={} trace_id={}",
                    idx.packages.len(),
                    tiers.as_ref().map(|t| t.rows.len()).unwrap_or(0),
                    idx.progressive_meta
                        .as_ref()
                        .map(|m| m.trace_id.as_str())
                        .unwrap_or(""),
                ),
            );
        }
    } else if let Ok(idx) = driver.index_status_with_meta(false, false, 10, init_deadline) {
        let _ = driver.tier_status(11, init_deadline);
        driver.trace(
            "control_snapshot",
            format!("packages={}", idx.packages.len()),
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

    let mut def_progressive = TamProgressiveLsp {
        emit_result_meta: Some(opts.emit_result_meta),
        emit_timing: Some(opts.emit_timing),
        ..Default::default()
    };
    let mut def_params = json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
    });
    if opts.emit_result_meta || opts.emit_timing {
        def_params["progressiveLsp"] = progressive_lsp_json(&def_progressive);
    }

    let def_result = driver.request_lsp(
        2,
        "textDocument/definition",
        def_params,
        discover_deadline,
    );

    let (def_value, def_trace_id, def_fetch) = match def_result {
        Ok(loc) => {
            let count = location_count(&loc);
            driver.trace(
                "discover_ok",
                format!(
                    "definition locations={count} raw={}",
                    truncate_json(&loc, 400)
                ),
            );
            let meta = parse_progressive_meta(&loc);
            let fetch = attach_fetch_trace(
                &driver,
                opts,
                meta.trace_id.as_deref(),
                "pass",
                false,
                20,
                discover_deadline,
            );
            (Some(loc), meta.trace_id, fetch)
        }
        Err(e) => {
            let fetch = if opts.fetch_trace_on_fail {
                attach_fetch_trace(
                    &driver,
                    opts,
                    None,
                    "fail",
                    true,
                    21,
                    discover_deadline,
                )
            } else {
                (None, None)
            };
            return Ok(finish_report(
                &driver,
                opts,
                "fail",
                format!("definition: {e}"),
                None,
                None,
                0,
                false,
                fetch.0,
                fetch.1,
                &golden,
                line,
                character,
                &uri,
            ));
        }
    };

    let def_count = def_value.as_ref().map(location_count).unwrap_or(0);
    let _init_caps = init_result;

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

    let want_trace = should_fetch_trace(None, result_tag)
        || (opts.fetch_trace_on_fail && result_tag == "fail");
    let fetch = if want_trace {
        attach_fetch_trace(
            &driver,
            opts,
            def_trace_id.as_deref(),
            result_tag,
            opts.fetch_trace_on_fail,
            22,
            discover_deadline,
        )
    } else {
        (None, None)
    };

    Ok(finish_report(
        &driver,
        opts,
        result_tag,
        notes,
        def_value,
        refs_value,
        refs_count,
        refs_ok,
        def_trace_id.or(fetch.0),
        fetch.1,
        &golden,
        line,
        character,
        &uri,
    ))
}

fn attach_fetch_trace(
    driver: &MuxDriver,
    opts: &DiscoverContainerOpts,
    trace_id: Option<&str>,
    result_tag: &str,
    on_fail: bool,
    control_id: u64,
    deadline: Instant,
) -> (Option<String>, Option<Value>) {
    if !on_fail && result_tag == "fail" {
        // caller handles fail path
    }
    if !should_fetch_trace(None, if result_tag == "fail" { "fail" } else { "pass" })
        && !(opts.fetch_trace_on_fail && result_tag == "fail")
    {
        return (trace_id.map(str::to_string), None);
    }
    let Some(tid) = trace_id.filter(|s| !s.is_empty()) else {
        return (None, None);
    };
    match driver.fetch_trace(tid, 500, control_id, deadline) {
        Ok(resp) => (
            Some(tid.to_string()),
            Some(json!({
                "trace_id": tid,
                "trace_row_count": resp.rows.len(),
                "trace_rows": trace_rows_to_json(&resp.rows, TRACE_ROW_CAP),
            })),
        ),
        Err(e) => (
            Some(tid.to_string()),
            Some(json!({ "trace_id": tid, "fetch_error": e })),
        ),
    }
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

fn location_count(v: &Value) -> usize {
    let v = crate::progressive_harness::lsp_result_value(v);
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
    trace_id: Option<String>,
    trace_dump: Option<Value>,
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
        "emit_result_meta": opts.emit_result_meta,
        "emit_timing": opts.emit_timing,
        "trace_id": trace_id,
        "trace_dump": trace_dump,
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
