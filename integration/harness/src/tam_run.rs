//! IT-TAM container runner (TAM-2): load suite YAML, mux serve, meta + FetchTrace.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use progressive_lsp_core::path_to_file_uri;
use serde_json::{json, Value};

use crate::mux_driver::{
    docker_available, docker_image_present, init_params_with_progressive, progressive_lsp_json,
    spawn_mux_docker, trace_rows_to_json, truncate_json, DockerMuxOpts, MuxDriver,
};
use crate::progressive_harness::{
    build_report_row, parse_progressive_meta, should_fetch_trace, TRACE_ROW_CAP,
};
use crate::tam::{load_suite, TamProgressiveLsp, TamReportRow, TamSuite};

pub const TAM_RUN_USAGE: &str = "\
plsp-it1 tam-run --suite PATH --workspace-root DIR \
  [--docker PATH] [--image NAME] [--platform linux/arm64] \
  [--mount-serve-bin PATH] [--init-deadline-ms N] [--request-deadline-ms N] [--quiet]
";

#[derive(Debug, Clone)]
pub struct TamRunOpts {
    pub suite_path: PathBuf,
    pub workspace_root: PathBuf,
    pub docker: PathBuf,
    pub image: String,
    pub platform: Option<String>,
    pub mount_serve_bin: Option<PathBuf>,
    pub prefix: String,
    pub init_deadline: Duration,
    pub request_deadline: Duration,
    pub verbose: bool,
}

pub fn parse_tam_run(args: &[String]) -> Result<TamRunOpts, String> {
    let mut suite_path = None;
    let mut workspace_root = None;
    let mut docker = PathBuf::from("docker");
    let mut image = "progressive-lsp-runtime:local".to_string();
    let mut platform = None;
    let mut mount_serve_bin = None;
    let mut prefix = "/opt/plsp".to_string();
    let mut init_deadline = Duration::from_secs(600);
    let mut request_deadline = Duration::from_secs(15);
    let mut verbose = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--suite" => {
                i += 1;
                suite_path = Some(PathBuf::from(
                    args.get(i).ok_or("--suite requires a path")?,
                ));
            }
            "--workspace-root" => {
                i += 1;
                workspace_root = Some(PathBuf::from(
                    args.get(i).ok_or("--workspace-root requires a path")?,
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
            "--mount-serve-bin" => {
                i += 1;
                mount_serve_bin = Some(PathBuf::from(
                    args.get(i)
                        .ok_or("--mount-serve-bin requires a path")?,
                ));
            }
            "--prefix" => {
                i += 1;
                prefix = args.get(i).ok_or("--prefix requires a value")?.clone();
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
            "--request-deadline-ms" => {
                i += 1;
                let ms: u64 = args
                    .get(i)
                    .ok_or("--request-deadline-ms requires a value")?
                    .parse()
                    .map_err(|_| "request-deadline-ms must be integer")?;
                request_deadline = Duration::from_millis(ms);
            }
            "--verbose" => verbose = true,
            "--quiet" => verbose = false,
            other => return Err(format!("unknown flag: {other}\n{TAM_RUN_USAGE}")),
        }
        i += 1;
    }
    Ok(TamRunOpts {
        suite_path: suite_path.ok_or("tam-run requires --suite")?,
        workspace_root: workspace_root.ok_or("tam-run requires --workspace-root")?,
        docker,
        image,
        platform,
        mount_serve_bin,
        prefix,
        init_deadline,
        request_deadline,
        verbose,
    })
}

pub fn run_tam(opts: &TamRunOpts) -> Result<Value, String> {
    let suite = load_suite(&opts.suite_path)?;
    let image = suite
        .server
        .as_ref()
        .and_then(|s| s.docker_image.clone())
        .unwrap_or_else(|| opts.image.clone());
    let platform = suite
        .server
        .as_ref()
        .and_then(|s| s.platform.clone())
        .or_else(|| opts.platform.clone());
    let prefix = suite
        .server
        .as_ref()
        .and_then(|s| s.prefix_in_container.clone())
        .unwrap_or_else(|| opts.prefix.clone());

    if !docker_available(&opts.docker) {
        return Ok(skip_report(&opts, &suite, "docker missing on host"));
    }
    if !docker_image_present(&opts.docker, &image) {
        return Ok(skip_report(
            &opts,
            &suite,
            &format!("docker image {image} not present"),
        ));
    }

    let docker_mux = DockerMuxOpts {
        docker: opts.docker.clone(),
        image,
        platform,
        wal: None,
        mount_serve_bin: opts.mount_serve_bin.clone(),
        prefix,
    };

    let driver = spawn_mux_docker(&opts.workspace_root, &docker_mux, opts.verbose)?;
    run_suite_on_driver(&driver, &suite, opts)
}

fn skip_report(opts: &TamRunOpts, suite: &TamSuite, notes: &str) -> Value {
    json!({
        "rpc": "tam-run",
        "suite": opts.suite_path.display().to_string(),
        "suite_id": suite.id,
        "result": "skip",
        "notes": notes,
        "rows": Vec::<TamReportRow>::new(),
        "cases": suite.cases.len(),
    })
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

fn merge_progressive(
    session: Option<&TamProgressiveLsp>,
    per: Option<&TamProgressiveLsp>,
) -> TamProgressiveLsp {
    let mut out = session.cloned().unwrap_or_default();
    if let Some(p) = per {
        if p.emit_result_meta.is_some() {
            out.emit_result_meta = p.emit_result_meta;
        }
        if p.emit_timing.is_some() {
            out.emit_timing = p.emit_timing;
        }
        if p.max_chain_iter.is_some() {
            out.max_chain_iter = p.max_chain_iter;
        }
        if p.max_tier.is_some() {
            out.max_tier = p.max_tier.clone();
        }
    }
    out
}

pub fn run_suite_on_driver(
    driver: &MuxDriver,
    suite: &TamSuite,
    opts: &TamRunOpts,
) -> Result<Value, String> {
    let root = opts.workspace_root.canonicalize().map_err(|e| e.to_string())?;
    let root_uri = path_to_file_uri(&root);
    let session_plsp = suite.session.as_ref().and_then(|s| s.progressive_lsp.as_ref());
    let init_deadline = driver.start + opts.init_deadline;

    let init_result = driver.request_lsp(
        1,
        "initialize",
        init_params_with_progressive(&root_uri, session_plsp),
        init_deadline,
    );

    match init_result {
        Ok(r) => {
            driver.trace("initialize_ok", truncate_json(&r, 300));
            let mux = &r["capabilities"]["experimental"]["progressiveLsp"]["mux"];
            if mux != &json!(true) {
                return Ok(tam_fail_report(
                    opts,
                    suite,
                    format!("expected progressiveLsp.mux true, got {mux}"),
                    vec![],
                ));
            }
        }
        Err(e) => {
            return Ok(tam_fail_report(
                opts,
                suite,
                format!("initialize: {e}"),
                vec![],
            ));
        }
    }

    driver.notify_lsp("initialized", json!({}))?;

    let emit_meta = session_plsp
        .and_then(|s| s.emit_result_meta)
        .unwrap_or(false);
    let emit_timing = session_plsp
        .and_then(|s| s.emit_timing)
        .unwrap_or(false);
    if emit_meta {
        let _ = driver.index_status_with_meta(emit_meta, emit_timing, 10, init_deadline);
    }

    let mut rows: Vec<TamReportRow> = Vec::new();
    let mut next_id: i64 = 2;
    let mut control_id: u64 = 100;
    let request_deadline = || Instant::now() + opts.request_deadline;

    for case in &suite.cases {
        let rel = case
            .file
            .as_deref()
            .ok_or("tam case requires file")?;
        let entry = root.join(rel);
        if !entry.is_file() {
            rows.push(TamReportRow {
                method: "(case)".into(),
                trace_id: None,
                meta_tier: None,
                backend_version: None,
                resolve_ms: None,
                trace_row_count: None,
                result: "fail".into(),
                notes: Some(format!("missing file {}", entry.display())),
                trace_rows: None,
            });
            continue;
        }
        let src = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
        let needle = case
            .anchor
            .as_ref()
            .and_then(|a| a.find.as_deref())
            .unwrap_or("");
        let (line, character) = crate::find_position(&src, needle)?;
        let uri = path_to_file_uri(&entry);

        driver.notify_lsp(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id(&entry),
                    "version": 1,
                    "text": src,
                }
            }),
        )?;

        for api in &case.apis {
            let merged = merge_progressive(session_plsp, api.progressive_lsp.as_ref());
            let mut params = match api.method.as_str() {
                "textDocument/definition"
                | "textDocument/references"
                | "textDocument/hover"
                | "textDocument/implementation"
                | "textDocument/typeDefinition" => json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                }),
                "textDocument/documentSymbol" | "textDocument/semanticTokens/full" => {
                    json!({ "textDocument": { "uri": uri } })
                }
                "workspace/symbol" => json!({
                    "query": api.query.as_deref().unwrap_or(needle),
                }),
                other => {
                    rows.push(TamReportRow {
                        method: other.to_string(),
                        trace_id: None,
                        meta_tier: None,
                        backend_version: None,
                        resolve_ms: None,
                        trace_row_count: None,
                        result: "skip".into(),
                        notes: Some("unsupported method in tam-run v2".into()),
                        trace_rows: None,
                    });
                    continue;
                }
            };
            if !progressive_lsp_json(&merged).as_object().unwrap().is_empty() {
                params["progressiveLsp"] = progressive_lsp_json(&merged);
            }
            if api.method == "textDocument/references" {
                params["context"] = json!({ "includeDeclaration": true });
            }

            let id = next_id;
            next_id += 1;
            let lsp_result = driver.request_lsp(
                id,
                &api.method,
                params,
                request_deadline(),
            );

            let (lsp_result, lsp_err) = match lsp_result {
                Ok(v) => (v, None),
                Err(e) => (Value::Null, Some(e)),
            };

            let mut row = if let Some(err) = lsp_err {
                TamReportRow {
                    method: api.method.clone(),
                    trace_id: None,
                    meta_tier: None,
                    backend_version: None,
                    resolve_ms: None,
                    trace_row_count: None,
                    result: "fail".into(),
                    notes: Some(err),
                    trace_rows: None,
                }
            } else {
                build_report_row(
                    &api.method,
                    &lsp_result,
                    api.expect.as_ref(),
                    api.trace.as_ref(),
                    None,
                )
            };

            if should_fetch_trace(api.trace.as_ref(), &row.result) {
                let trace_id = row
                    .trace_id
                    .clone()
                    .or_else(|| parse_progressive_meta(&lsp_result).trace_id);
                if let Some(tid) = trace_id.filter(|s| !s.is_empty()) {
                    control_id += 1;
                    match driver.fetch_trace(&tid, 500, control_id, request_deadline()) {
                        Ok(resp) => {
                            let count = resp.rows.len() as u32;
                            row.trace_row_count = Some(count);
                            row.trace_rows =
                                Some(trace_rows_to_json(&resp.rows, TRACE_ROW_CAP));
                        }
                        Err(e) => {
                            row.result = "fail".into();
                            row.notes = Some(format!(
                                "{}; FetchTrace: {e}",
                                row.notes.unwrap_or_default()
                            ));
                        }
                    }
                }
            }

            rows.push(row);
        }
    }

    let overall = if rows.iter().any(|r| r.result == "fail") {
        "fail"
    } else if rows.is_empty() {
        "skip"
    } else {
        "pass"
    };

    Ok(json!({
        "rpc": "tam-run",
        "suite": opts.suite_path.display().to_string(),
        "suite_id": suite.id,
        "workspace_root": root.display().to_string(),
        "result": overall,
        "cases": suite.cases.len(),
        "rows": rows,
        "elapsed_ms": driver.start.elapsed().as_millis(),
        "trace": driver.take_trace(),
    }))
}

fn tam_fail_report(opts: &TamRunOpts, suite: &TamSuite, notes: String, rows: Vec<TamReportRow>) -> Value {
    json!({
        "rpc": "tam-run",
        "suite": opts.suite_path.display().to_string(),
        "suite_id": suite.id,
        "result": "fail",
        "notes": notes,
        "cases": suite.cases.len(),
        "rows": rows,
    })
}
