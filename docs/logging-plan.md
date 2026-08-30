# Logging mutation / refactor plan

Agents implement from this file plus [logging.md](logging.md), [design-patterns.md](design-patterns.md), and the LOG WPs in [implementation-plan.md](implementation-plan.md). Do not start LOG-N+1 until LOG-N is signed off. Branch stack: [branching.md](branching.md) (`log0`–`log4` on **current `main`**, not `poc-no-console`). Pointer payload: [logging/agent-context.md](logging/agent-context.md).

## Current vs target

| Area | Today | After LOG-4 |
|---|---|---|
| `$PREFIX/log/` | mkdir only | WAL sqlite `serve-<ts>-<pid>.sqlite` |
| `LogPort` | missing | core Port; every diagnostic emit |
| `eprintln!` in product libs/bin | `src/main.rs` fatal | gone except CLI usage/help |
| `ConfigLoad.warnings` | dropped in `ServeHost` | `warn` records |
| Engine stderr | no pipe; poc-ide `Stdio::null()` | `ChildStderrAdapter` / `LogFileTailAdapter` when spawn exists |
| `log` / `tracing` | unused by default server features | global bridges in composition root |
| rusqlite | poc-ide only | `progressive-lsp-log` + poc-ide (two schemas) |
| Tests | N/A | `FakeLog` / tempfile; no sleep |

## Crate graph change

```text
progressive-lsp (bin) ──► progressive-lsp-log ──► progressive-lsp-core
         │                         │
         │                         └── rusqlite bundled (static amalgamation)
         └── existing crates ──► progressive-lsp-core  (LogPort only)
```

`progressive-lsp-core` stays sqlite-free. Do not thread `LogPort` through every constructor in LOG-1; LOG-3 is the migrate-call-sites WP. Process-wide `OnceLock<LogPort>` is forbidden.

## File-level mutation

### LOG-0 — Documentation ingest

Docs only. Wire [logging.md](logging.md) and this plan into the docs tree. Draft every LOG type as a [design-patterns.md](design-patterns.md) row. No crates. No rusqlite in server. No `eprintln!` changes. **SIGNED OFF** on `log0`.

### LOG-1 — Port + records (no sqlite in server)

**SIGNED OFF** on `log1`. Core stays sqlite-free. Do not add `progressive-lsp-log` here.

| File | Change |
|---|---|
| `progressive-lsp-core/src/log.rs` (new) | `LogPort`, `LogRecord`, `LogLevel`, `LogOrigin`, `LogComponent`, `LogScope`, `FakeLog`, `MemoryLog`, `NullLog`, `NeverFailLog` |
| `progressive-lsp-core/src/lib.rs` | `pub mod log;` reexports |
| `progressive-lsp-core/src/config.rs` | `[log]` overlay: `level`, `path`; invalid level → warning + default; merge chain |
| `progressive-lsp-core/src/prefix.rs` | no behavior change; tests already cover `log_dir()` |
| `docs/design-patterns.md` | rows for every new type (already drafted on LOG-0) |
| Tests | FakeLog records; scope nest/restore; never-fail NullLog; config `[log]` unknown key still warns |

**Do not** add rusqlite to core. **Do not** change `eprintln!` yet.

### LOG-2 — WAL repository crate

| File | Change |
|---|---|
| `progressive-lsp-log/` (new workspace member) | `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath` |
| workspace `Cargo.toml` | member + pin `rusqlite = { version = "=0.40.2", features = ["bundled"] }` |
| `xtask` / Docker musl | amalgamation links static; `check-static` on the core ELF |
| Tests | tempfile WAL; `PRAGMA journal_mode` returns `wal`; injected COMMIT failure retries without panic; `BATCH_MAX=1`; FakeClock; re-entrancy; Drop flushes |

### LOG-3 — Facades, bridges, eprintln death

**SIGNED OFF** on `log3`. `MemoryLog` bootstrap only; sqlite serve/install wire is LOG-4.

| File | Change |
|---|---|
| `progressive-lsp-log/src/bridges.rs` | `LogCrateBridge`, `TracingBridge` |
| `progressive-lsp-log/src/child_stderr.rs` | `ChildStderrAdapter` + line parser; `FakeChildStderr` |
| `progressive-lsp-log/src/log_file_tail.rs` | `LogFileTailAdapter` (zls/biome) |
| `progressive-lsp-log/src/lsp_message.rs` | `LspLogMessageAdapter` |
| `src/main.rs` | `MemoryLog` bootstrap; no diagnostic `eprintln!` after Facade exists; usage still stderr |
| `src/lib.rs` / `src/serve_host.rs` | pass `LogPort`; emit `ConfigLoad.warnings` |
| `progressive-lsp-engine/src/adapter.rs` | `ChildIo` Value object: stdout LSP + optional stderr pipe |
| `progressive-lsp-engine/src/supervisor.rs` | attach `ChildStderrAdapter` when a pipe exists |
| `progressive-lsp-script/src/host.rs` | extend argv allowlist: clangd `--log=`; optional gopls `-logfile`; **not** `-rpc.trace`, `RA_LOG_FILE`, or `TY_LOG_PROFILE` |
| Product `eprintln!` | replace in `src/` and `progressive-lsp-*` (not xtask, not `integration/harness`, not `bakeoff.rs`) |
| grep gate | product crates contain no diagnostic `eprintln!` |

**poc-ide `StdioLsp` `stderr(Stdio::null())`:** leave in LOG-3 or document; do not inherit engine stderr into the IDE process.

### LOG-4 — Wire serve/install + docs lock

**SIGNED OFF** on `log4`. Stack complete. Do not open `log5`.

| File | Change |
|---|---|
| `src/lib.rs` `run` | bootstrap order from [logging.md](logging.md) |
| `src/session.rs` | `LogScope` around didOpen/didChange/definition |
| `progressive-lsp-index` / `watch` / `install` | scope + `info`/`warn` on silent failure paths |
| `docs/user/README.md` | troubleshooting: sqlite under `$PREFIX/log/` |
| `docs/host-deps.md` | sqlite amalgamation is our artifact |
| `docs/poc-ide/third-party.md` | rusqlite allowed in `progressive-lsp-log` |
| Integration | IT-1.7 usage **still** stderr (`CliUsageAdapter` unchanged); optional sqlite file after `serve` handshake (Linux CI / Docker). Darwin: do not fake musl greens. |

## Call-site rules

```rust
log.info("index package done"); // #[track_caller]
let _g = LogScope::enter(LogScope::new().path(path).operation("textDocument/definition"));
```

Never `eprintln!(...)` on diagnostic paths. Never `emit(...).unwrap()`.

## poc-ide coexistence

Keep `RunLog` (`events` category/event/payload). Share the rusqlite **0.40.2** pin. Do not write poc-ide rows into the server WAL. Post-LOG-4 (not scheduled): read-only query of server logs from the IDE.

## Risk register

| Risk | Mitigation |
|---|---|
| musl + sqlite `DT_NEEDED` | `bundled`; omit loadable extensions; `check-static` sign-off |
| Logging on `didChange` | non-blocking `try_send`; drop oldest; no sqlite on the LSP thread |
| Re-entrant deadlock | Actor + `ReentrancyGuard`; writer never logs |
| Batch lost on crash | `BATCH_MAX=32` + immediate commit on `Error` |
| Engine stdout contamination | never attach stderr Adapter to stdout |
| Secrets in extras | `sanitize_extras`; mutation test |
| Global logger vs tests | `LogCrateBridge` installed only in the bin |

## Out of scope for this stack

xtask `eprintln!`, integration harness fetch progress, Java `bakeoff.rs`. Operator CLIs. If they call a library that takes `LogPort`, pass `NullLog` or `InheritStderrAdapter` **only in those bins**.
