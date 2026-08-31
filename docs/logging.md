# Global logging

First-class observability for `progressive-lsp serve` / `install` and for every engine pack we spawn. Stock LSP stdout stays JSON-RPC. Logs never fail a user path. Types: [detailed-design.md](detailed-design.md). Patterns: [design-patterns.md](design-patterns.md). Plan: [logging-plan.md](logging-plan.md), [implementation-plan.md](implementation-plan.md) LOG-0–LOG-11. Exits: [milestones.md](milestones.md).

This file is the **source of truth** for the logging stack. `poc-ide` `RunLog` is a **consumer-sample** sqlite debug file ([poc-ide/architecture.md](poc-ide/architecture.md)); it does not satisfy this spec. Do not merge the two schemas in LOG-1–LOG-11.

**LOG-0–LOG-8 are signed off.** Do not reopen them. Remaining operational silent paths are LOG-9+ ([coverage matrix](#coverage-matrix-zero-blind-spots)). After LOG-11, a silent operational class in that matrix is a defect.

## Locked decisions

| Decision | Choice | Why |
|---|---|---|
| Product logs | SQLite WAL file under `$PREFIX/log/` | Directory already exists; queryable; crash-recoverable |
| Call site | `LogPort` Facade in every crate | Same injection rule as `ClockPort`; no diagnostic `eprintln!` |
| SQLite crate | `rusqlite` **=0.40.2** `bundled` in `progressive-lsp-log` | Same pin as poc-ide; amalgamation is **static C in our ELF**, not a host `.so` |
| Loadable extensions | Off (`SQLITE_OMIT_LOAD_EXTENSION`) | `dlopen` is forbidden ([host-deps.md](host-deps.md)) |
| Writer | Single Actor thread owns the connection | WAL is one-writer; re-entrant emit cannot touch the `Connection` |
| Batching | Small commit batches + immediate commit on `Error` | Crash loses at most one uncommitted batch (≤ `BATCH_MAX`) |
| Failure | `emit` returns `()` | Missing fields are NULL; full channel drops oldest and counts |
| stdout | Never log here | Content-Length JSON-RPC |
| stderr (product) | Diagnostic `eprintln!` is a defect after LOG-3 | CLI **usage/help** is the one exception (IT-1.7) |
| `poc-ide` `RunLog` | Unchanged in LOG-1–LOG-11 | Different audience, different columns; optional share is post-LOG-11 |
| xtask / integration harness / bakeoff | Stay operator CLIs | They are not `serve`; library code they call still goes through `LogPort` |

**Supersedes:** “rusqlite is poc-ide only — never musl server crates” in [poc-ide/third-party.md](poc-ide/third-party.md). The amalgamation is an **our-artifact** compiled into the static ELF. `check-static` must stay green (no `DT_NEEDED`, no interpreter).

## Problem (as of LOG-0)

- `$PREFIX/log/` is created and git-excluded. Nothing writes there.
- `ConfigLoad.warnings` is collected then **discarded** (`ServeHost` keeps `.config` only). Design already said “log at warn.”
- Product `eprintln!`: `src/main.rs` fatals. Operator tools (xtask, `plsp-it1`, Java bake-off) also print; those stay CLI.
- Engine spawn is reserved (`PackAdapter` refuses exec on this host). `ChildHandle` has **no stderr pipe**. poc-ide `StdioLsp` uses `stderr(Stdio::null())` — child logs are discarded. Supervisor comments say “stdio proxy”; there is no `Command` yet.
- Spawn allowlist already names `RUST_LOG` / `RA_LOG` / `TY_LOG` and argv `--log-level` / `--log-file` (`progressive-lsp-script`). Capture is not implemented. Gaps: clangd `--log=`, gopls `-logfile`.
- No `log` / `tracing` subscriber in **our** crates. Transitive `log` 0.4.34 / `tracing` 0.1.44 in `Cargo.lock` are unused by default server features. Optional `t2-stack-graphs` pulls `tree-sitter-graph` which **does** use `log`. Tree-sitter T1 stays silent.

## Remaining problem (after LOG-4)

LOG-4 wired the WAL, Facades, and a subset of silent failures. An operator querying `$PREFIX/log/serve-*.sqlite` still cannot answer “why isn’t this working?” for the paths below. Verified against code on this tree (not only the ingest list).

Covered today at default **info**: config unknown keys, watch overflow, index-cache IO, watch/ghost-disk `read_to_string` fail, install `remove_file` fail, CLI usage/help, process fatals after Facade exists. `didOpen` / `didChange` / `definition` are **debug**.

| Gap | Where | What sqlite sees today |
|---|---|---|
| `EngineSupervisor` has no `LogPort` | `progressive-lsp-engine/src/supervisor.rs` | `try_spawn` `NotDiscovered` / stub refuse / `Aborted` / `Hash` / `Backoff`, `note_crash`, `last_error` stay in the mutex `BTreeMap` |
| Serve drops the supervisor | `src/lib.rs` `serve_with_io_and_log` | Constructs `PackAdapter`s then `let _supervisor = supervisor` — never `try_spawn`, never attached to `ServeHost` / `WorkspaceSession` |
| `discover_pack_opt` swallows `EngineError::Hash` | `progressive-lsp-engine/src/discovery.rs` | `.ok()` turns hash mismatch into `NotDiscovered` / `Ok(false)` |
| `PackAdapter::spawn` refuses `Command` | `progressive-lsp-engine/src/pack.rs` | Returns `EngineError::Spawn` (“stub pack” / “reserved for Linux CI”). No emit. Do **not** implement real spawn to get logs — **log the refuse** |
| `ChildHandle` has no live OS pipes | `adapter.rs` `ChildIo` is two bools | `ChildStderrAdapter` / `LogFileTailAdapter` / `LspLogMessageAdapter` exist; `stderr_attached` is a set flag; nothing ingests child bytes. `NullStderrAdapter` stays forbidden on prod spawn |
| `ScriptHost` has no `LogPort` | `progressive-lsp-script/src/host.rs` | `on_bootstrap` Abort → `InitializeFailed` to the client only. `on_engine_spawn` Skip is silent. `on_pre_index` skip / `on_workspace_discover` drop roots stay in `skipped_packages` |
| Control socket | `src/control_socket.rs` | `bind_control_socket` / `spawn_control_accept` / `PayloadTooLarge` / empty-method drop: no `LogPort`. `--control-fd` is parsed then `let _ = opts.control_fd` |
| Protocol crate | `progressive-lsp-protocol` `rpc` / `framing` / `mux` / `LspFacade` | Parse, missing `Content-Length`, method-not-found, mux errors: JSON-RPC / process `Err` only. **Never** log message bodies or buffer text |
| T3 skip | `EngineResolver` returns `NotReady`; session chain is T2+T1 only | Product must not fail the user; ops must see “pack skipped” **once** per `(language, package)` |
| Install hash / verify refuse | `Installer::apply_with_verify` | Returns `InstallError::Hash` / verify `Err`; sqlite only logs follow-on `remove_file` |
| Sqlite open fail | `src/lib.rs` `wire_process_log` | Keeps `MemoryLog`; those rows die with the process |
| Session completeness | `src/session.rs` / `LspFacade` | `didClose` silent. `initialize` fail is JSON-RPC `-32002` only. `shutdown` is success with no row. `FilesSinceAnswer.truncated` is proto-only. `indexer_for` `_ => None` is silent. `PluginRegistry::get` `UnsupportedLanguage` is unused on the serve path |

**Allowlist still blocks** `-rpc.trace`, `RA_LOG_FILE`, `TY_LOG_PROFILE`. Keep blocked. Capture via adapters, not by enabling dumps.

## Architecture

```text
  Language crates / index / watch / engine / protocol / install / scripts
           │  LogPort::emit  (never Result)
           ▼
     LogFacade  ── ReentrancyGuard (thread-local)
           │
           ├── LogScope  (Context Object: path / line / operation)
           │
           ▼
     LogSink Port
           │
     ┌─────┴──────────────────────────────┐
     │                                    │
  FakeLog / MemoryLog              SqliteLogRepository
  (tests)                          (prod Adapter)
                                          │
                                   WriterActor (thread)
                                          │
                                   CrashSafeBatch
                                          │
                                   WAL Connection
                                          │
                                   $PREFIX/log/serve-<unix_ms>-<pid>.sqlite

  Capture Adapters (into LogPort, never onto stdout):
    StderrEmitAdapter        our former eprintln sites
    LogCrateBridge           log::Log global logger
    TracingBridge            tracing Layer (poc-ide / rare in-process)
    ChildStderrAdapter       engine pack stderr lines
    LogFileTailAdapter       zls / biome daemon files (optional --log-file)
    LspLogMessageAdapter     window/logMessage / $/logTrace from a proxied engine (secondary)
    ConfigWarnAdapter        ConfigLoad.warnings
```

**Processes**

```text
editor  --stdio JSON-RPC-->  progressive-lsp serve
                                ├── LogPort → sqlite WAL
                                └── EngineSupervisor
                                      └── pack child
                                            stdout: LSP (proxied; logMessage intercepted)
                                            stderr: ChildStderrAdapter → LogPort
                                            files:  LogFileTailAdapter when the engine logs to disk
```

One sqlite **file per serve/install process**. Multiple `serve` processes on one prefix do not share a writer. WAL is still required so a crash mid-commit does not corrupt the file, and so a later reader can query while `serve` is up.

### On-disk

```text
$PREFIX/log/
  serve-<unix_ms>-<pid>.sqlite
  serve-<unix_ms>-<pid>.sqlite-wal
  serve-<unix_ms>-<pid>.sqlite-shm
  serve-fallback-<unix_ms>-<pid>.sqlite   # LOG-9: primary open failed
  <pack>/                    # optional tails (zls, biome)
```

`PrefixLayout::log_dir()` is the directory. Filename is a Value object `ServeLogPath` (same idea as poc-ide `RunLogPath`). Tests inject `:memory:` (shared-cache URI) or a tempfile. Override: env `PROGRESSIVE_LSP_LOG` (absolute file path) or `[log].path` in config. Empty / unset → default name under `log_dir()`.

If the primary WAL cannot open, [durable fallback](#durable-fallback-sqlite-open-fail) tries `ServeLogPath::fallback` in the same directory, then a tempfile-injected / `std::env::temp_dir()` WAL named `progressive-lsp-serve-<unix_ms>-<pid>.sqlite`. Still rusqlite WAL. Never syslog, journald, OpenTelemetry, or JSON files.

Do **not** write logs under the worktree overlay. Belt `.gitignore` already lists `log/`.

## Implementation design

Every type below is in [design-patterns.md](design-patterns.md). Ad-hoc logger helpers are a defect.

### Crate split

| Crate | Owns | Must not own |
|---|---|---|
| `progressive-lsp-core` | `LogPort`, `LogRecord`, `LogLevel`, `LogOrigin`, `LogScope`, `LogComponent`, `NeverFailLog` Decorator, `FakeLog`, `MemoryLog`, `NullLog` | rusqlite, threads, `log` crate |
| `progressive-lsp-log` | `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath`, `LogOpenPlan`, WAL pragmas, capture Adapters | LSP parsing of engine **requests** (protocol crate still owns JSON-RPC) |
| Composition root | Wires `SqliteLogRepository` via `LogOpenPlan` (or `MemoryLog` until prefix exists / after all WAL opens fail), installs bridges, flushes on shutdown | Direct `Connection` use |

Libs take `Arc<dyn LogPort>`. The bin is the only place that constructs the sqlite Adapter. Tests never open `$HOME`. Process-wide `OnceLock<LogPort>` is forbidden.

### `LogRecord` (DTO)

Log what is known. Omit what is not. **Never** fail construction.

| Column / field | Type | Required | Source |
|---|---|---|---|
| `id` | INTEGER PK | sqlite | AUTOINCREMENT |
| `ts_unix_ms` | INTEGER | yes | `ClockPort::unix_ms` |
| `level` | TEXT | yes | `LogLevel` (`error` `warn` `info` `debug` `trace`) |
| `component` | TEXT | no | crate name or pack name (`LogComponent`) |
| `source_file` | TEXT | no | `#[track_caller]` / `log::Record::file` / engine parse |
| `source_line` | INTEGER | no | same |
| `source_repo` | TEXT | yes | `progressive-lsp` \| `third-party` (`LogOrigin`) |
| `source_crate` | TEXT | no | Cargo crate or pack id |
| `content_path` | TEXT | no | `LogScope` workspace path under question |
| `content_file` | TEXT | no | basename of `content_path` if path set |
| `content_line` | INTEGER | no | `LogScope` or parsed engine location |
| `operation` | TEXT | no | LSP method, `index`, `spawn`, `watch`, `install`, `config`, `serve`, `protocol`, `control`, `resolve`, `initialize`, `filesSince`, `log`, `script` |
| `message` | TEXT | yes | body; truncation at 64 KiB, lossy UTF-8 |
| `extras` | TEXT | no | sanitized JSON object; never file bodies / secrets |

Indexes: `(ts_unix_ms)`, `(level)`, `(component)`, `(content_path)`, `(source_repo)`.

`sanitize_extras` drops keys `text`, `content`, `body`, `clipboard`, `password`, `secret`, `token` (same family as poc-ide `sanitize_payload`).

### Ports and Adapters

**`LogPort`** — Dependency injection / Port. `fn emit(&self, record: LogRecord);` plus convenience `error` / `warn` / `info` / `debug` / `trace` with `#[track_caller]`. No `Result`.

**`NeverFailLog`** — Decorator. Wraps a `LogSink` that may return `Result`. Swallows errors. Used around sqlite so a full disk cannot panic `serve`.

**`LogSink`** — Port for durable append. Prod: `SqliteLogRepository`. Tests: `FakeLog` (records into a mutex `Vec` for assertions).

**`LogScope`** — Context Object. Task-local / thread-local: `content_path`, `content_line`, `operation`, `component`. Index, resolve, and LSP handlers **enter** a scope for the file they are working. `emit` copies scope fields into the record when the caller left them unset. Drop of the guard restores the previous scope (stack).

**`ReentrancyGuard`** — Proxy / Guard. Thread-local `IN_EMIT`. If `emit` is already on the stack, enqueue on the Actor channel without taking Facade locks that could deadlock. The writer thread **never** calls `LogPort`.

**`WriterActor`** — Actor. Owns the `rusqlite::Connection`. Receives `LogRecord` / `Flush` / `Shutdown` on an mpsc channel. `check_same_thread` stays true: the connection does not move.

**`CrashSafeBatch`** — Unit of Work. Accumulates records. Commit when any of:

1. `len >= BATCH_MAX` (default **32**)
2. `ClockPort` elapsed since last commit ≥ `BATCH_MS` (default **50**; production only — tests set `BATCH_MAX = 1` or call `Flush`)
3. Incoming record `level == Error` (flush **including** that record immediately)
4. `Flush` / `Shutdown` / `Drop`

Each commit is one `BEGIN IMMEDIATE` … `COMMIT` transaction. Crash during COMMIT: WAL recovery drops the incomplete transaction; previous commits remain. Uncommitted RAM batch is lost (bounded). **Do not** `thread::sleep` in tests; FakeClock + `Flush` is the gate.

Required pragmas:

- `journal_mode=WAL`
- `synchronous=NORMAL` (not `OFF`)
- `busy_timeout=5000`
- `wal_autocheckpoint=1000`

On `COMMIT` failure: keep the batch in a retry `Vec` (cap 1024). Overflow: drop oldest, increment `dropped_count`, insert one `warn` meta row on the next successful commit (`operation = "log"`).

`Drop` of `SqliteLogRepository`: send `Shutdown`, join the Actor without a test sleep (`BATCH_MAX=1` so join is immediate). Composition root `shutdown` / `exit`: `Flush` before process end.

**Not in v1:** a JSON/text sidecar ring file. LOG-9 fallback is still **sqlite WAL** (`ServeLogPath::fallback` / `in_temp`). Bounded WAL batching is the crash story.

**`ServeLogPath`** — Value object. `{log_dir}/serve-{unix_ms}-{pid}.sqlite`. LOG-9 adds `fallback` (`serve-fallback-{unix_ms}-{pid}.sqlite` in the same dir) and `in_temp` (`{temp_dir}/progressive-lsp-serve-{unix_ms}-{pid}.sqlite`). Tests inject both directories; never `$HOME`.

**`LogOpenPlan`** — Command. Ordered WAL open: primary `ServeLogPath` → same-dir fallback → temp WAL. First success wins; replay `MemoryLog` into it; emit `warn` `operation=log` naming the path that opened and why the previous failed. All three fail → keep `MemoryLog` (honest residual: those rows die with the process). Still `emit` returns `()`.

**`LogLevel`** — Value object. Unknown parse → `info` (never fail). Filter: records below configured min level are dropped **in the Facade**.

**`LogOrigin`** — Value object. `FirstParty` (`progressive-lsp`) vs `ThirdParty`.

**`LogComponent`** — Value object. Stable strings: `core`, `protocol`, `control`, `engine`, `index`, `watch`, `install`, `script`, `lang-<id>`, pack name, `xtask` (only if a lib path logs).

### Capture Adapters (harmonize 3rd-party + stderr)

Investigation: engine packs speak LSP on **stdout**. Logs are stderr and/or a side file. Mixing those onto stdout is fatal for Content-Length. `window/logMessage` is secondary (not a substitute for panics).

| Adapter | Pattern | Input | Origin | Invariant |
|---|---|---|---|---|
| `StderrEmitAdapter` | Adapter | Former `eprintln!` sites in the product bin/libs | first-party | After LOG-3, grep of diagnostic `eprintln!` in `src/` and `progressive-lsp-*` is empty except tests and the CLI usage exception |
| `LogCrateBridge` | Adapter | `log::Log::log` | third-party unless target starts with `progressive_lsp` | Installed once in the composition root; captures optional `t2-stack-graphs` `log` users |
| `TracingBridge` | Adapter | `tracing` `Event`s | same rule | Server default features have no tracing emitters; poc-ide (eframe/rfd) does — that is the IDE process |
| `ChildStderrAdapter` | Observer + Adapter | Line-delimited stderr of a pack | third-party | stdout of the child is **never** this Adapter; bounded drain so stderr cannot stall LSP |
| `LogFileTailAdapter` | Adapter | Engine log **file** | third-party | Prefer `$PREFIX/log/<pack>/`; do not parse LSP from the file |
| `LspLogMessageAdapter` | Adapter | `window/logMessage` / `window/showMessage` / `$/logTrace` | third-party | Secondary; never a substitute for crash/panic on stderr |
| `NullStderrAdapter` | Adapter | `stderr(Stdio::null())` | — | **Forbidden** on production pack spawn (today’s poc-ide `StdioLsp` gap) |
| `InheritStderrAdapter` | Adapter | `stderr(Stdio::inherit())` | — | Operator/CI harness only |
| `ConfigWarnAdapter` | Adapter | `ConfigLoad.warnings` | first-party | Unknown keys emit `warn` + `operation=config` |
| `CliUsageAdapter` | Adapter | `--help` / usage | first-party | **Also** writes stderr (IT-1.7) |

**Engine children (when `PackAdapter` actually spawns)**

Pipe stderr; do **not** inherit it. Do **not** set `--log-file` into `$PREFIX/log/` unless using `LogFileTailAdapter` (zls/biome). Prefer stderr capture + existing env allowlist.

| Pack | stdout | How it logs | Our capture | Flags / env |
|---|---|---|---|---|
| rust-analyzer | LSP | stderr (`RA_LOG` / `RUST_LOG`); optional `--log-file`; `window/logMessage` secondary | `ChildStderrAdapter` + `LspLogMessageAdapter` | `RUST_LOG`, `RA_LOG` allowlisted; prod spawn does not set `--log-file` |
| ty | LSP | stderr; `TY_LOG` / `RUST_LOG` | `ChildStderrAdapter` | `TY_LOG`, `RUST_LOG` |
| clangd | LSP | stderr via `--log=error\|info\|verbose` | `ChildStderrAdapter` | `--log-level` is allowlisted but **clangd wants `--log=`** — LOG-3 extends argv allowlist |
| tsgo | LSP | stderr (Go LS pattern) | `ChildStderrAdapter` | `--stdio` allowlisted; no `-rpc.trace` |
| gopls | LSP | stderr; `-logfile` / `-rpc.trace` are **single-dash** | `ChildStderrAdapter`; optional `LogFileTailAdapter` | Do **not** enable `-rpc.trace`. LOG-3 may allowlist `-logfile` |
| zls | LSP | Default **file** (often `~/.cache/zls/zls.log`) | `LogFileTailAdapter` forcing `--log-file=$PREFIX/log/zls/…` | `--log-file`, `--log-level` already allowlisted |
| biome | LSP (`lsp-proxy`) | Daemon log dir under OS cache | `LogFileTailAdapter` on a prefix path if settable; proxy stderr secondary | `--log-file` may not be the daemon; verify pin |
| superhtml | LSP | stderr (Zig LS convention) | `ChildStderrAdapter`; file fallback if the pin documents one | `--log-level` / `--log-file` if CLI matches |
| phpantom | LSP | stderr; `RUST_LOG` | `ChildStderrAdapter` | `RUST_LOG` |

**Allowlist gaps (LOG-3):** clangd `--log=`; gopls `-logfile`. Do **not** add `-rpc.trace`, `RA_LOG_FILE`, or `TY_LOG_PROFILE` (RPC dumps / unconstrained files). `TMPDIR` is already allowlisted so engines do not scratch the workspace.

**In-process `log` crate:** default server features have **no** first-party emitters. Tree-sitter stays silent. Optional `--features t2-stack-graphs` uses `log` — `LogCrateBridge` captures those. poc-ide egui/`notify`/`rfd` are `log`/`tracing` speakers in the **IDE process**, not `serve`.

**Line parse (best-effort, never fail):** if a stderr line looks like `LEVEL module: message`, fill `level` / `source_crate`. Otherwise `level=info`, `message=line`. Invalid UTF-8 → lossy. No regex panic.

**poc-ide `StdioLsp`:** LOG-3 left `stderr(Stdio::null())`. Do not inherit engine stderr into the IDE process. LOG-4 may pipe into `RunLog`; server-side capture is the product gate.

### Config

```toml
[log]
level = "info"          # error warn info debug trace; omit = info
path = ""               # optional absolute sqlite path; omit = ServeLogPath default
```

Unknown keys still warn (and now **emit**). `[log]` merges on the same chain as the rest of `Config`. Invalid `level` → warn + default `info` (never fail boot).

`PROGRESSIVE_LSP_LOG` env overrides `path` for the process. `--prefix` still wins for the directory when `path` is unset.

### Bootstrap order (composition root)

1. `MemoryLog` (ring, cap 4096) so prefix/config failures are not silent.
2. Resolve prefix; `ensure_dirs` (creates `log/`).
3. `LogOpenPlan`: open `SqliteLogRepository` on primary `ServeLogPath`; on failure retry fallback then temp WAL (LOG-9). Only if all three fail: keep `MemoryLog` and continue serving.
4. Replay the ring into whichever WAL opened (best-effort).
5. Install `LogCrateBridge` / `TracingBridge`.
6. Load config; `ConfigWarnAdapter` emits warnings.
7. Serve / install.
8. On shutdown: `Flush` + join.

### CLI exception

`USAGE` / `--help` / unknown-command text stays on **stderr** (IT-1.7). Those strings are **also** `LogPort::warn` with `operation=cli`. Diagnostic fatals in `main` go to `LogPort` once the Facade exists.

### Hygiene

- Tests: `FakeLog` / `MemoryLog`; sqlite tests use tempfile or shared-cache memory URI. No `$HOME`. No `thread::sleep`. `BATCH_MAX=1` or explicit `Flush`. LOG-6+ FakeLog asserts name `level` / `operation` / `component` / message substring.
- `progressive-lsp-log` is on the 95% llvm-cov denominator and the 80% mutants list.
- `check-static` on musl ELFs after rusqlite is linked. Fail closed if sqlite pulls `libdl` as `DT_NEEDED`.
- Hot path: channel send is non-blocking (`try_send`); full channel drops oldest.

### What we do not do (LOG-1–LOG-11)

- JSON files, syslog, journald, OpenTelemetry export.
- Logging LSP **message bodies** or buffer text.
- `window/logMessage` from **our** server to the editor (optional later Adapter).
- Sharing one sqlite file across two `serve` processes.
- Replacing poc-ide `RunLog`.
- Capturing engine **stdout** as text (it is LSP).
- Implementing real `PackAdapter` `Command` spawn to get logs (LOG the refuse; ChildIo wiring is LOG-10, ready when spawn exists).
- Enabling `-rpc.trace`, `RA_LOG_FILE`, or `TY_LOG_PROFILE`.

## Durable fallback (sqlite open fail)

LOG-4: `wire_process_log` keeps `MemoryLog` when `SqliteLogRepository::open` fails. Those rows are gone when the process exits — an operator with an empty `$PREFIX/log/` cannot tell *why* serve “had no log file.”

LOG-9 closes that without syslog / journald / OTel / JSON:

1. **Primary** — `ServeLogPath` as today (`$PREFIX/log/serve-<unix_ms>-<pid>.sqlite`, or `PROGRESSIVE_LSP_LOG` / `[log].path`).
2. **Same-dir fallback** — `ServeLogPath::fallback`: `$PREFIX/log/serve-fallback-<unix_ms>-<pid>.sqlite`. Same `SqliteLogRepository` / `WriterActor` / WAL pragmas.
3. **Temp WAL** — `ServeLogPath::in_temp`: tests inject a tempfile directory; production uses `std::env::temp_dir()/progressive-lsp-serve-<unix_ms>-<pid>.sqlite`. Still rusqlite bundled. Not `$HOME`. Not a JSON sidecar.
4. **Last residual** — all three fail (prefix `log/` and temp dir both unwritable): keep `MemoryLog`. That host cannot retain files; we do not invent syslog. Composition root still serves. `emit` still returns `()`.

On each failed attempt, the next successful WAL’s first rows include `warn` `operation=log` `component=core` with the failed path and the `io`/`rusqlite` reason (no secrets). Replay the bootstrap ring into the WAL that actually opened. `Flush` + join on shutdown still applies.

User troubleshooting: if `$PREFIX/log/` has no `serve-*.sqlite`, look for `serve-fallback-*.sqlite` then `progressive-lsp-serve-*.sqlite` under the process temp dir.

## Coverage matrix (zero-blind-spots)

This table **is** the definition. After the last LOG-N, a silent class here is a defect. Default `[log]` level stays **info**. Lifecycle/ops failures: **warn**. Skip/fallback that did not fail the user: **info**. Hot path (`didChange`): **debug**.

`origin` is `LogOrigin` (`progressive-lsp` vs `third-party`). Test double is `FakeLog` unless noted. Assert on `level` / `operation` / `component` / message substring — no `thread::sleep`.

### Landed LOG-3 / LOG-4 (do not reopen)

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| Config unknown key / invalid `[log].level` | `ConfigWarnAdapter` | warn | `config` | first-party | LOG-4 | `FakeLog` |
| Watch overflow | `WatchCoalescer` | warn | `watch` | first-party | LOG-4 | `FakeLog` |
| Index-cache IO miss | `IndexCache::emit_io_warn` | warn | `index` | first-party | LOG-4 | `FakeLog` |
| Ghost-disk / watch `read_to_string` fail | `WorkspaceSession::emit_watch_io` | warn | `watch` | first-party | LOG-4 | `FakeLog` |
| Install `remove_file` fail | `Installer::remove_or_emit` | warn | `install` | first-party | LOG-4 | `FakeLog` |
| CLI usage / help | `CliUsageAdapter` | warn | `cli` | first-party | LOG-3 | `FakeLog` |
| Process fatal after Facade exists | `StderrEmitAdapter` | error | `serve` / `install` | first-party | LOG-3 | `FakeLog` |
| `didOpen` / `didChange` / `definition` | `WorkspaceSession` | debug | LSP method | first-party | LOG-4 | `FakeLog` |
| Sqlite primary open fail (MemoryLog only) | `wire_process_log` | warn | `log` | first-party | LOG-4 | `MemoryLog` — **not durable; LOG-9** |

### LOG-6 — Supervisor + ScriptHost lifecycle

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| `try_spawn` `NotDiscovered` | `EngineSupervisor` | warn | `spawn` | first-party | LOG-6 | `FakeLog` |
| `try_spawn` stub / “reserved for Linux CI” refuse | `PackAdapter` via supervisor | warn | `spawn` | first-party | LOG-6 | `FakeLog` |
| `try_spawn` `EngineError::Hash` (do not swallow via `discover_pack_opt`) | `discover_pack` | warn | `spawn` | first-party | LOG-6 | `FakeLog` |
| `try_spawn` `Aborted` (`on_engine_spawn` / `ScriptHookBridge`) | `EngineSupervisor` + `ScriptHost` | warn | `spawn` | first-party | LOG-6 | `FakeLog` |
| `try_spawn` `Backoff` (not yet due) | `EngineSupervisor` | info | `spawn` | first-party | LOG-6 | `FakeLog` |
| `try_spawn` `Ok(true)` | `EngineSupervisor` | info | `spawn` | first-party | LOG-6 | `FakeLog` |
| `note_crash` / `poll_health` dead child | `EngineSupervisor` | warn | `spawn` | first-party | LOG-6 | `FakeLog` |
| `on_bootstrap` Abort / sandbox → `InitializeFailed` | `ScriptHost` | warn | `initialize` | first-party | LOG-6 | `FakeLog` |
| `on_engine_spawn` Skip (no spawn) | `ScriptHost` | info | `spawn` | first-party | LOG-6 | `FakeLog` |
| `on_pre_index` skip package | `ScriptHost` + `WorkspaceSession` | info | `index` | first-party | LOG-6 | `FakeLog` |
| `on_workspace_discover` drop root | `ScriptHost` + `WorkspaceSession` | info | `index` | first-party | LOG-6 | `FakeLog` |
| `on_install_verify` Abort | `ScriptHost` | warn | `install` | first-party | LOG-6 | `FakeLog` |
| Hook `ScriptSandbox` | `ScriptHost` | warn | `script` | first-party | LOG-6 | `FakeLog` |
| `on_watch` drop path | `ScriptHost` | debug | `watch` | first-party | LOG-6 | `FakeLog` |

Serve composition root **must not drop** `EngineSupervisor`. Pass `Arc<dyn LogPort>` like `ClockPort`, `with_supervisor` on `WorkspaceSession` / `ServeHost`, and `try_spawn` registered packs after initialize has a workspace root so the stub refuse is a sqlite row. **Do not** start `std::process::Command`.

### LOG-7 — Protocol, control socket, install hash

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| JSON-RPC parse / invalid request | `rpc::parse_request` via `LspFacade` | warn | `protocol` | first-party | LOG-7 | `FakeLog` |
| Missing / invalid `Content-Length` | `FramingError` | warn | `protocol` | first-party | LOG-7 | `FakeLog` |
| Method-not-found | `LspFacade::handle_request` | info | `protocol` | first-party | LOG-7 | `FakeLog` |
| Mux unknown channel / payload too large / incomplete | `MuxError` | warn | `protocol` | first-party | LOG-7 | `FakeLog` |
| Control bind success | `bind_control_socket` | info | `control` | first-party | LOG-7 | `FakeLog` |
| Control bind fail | `bind_control_socket` | warn | `control` | first-party | LOG-7 | `FakeLog` |
| Accept loop IO fail / connection handler err | `spawn_control_accept` | warn | `control` | first-party | LOG-7 | `FakeLog` |
| Envelope unknown method | `ControlServer::dispatch_envelope` | info | `control` | first-party | LOG-7 | `FakeLog` |
| `CodecError::PayloadTooLarge` / empty-method drop | `control_socket::drain_frames` | warn | `control` | first-party | LOG-7 | `FakeLog` |
| `ControlPlane` `Status::error` (`SetConfig` / `InstallPacks` / `ReloadConfig` / `ReloadScripts`) | `ServeHost` | warn | `control` | first-party | LOG-7 | `FakeLog` |
| `--control-fd` ignored (`pending`) | `src/lib.rs` | warn | `control` | first-party | LOG-7 | `FakeLog` |
| `InstallError::Hash` (tmp or dest) | `Installer::apply_with_verify` | warn | `install` | first-party | LOG-7 | `FakeLog` |
| Install verify refuse (`InstallError::Refused`) | `Installer` | warn | `install` | first-party | LOG-7 | `FakeLog` |

**Never** put JSON-RPC / Envelope **bodies**, header buffer text, or install blob bytes in `message` / `extras`. Method name, error code, path, expected/actual **hex** for hash are OK.

### LOG-8 — Resolver / session completeness

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| T3 `EngineNotReady` / `ResolveOutcome::NotReady` fallback (once per `(language, package)`) | `EngineResolver` | info | `resolve` | first-party | LOG-8 | `FakeLog` |
| `initialize` success | `ServeHost::on_initialize` | info | `initialize` | first-party | LOG-8 | `FakeLog` |
| `InitializeFailed` (also JSON-RPC `-32002`) | `LspFacade` + `ScriptHost` + `ServeHost` | warn | `initialize` | first-party | LOG-8 | `FakeLog` |
| `didClose` | `WorkspaceSession` | debug | `textDocument/didClose` | first-party | LOG-8 | `FakeLog` |
| `shutdown` | `LspFacade` | debug | `shutdown` | first-party | LOG-8 | `FakeLog` |
| `FilesSinceAnswer.truncated` | `ServeHost::files_since` (not also the journal) | info | `filesSince` | first-party | LOG-8 | `FakeLog` |
| No indexer / unknown `language_id` (once per id) | `WorkspaceSession::indexer_for` | info | `resolve` | first-party | LOG-8 | `FakeLog` |
| `UnsupportedLanguage` from `PluginRegistry::get` | `PluginRegistry` | info | `resolve` | first-party | LOG-8 | `FakeLog` |

Insert `EngineResolver` as the first `ResolverChain` step when a supervisor is attached so `NotReady` is a real skip (T2/T1 still answer). Do **not** fail the user. Do **not** emit on every `didChange` / `definition` after the first skip for that pair.

`shutdown` success is client-visible; debug is completeness, not a warn.

### LOG-9 — Durable WAL fallback

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| Primary WAL open fail; fallback WAL opened | `LogOpenPlan` | warn | `log` | first-party | LOG-9 | tempfile + `FakeLog` |
| Same-dir fallback fail; temp WAL opened | `LogOpenPlan` | warn | `log` | first-party | LOG-9 | injected temp dir |
| All WAL opens fail; `MemoryLog` only | `wire_process_log` | error | `log` | first-party | LOG-9 | `MemoryLog` |

### LOG-10 — Child capture (ready when spawn exists)

| Event | Crate / type | Level | `operation` | Origin | Lands | Test double |
|---|---|---|---|---|---|---|
| Pack stderr line | `ChildStderrAdapter` | parsed / info | `spawn` | third-party | LOG-10 | `FakeChildStderr` |
| Engine log file line | `LogFileTailAdapter` | info | `spawn` | third-party | LOG-10 | tempfile |
| `window/logMessage` / `$/logTrace` | `LspLogMessageAdapter` | mapped | `spawn` | third-party | LOG-10 | `FakeLog` |
| `NullStderrAdapter` on prod pack spawn | forbidden | — | — | — | LOG-3 (keep) | unit assert |

`ChildHandle` still has no live OS `Read`. LOG-10 tests `attach_if_stderr_pipe` + `FakeChildStderr::drain`. When `PackAdapter` later owns a real pipe, attach without a new Adapter type. **Do not** implement `Command` spawn on this WP. **Never** attach an Adapter to child stdout.

### LOG-11 — Operational Err hygiene

Every product `Err` on `serve` / `install` either emits or is listed as **client-visible only** with a reason. Gate: `tests/log_hygiene.rs` (or equivalent in a listed crate) plus the table below. Not a new runtime type.

### Explicit non-goals (not blind spots)

| Class | Why it is not a blind spot |
|---|---|
| Hot-path debug volume (`didChange`) | Default filter is `info`; debug is opt-in |
| LSP message bodies / buffer text | Locked; `sanitize_extras` |
| Engine **stdout** as text | It is JSON-RPC; `ChildIo::stdout_is_never_log_adapter` |
| poc-ide `RunLog` | Separate process, separate schema |
| xtask / `plsp-it1` / Java `bakeoff.rs` | Operator CLIs (IT-1.7 family) |
| Blocked `-rpc.trace` / `RA_LOG_FILE` / `TY_LOG_PROFILE` | Capture via adapters; do not enable dumps |
| Tree-sitter silence | No first-party `log` emitters; optional `t2-stack-graphs` goes through `LogCrateBridge` |
| Empty definition locations | Valid miss, not an operational failure |
| All three WAL opens fail | Unwritable prefix **and** temp dir; syslog is forbidden |
| `PackAdapter` real `Command` on Darwin | Linux CI / Docker; LOG the refuse |
| Sharing one WAL across two `serve` processes | Locked |

### Client-visible only (after emit also exists)

These stay on the protocol / CLI **and** get a sqlite row from LOG-7/LOG-8. They are not “sqlite instead of the client.”

| Path | Client sees | Sqlite (after LOG-N) |
|---|---|---|
| JSON-RPC error (`-32700` / `-32600` / `-32601` / `-32002`) | editor | LOG-7 / LOG-8 `warn`/`info` `operation=protocol` or `initialize` |
| Envelope `Status.code != 0` | progressive client | LOG-7 `warn` `operation=control` |
| `InstallError` from CLI | stderr via `StderrEmitAdapter` + process exit | LOG-7 hash/refuse `warn`; LOG-4 `remove_file` |
| `FramingError` tearing down `serve` | process exit | LOG-7 then `Flush` |
| `--help` / usage | stderr (IT-1.7) | LOG-3 `operation=cli` |
