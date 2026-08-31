# Logging mutation / refactor plan

Agents implement from this file plus [logging.md](logging.md), [design-patterns.md](design-patterns.md), and the LOG WPs in [implementation-plan.md](implementation-plan.md). Do not start LOG-N+1 until LOG-N is signed off. Branch stack: [branching.md](branching.md) (`log0`–`log11`; `log0`–`log5` **signed off**; `log0`–`log4` on **current `main`**, not `poc-no-console`; parent of `log5` is `log4`). Pointer payload: [logging/agent-context.md](logging/agent-context.md).

LOG-0–LOG-10 are **signed off**. Do not reopen them. LOG-11 closes remaining silent paths so `$PREFIX/log/serve-*.sqlite` answers “why isn’t this working?”

## Current vs target

| Area | After LOG-4 (today) | After LOG-11 |
|---|---|---|
| `$PREFIX/log/` | WAL `serve-<ts>-<pid>.sqlite`; open fail → `MemoryLog` dies with process | Same WAL; `LogOpenPlan` fallback then temp WAL; `MemoryLog` only if all three fail |
| `EngineSupervisor` | No `LogPort`; `last_error` in-memory; serve **drops** the supervisor | `with_log`; spawn/crash/backoff/abort/hash/stub refuse emit; serve **holds** supervisor and `try_spawn`s |
| `PackAdapter::spawn` | Refuses `Command` (stub / Linux CI) with no sqlite row | Same refuse; **warn** `operation=spawn`. No real `Command` on this stack |
| Child capture | LOG-10: `FakeChildStderr` drain → FakeLog; attach when a `Read` exists | Same. No Darwin spawn. `PackAdapter` still refuses `Command` |
| `ScriptHost` | No `LogPort`; Abort → client `InitializeFailed` only | `with_log`; bootstrap abort / spawn skip / pre_index skip emit |
| Protocol / control | JSON-RPC / Envelope errors client-only | `warn`/`info` `operation=protocol`/`control`; **no** bodies |
| T3 `NotReady` | Silent fallback to T2/T1 | Once per `(language, package)` `info` `operation=resolve` |
| Install hash / verify | `Err` to CLI; sqlite only `remove_file` | `warn` `operation=install` with expected/actual hex |
| Tests | `FakeLog` / tempfile; no sleep | Same; named FakeLog asserts on warn message / operation / component |

## Crate graph change

```text
progressive-lsp (bin) ──► progressive-lsp-log ──► progressive-lsp-core
         │                         │
         │                         └── rusqlite bundled (static amalgamation)
         └── existing crates ──► progressive-lsp-core  (LogPort only)
         └── progressive-lsp-engine ──► progressive-lsp-log  (capture Adapters only; no Connection)
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

**SIGNED OFF** on `log4`. Parent of `log5`. Do not reopen LOG-0–LOG-4. Remaining coverage is LOG-5+.

| File | Change |
|---|---|
| `src/lib.rs` `run` | bootstrap order from [logging.md](logging.md) |
| `src/session.rs` | `LogScope` around didOpen/didChange/definition |
| `progressive-lsp-index` / `watch` / `install` | scope + `info`/`warn` on silent failure paths |
| `docs/user/README.md` | troubleshooting: sqlite under `$PREFIX/log/` |
| `docs/host-deps.md` | sqlite amalgamation is our artifact |
| `docs/poc-ide/third-party.md` | rusqlite allowed in `progressive-lsp-log` |
| Integration | IT-1.7 usage **still** stderr (`CliUsageAdapter` unchanged); optional sqlite file after `serve` handshake (Linux CI / Docker). Darwin: do not fake musl greens. |

### LOG-5 — Documentation ingest (remaining coverage)

Docs only. Coverage matrix, LOG-6–LOG-11 mutation, pattern rows, and stack. No crates. No Rust. **SIGNED OFF** on `log5`.

| File | Change |
|---|---|
| `docs/logging.md` | Remaining problem; coverage matrix; `LogOpenPlan`; LOG-5+ pointer. Locked decisions unchanged |
| `docs/logging-plan.md` | This section |
| `docs/logging/agent-context.md` | `log5`–`log11` in READ/BRANCH |
| `docs/milestones.md` / `docs/implementation-plan.md` / `docs/branching.md` | LOG-5+ exits and WPs; supersede “do not open log5” |
| `docs/design-patterns.md` | `LogOpenPlan`; existing rows for supervisor / ScriptHost / LspFacade / ServeLogPath |
| `docs/user/README.md` | Fallback WAL troubleshooting (design; code is LOG-9) |

**Do not** change `eprintln!`. **Do not** add types in Rust. **Do not** implement `PackAdapter` `Command`.

### LOG-6 — Supervisor + ScriptHost lifecycle

**SIGNED OFF** on `log6`. Inject `Arc<dyn LogPort>` like `ClockPort`. Emit spawn/crash/backoff/abort at **warn** (failures) or **info** (skip/backoff/success). Serve must **hold** the supervisor and `try_spawn` so stub refuse is a row.

| File | Change |
|---|---|
| `progressive-lsp-engine/src/supervisor.rs` | `with_log`; emit on `try_spawn` (`NotDiscovered`, `Hash`, `Aborted`, `Backoff`, `Spawn` including stub/reserved), `note_crash_err`, `Ok(true)` |
| `progressive-lsp-engine/src/discovery.rs` | Do not swallow `EngineError::Hash` in `discover_pack_opt` without an emit (supervisor sees the `Result` or `discover_pack` is called) |
| `progressive-lsp-engine/src/pack.rs` | No `Command`. Supervisor already logs `EngineError::Spawn` text |
| `progressive-lsp-engine/src/hooks.rs` | `ScriptHookBridge` unchanged besides supervisor emit of Abort |
| `progressive-lsp-script/src/host.rs` | `with_log`; emit bootstrap Abort, engine-spawn Skip, pre_index skip, discover drop, install-verify Abort, sandbox |
| `src/lib.rs` / `src/serve_host.rs` / `src/session.rs` | Pass log + supervisor; `try_spawn` registered packs after workspace root; **do not** `let _supervisor = supervisor` |
| Tests | `FakeLog` asserts `warn`/`info`, `operation=spawn`/`initialize`/`index`/`install`/`script`, `component=engine`/`script`; `FakeClock`; no sleep |

**Do not** start sqlite bootstrap (already LOG-4). **Do not** start `std::process::Command`. **Do not** start ChildIo pipe readers (LOG-10). **Do not** start protocol/control emits (LOG-7). **Do not** insert `EngineResolver` (LOG-8). Darwin: do not fake musl greens.

### LOG-7 — Protocol + control socket + install hash

**SIGNED OFF** on `log7`. `LspFacade` / `ControlServer` / `Installer` `with_log`. Emit parse / framing / mux / method-not-found (`operation=protocol`), bind/accept/`PayloadTooLarge`/unknown method/`Status::error`/`--control-fd` ignored (`operation=control`), hash/verify refuse (`operation=install`) **without** payload bytes. Hash rows include expected/actual hex **before** `remove_or_emit`.

| File | Change |
|---|---|
| `progressive-lsp-protocol/src/lib.rs` `LspFacade` | `with_log`; emit parse / method-not-found / framing / mux **without** payload bytes |
| `progressive-lsp-protocol/src/framing.rs` / `rpc.rs` / `mux.rs` | Call sites in Facade; crates take `LogPort` only at the Facade unless a function already has context |
| `src/control_socket.rs` | `bind` success/fail, accept-loop err, `PayloadTooLarge`, empty-method drop |
| `progressive-lsp-control/src/service.rs` | Unknown method emit; `with_log` on `ControlServer` or emit from `ServeHost` plane methods |
| `src/serve_host.rs` | `Status::error` on SetConfig / InstallPacks / ReloadConfig / ReloadScripts; `install_pack_from_inbox_or_stub` must `Installer::with_log` (today it is `Installer::new(LocalFs)` with no port) |
| `src/lib.rs` | `--control-fd` ignored → one `warn` `operation=control` |
| `progressive-lsp-install/src/lib.rs` | `InstallError::Hash` and verify `Err` **before** `remove_or_emit`; expected/actual hex; no blob bytes |
| Tests | `FakeLog` on `operation=protocol`/`control`/`install`; method name in message; **no** body text in `message`/`extras` |

**Do not** start supervisor spawn (LOG-6). **Do not** log JSON-RPC / Envelope bodies. **Do not** start `LogOpenPlan` (LOG-9). **Do not** enable `-rpc.trace`.

### LOG-8 — Resolver / T3 skip + session completeness

| File | Change |
|---|---|
| `progressive-lsp-engine/src/resolve.rs` | Once per `(language, package)` `info` “pack skipped” / `EngineNotReady`; `operation=resolve` |
| `src/session.rs` | `EngineResolver` first in `ResolverChain` when supervisor attached; `didClose` debug; `indexer_for` None → once-per-id info; initialize fail already warn from LOG-6 + Facade |
| `src/serve_host.rs` | `initialize` success info; `files_since` truncated → info `operation=filesSince` |
| `progressive-lsp-watch/src/journal.rs` | **ServeHost only** (one place): truncated emit is `ServeHost::files_since`, not the journal |
| `progressive-lsp-protocol/src/lib.rs` | `shutdown` debug; `InitializeFailed` warn if not already from LOG-7 |
| `progressive-lsp-plugin/src/lib.rs` | `UnsupportedLanguage` info when `get` fails (if serve calls it) |
| Tests | `FakeLog` once-per-pair (second `definition` does not duplicate); T2/T1 still `Ready`; no sleep |

**Do not** fail the user on T3 skip. **Do not** emit on every hot-path resolve after the first skip. **Do not** start child capture. **Do not** start `Command` spawn.

### LOG-9 — Durable MemoryLog fallback

**SIGNED OFF** on `log9`. `LogOpenPlan` Command opens primary → fallback → temp WAL. Do not start child capture.

| File | Change |
|---|---|
| `progressive-lsp-log/src/path.rs` | `ServeLogPath::fallback`, `ServeLogPath::in_temp` |
| `progressive-lsp-log/` (new module or `path.rs`) | `LogOpenPlan` Command: primary → fallback → temp; tests inject dirs |
| `src/lib.rs` `wire_process_log` | Use `LogOpenPlan`; replay ring into the WAL that opened; `error` only if all fail |
| `docs/user/README.md` | Troubleshooting: missing primary → `serve-fallback-*.sqlite` then temp WAL |
| Tests | tempfile: fail primary (dir-as-file or chmod), assert fallback file exists and contains the warn + replayed ring; `BATCH_MAX=1`; no `$HOME`; no sleep |

**Do not** add JSON/syslog/journald/OTel. **Do not** start child capture. **Do not** change `[log]` default level.

### LOG-10 — Child capture wiring

**SIGNED OFF** on `log10`. Gated on `ChildIo` existing. Ready when spawn exists. Tests with `FakeChildStderr`. Do not start LOG-11 from this branch.

| File | Change |
|---|---|
| `progressive-lsp-engine/src/supervisor.rs` | If `ChildIo::has_stderr_pipe` **and** a stderr `Read` exists, `ChildStderrAdapter::attach_if_stderr_pipe`; today `ChildHandle` has no `Read` — tests pass a `FakeChildStderr` |
| `progressive-lsp-log/src/child_stderr.rs` | Already lands parser; tests `drain_fake` → `FakeLog` third-party origin |
| `progressive-lsp-log/src/log_file_tail.rs` / `lsp_message.rs` | Wire from supervisor only when a tail path / proxied `window/logMessage` exists; tests tempfile / FakeLog |
| `progressive-lsp-engine/src/pack.rs` | **Still refuse `Command`.** Do not add `std::process::Stdio` pipes here |
| Tests | `FakeChildStderr` overflow drops oldest; stdout never attached; `NullStderrAdapter::forbidden_on_prod_spawn` |

**Do not** implement real pack spawn on Darwin. **Do not** attach Adapter to engine stdout. **Do not** set `-rpc.trace` / `RA_LOG_FILE` / `TY_LOG_PROFILE`.

### LOG-11 — Operational Err hygiene gate

| File | Change |
|---|---|
| `tests/log_hygiene.rs` (or `progressive-lsp-log` test) | Every operational `Err` in `src/` + `progressive-lsp-{engine,script,protocol,control,install,index,watch,resolve}` either has a `LogPort` emit on that path **or** is listed in [logging.md](logging.md) “client-visible only” with a reason |
| [logging.md](logging.md) matrix | Last pass: no silent class remains |
| Docs | Sign-off: stack complete at `log11` |

**Do not** start new Adapters. **Do not** reopen LOG-0–LOG-10 types. **Do not** “fix” poc-ide `RunLog`.

## Call-site rules

```rust
log.info("index package done"); // #[track_caller]
let _g = LogScope::enter(LogScope::new().path(path).operation("textDocument/definition"));
```

Never `eprintln!(...)` on diagnostic paths. Never `emit(...).unwrap()`.

## poc-ide coexistence

Keep `RunLog` (`events` category/event/payload). Share the rusqlite **0.40.2** pin. Do not write poc-ide rows into the server WAL. Post-LOG-11 (not scheduled): read-only query of server logs from the IDE.

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

xtask `eprintln!`, integration harness fetch progress, Java `bakeoff.rs`. Operator CLIs. If they call a library that takes `LogPort`, pass `NullLog` or `InheritStderrAdapter` **only in those bins**. poc-ide `RunLog`. Real `PackAdapter` `Command` spawn (Linux CI). Syslog / journald / OTel / JSON files. LSP bodies. Tree-sitter silence. Blocked RPC dump flags.
