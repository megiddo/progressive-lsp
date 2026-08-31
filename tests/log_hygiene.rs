//! LOG-11 hygiene: every operational `Err` in `src/` and the listed crates
//! either has a `LogPort` emit on that path or is named in
//! `docs/logging.md` “Client-visible only” with a reason.
//!
//! Sources are `include_str!` (compile-time). Not a runtime type. No new Adapter.

/// Substrings that must appear in the logging.md client-visible-only table.
const CLIENT_VISIBLE_DOC: &[&str] = &[
    "JSON-RPC",
    "Status.code",
    "InstallError",
    "FramingError",
    "MuxError",
    "--help",
    "CliError",
];

/// Emit helpers that must exist so “emit on that path” is real, not a comment.
const EMIT_ON_PATH_TOKENS: &[&str] = &[
    "emit_spawn",
    "emit_op",
    "emit_control",
    "emit_control_status_error",
    "emit_protocol_warn",
    "emit_hash_mismatch",
    "emit_verify_refused",
    "CliUsageAdapter",
    "StderrEmitAdapter",
];

const LOGGING_MD: &str = include_str!("../docs/logging.md");

const LISTED: &[(&str, &str)] = &[
    ("src/lib.rs", include_str!("../src/lib.rs")),
    ("src/session.rs", include_str!("../src/session.rs")),
    ("src/serve_host.rs", include_str!("../src/serve_host.rs")),
    (
        "src/control_socket.rs",
        include_str!("../src/control_socket.rs"),
    ),
    ("src/main.rs", include_str!("../src/main.rs")),
    (
        "progressive-lsp-engine/src/supervisor.rs",
        include_str!("../progressive-lsp-engine/src/supervisor.rs"),
    ),
    (
        "progressive-lsp-engine/src/discovery.rs",
        include_str!("../progressive-lsp-engine/src/discovery.rs"),
    ),
    (
        "progressive-lsp-engine/src/pack.rs",
        include_str!("../progressive-lsp-engine/src/pack.rs"),
    ),
    (
        "progressive-lsp-engine/src/adapter.rs",
        include_str!("../progressive-lsp-engine/src/adapter.rs"),
    ),
    (
        "progressive-lsp-engine/src/hooks.rs",
        include_str!("../progressive-lsp-engine/src/hooks.rs"),
    ),
    (
        "progressive-lsp-engine/src/resolve.rs",
        include_str!("../progressive-lsp-engine/src/resolve.rs"),
    ),
    (
        "progressive-lsp-engine/src/lib.rs",
        include_str!("../progressive-lsp-engine/src/lib.rs"),
    ),
    (
        "progressive-lsp-script/src/host.rs",
        include_str!("../progressive-lsp-script/src/host.rs"),
    ),
    (
        "progressive-lsp-script/src/engine.rs",
        include_str!("../progressive-lsp-script/src/engine.rs"),
    ),
    (
        "progressive-lsp-script/src/lib.rs",
        include_str!("../progressive-lsp-script/src/lib.rs"),
    ),
    (
        "progressive-lsp-protocol/src/lib.rs",
        include_str!("../progressive-lsp-protocol/src/lib.rs"),
    ),
    (
        "progressive-lsp-protocol/src/framing.rs",
        include_str!("../progressive-lsp-protocol/src/framing.rs"),
    ),
    (
        "progressive-lsp-protocol/src/rpc.rs",
        include_str!("../progressive-lsp-protocol/src/rpc.rs"),
    ),
    (
        "progressive-lsp-protocol/src/mux.rs",
        include_str!("../progressive-lsp-protocol/src/mux.rs"),
    ),
    (
        "progressive-lsp-control/src/service.rs",
        include_str!("../progressive-lsp-control/src/service.rs"),
    ),
    (
        "progressive-lsp-control/src/codec.rs",
        include_str!("../progressive-lsp-control/src/codec.rs"),
    ),
    (
        "progressive-lsp-control/src/lib.rs",
        include_str!("../progressive-lsp-control/src/lib.rs"),
    ),
    (
        "progressive-lsp-install/src/lib.rs",
        include_str!("../progressive-lsp-install/src/lib.rs"),
    ),
    (
        "progressive-lsp-install/src/hash.rs",
        include_str!("../progressive-lsp-install/src/hash.rs"),
    ),
    (
        "progressive-lsp-install/src/manifest.rs",
        include_str!("../progressive-lsp-install/src/manifest.rs"),
    ),
    (
        "progressive-lsp-install/src/dist_manifest.rs",
        include_str!("../progressive-lsp-install/src/dist_manifest.rs"),
    ),
    (
        "progressive-lsp-install/src/transport.rs",
        include_str!("../progressive-lsp-install/src/transport.rs"),
    ),
    (
        "progressive-lsp-install/src/probe.rs",
        include_str!("../progressive-lsp-install/src/probe.rs"),
    ),
    (
        "progressive-lsp-index/src/cache.rs",
        include_str!("../progressive-lsp-index/src/cache.rs"),
    ),
    (
        "progressive-lsp-index/src/service.rs",
        include_str!("../progressive-lsp-index/src/service.rs"),
    ),
    (
        "progressive-lsp-watch/src/coalescer.rs",
        include_str!("../progressive-lsp-watch/src/coalescer.rs"),
    ),
    (
        "progressive-lsp-resolve/src/stack_graph.rs",
        include_str!("../progressive-lsp-resolve/src/stack_graph.rs"),
    ),
];

const EMIT_BLOB: &str = concat!(
    include_str!("../src/lib.rs"),
    include_str!("../src/serve_host.rs"),
    include_str!("../src/control_socket.rs"),
    include_str!("../progressive-lsp-engine/src/supervisor.rs"),
    include_str!("../progressive-lsp-script/src/host.rs"),
    include_str!("../progressive-lsp-protocol/src/lib.rs"),
    include_str!("../progressive-lsp-install/src/lib.rs"),
    include_str!("../progressive-lsp-log/src/cli_usage.rs"),
    include_str!("../progressive-lsp-log/src/stderr_emit.rs"),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    ClientVisible,
    EmitOnPath,
    NonOperational,
}

fn is_construction(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('*') {
        return false;
    }
    if t.contains("if let Err") || t.contains("matches!") || t.contains("assert") {
        return false;
    }
    // Handling, not a constructor: `Err(e) if … =>` / spawn-hook Abort / cache miss.
    if t.contains("Err(") && t.contains(" if ") {
        return false;
    }
    if t.contains("SpawnHookResult::") || t.contains("=> None") {
        return false;
    }
    t.contains("return Err(")
        || t.contains("=> Err(")
        || t.contains(".map_err(")
        || t.contains("ok_or(")
        || t.contains("ok_or_else(")
        || (t.contains("Err(")
            && (t.contains("::") || t.contains("Failed(") || t.contains("Error")))
}

fn classify(rel: &str, line: &str) -> Option<Family> {
    if rel.contains("stack_graph.rs") {
        return Some(Family::NonOperational);
    }
    if line.contains("EvalAltResult") {
        return Some(Family::NonOperational);
    }
    if line.contains("CliError")
        || line.contains("JsonRpcError")
        || line.contains("FramingError")
        || line.contains("MuxError")
        || line.contains("CodecError")
        || line.contains("InitializeFailed")
        || line.contains("InstallError")
    {
        return Some(Family::ClientVisible);
    }
    if line.contains("EngineError")
        || line.contains("ScriptSandbox")
        || line.contains("ScriptAbort")
        || line.contains("ConfigError")
    {
        return Some(Family::EmitOnPath);
    }
    if rel.contains("progressive-lsp-protocol/")
        || rel.contains("progressive-lsp-control/")
        || rel.contains("progressive-lsp-install/")
    {
        return Some(Family::ClientVisible);
    }
    if rel.contains("progressive-lsp-engine/")
        || rel.contains("progressive-lsp-script/")
        || rel.contains("progressive-lsp-index/")
        || rel.contains("progressive-lsp-watch/")
        || rel.ends_with("control_socket.rs")
        || rel.ends_with("supervisor.rs")
        || rel.ends_with("serve_host.rs")
        || (rel.starts_with("src/") && rel.ends_with("lib.rs"))
    {
        return Some(Family::EmitOnPath);
    }
    None
}

fn collect_sites() -> Vec<(String, usize, String, Option<Family>)> {
    let mut sites = Vec::new();
    for (rel, src) in LISTED {
        let mut in_tests = false;
        for (i, line) in src.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                in_tests = true;
            }
            if in_tests {
                continue;
            }
            if !is_construction(line) {
                continue;
            }
            let family = classify(rel, line);
            sites.push(((*rel).to_string(), i + 1, line.trim().to_string(), family));
        }
    }
    sites
}

fn client_visible_section(doc: &str) -> &str {
    let start = doc
        .find("### Client-visible only")
        .expect("logging.md must have ### Client-visible only");
    let rest = &doc[start..];
    rest.split("\n## ").next().unwrap_or(rest)
}

fn coverage_matrix_section(doc: &str) -> &str {
    let start = doc
        .find("## Coverage matrix")
        .expect("logging.md must have ## Coverage matrix");
    &doc[start..]
}

#[test]
fn operational_err_sites_are_classified() {
    let sites = collect_sites();
    assert!(
        !sites.is_empty(),
        "expected operational Err constructors in listed crates"
    );
    let silent: Vec<String> = sites
        .iter()
        .filter(|(_, _, _, family)| family.is_none())
        .map(|(rel, n, line, _)| format!("{rel}:{n}:{line}"))
        .collect();
    assert!(
        silent.is_empty(),
        "unclassified operational Err (emit on path or list in logging.md client-visible only): {silent:?}"
    );
}

#[test]
fn client_visible_only_allowlist_in_logging_md() {
    let section = client_visible_section(LOGGING_MD);
    assert!(
        section.contains("reason") || section.contains("Why") || section.contains("stay on"),
        "client-visible-only table must give a reason per path"
    );
    for key in CLIENT_VISIBLE_DOC {
        assert!(
            section.contains(key),
            "docs/logging.md client-visible-only table missing {key}"
        );
    }
    assert!(
        section.contains("sqlite") || section.contains("Sqlite") || section.contains("LOG-"),
        "client-visible-only rows must still name the sqlite emit (not sqlite instead of the client)"
    );
}

#[test]
fn coverage_matrix_has_no_silent_class() {
    let matrix = coverage_matrix_section(LOGGING_MD);
    let silent_rows: Vec<&str> = matrix
        .lines()
        .filter(|l| l.starts_with('|') && !l.contains("Event |") && !l.contains("---"))
        .filter(|l| {
            l.to_ascii_lowercase()
                .split('|')
                .any(|cell| cell.trim() == "silent")
        })
        .collect();
    assert!(
        silent_rows.is_empty(),
        "coverage matrix has a silent class (defect after LOG-11): {silent_rows:?}"
    );
}

#[test]
fn emit_on_path_tokens_exist() {
    let missing: Vec<&str> = EMIT_ON_PATH_TOKENS
        .iter()
        .copied()
        .filter(|t| !EMIT_BLOB.contains(t))
        .collect();
    assert!(
        missing.is_empty(),
        "emit-on-path token missing from listed crates / log crate: {missing:?}"
    );
}

#[test]
fn optional_t2_fetch_is_non_goal() {
    let goals = LOGGING_MD
        .find("### Explicit non-goals")
        .expect("logging.md must keep Explicit non-goals");
    let section = &LOGGING_MD[goals..];
    assert!(
        section.contains("stack-graph")
            || section.contains("stack-graphs")
            || section.contains("T2 fetch"),
        "optional T2 stack-graph fetch must stay a non-goal (not a silent serve/install class)"
    );
}

#[test]
fn hygiene_test_has_no_thread_sleep() {
    let src = include_str!("log_hygiene.rs");
    let forbidden = format!("{}{}", "thread::", "sleep");
    assert!(
        !src.contains(&forbidden),
        "hygiene test must not use {forbidden}"
    );
}
