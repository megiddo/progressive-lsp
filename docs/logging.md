# Global logging

First-class observability for `progressive-lsp serve` / `install` and for every engine pack we spawn. Stock LSP stdout stays JSON-RPC. Logs never fail a user path. Types: [detailed-design.md](detailed-design.md). Patterns: [design-patterns.md](design-patterns.md). Plan: [logging-plan.md](logging-plan.md), [implementation-plan.md](implementation-plan.md) LOG-0–LOG-4. Exits: [milestones.md](milestones.md).

This file is the **source of truth** for the logging stack. `poc-ide` `RunLog` is a **consumer-sample** sqlite debug file ([poc-ide/architecture.md](poc-ide/architecture.md)); it does not satisfy this spec. Do not merge the two schemas in LOG-1–LOG-4.

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
| `poc-ide` `RunLog` | Unchanged in LOG-1–LOG-4 | Different audience, different columns; optional share is post-LOG-4 |
| xtask / integration harness / bakeoff | Stay operator CLIs | They are not `serve`; library code they call still goes through `LogPort` |

**Supersedes:** “rusqlite is poc-ide only — never musl server crates” in [poc-ide/third-party.md](poc-ide/third-party.md). The amalgamation is an **our-artifact** compiled into the static ELF. `check-static` must stay green (no `DT_NEEDED`, no interpreter).

## Problem (as of LOG-0)

- `$PREFIX/log/` is created and git-excluded. Nothing writes there.
- `ConfigLoad.warnings` is collected then **discarded** (`ServeHost` keeps `.config` only). Design already said “log at warn.”
- Product `eprintln!`: `src/main.rs` fatals. Operator tools (xtask, `plsp-it1`, Java bake-off) also print; those stay CLI.
- Engine spawn is reserved (`PackAdapter` refuses exec on this host). `ChildHandle` has **no stderr pipe**. poc-ide `StdioLsp` uses `stderr(Stdio::null())` — child logs are discarded. Supervisor comments say “stdio proxy”; there is no `Command` yet.
- Spawn allowlist already names `RUST_LOG` / `RA_LOG` / `TY_LOG` and argv `--log-level` / `--log-file` (`progressive-lsp-script`). Capture is not implemented. Gaps: clangd `--log=`, gopls `-logfile`.
- No `log` / `tracing` subscriber in **our** crates. Transitive `log` 0.4.34 / `tracing` 0.1.44 in `Cargo.lock` are unused by default server features. Optional `t2-stack-graphs` pulls `tree-sitter-graph` which **does** use `log`. Tree-sitter T1 stays silent.

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
  <pack>/                    # optional tails (zls, biome)
```

`PrefixLayout::log_dir()` is the directory. Filename is a Value object `ServeLogPath` (same idea as poc-ide `RunLogPath`). Tests inject `:memory:` (shared-cache URI) or a tempfile. Override: env `PROGRESSIVE_LSP_LOG` (absolute file path) or `[log].path` in config. Empty / unset → default name under `log_dir()`.

Do **not** write logs under the worktree overlay. Belt `.gitignore` already lists `log/`.

## Implementation design

Every type below is in [design-patterns.md](design-patterns.md). Ad-hoc logger helpers are a defect.

### Crate split

| Crate | Owns | Must not own |
|---|---|---|
| `progressive-lsp-core` | `LogPort`, `LogRecord`, `LogLevel`, `LogOrigin`, `LogScope`, `LogComponent`, `NeverFailLog` Decorator, `FakeLog`, `MemoryLog`, `NullLog` | rusqlite, threads, `log` crate |
| `progressive-lsp-log` | `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath`, WAL pragmas, capture Adapters | LSP parsing of engine **requests** (protocol crate still owns JSON-RPC) |
| Composition root | Wires `SqliteLogRepository` (or `MemoryLog` until prefix exists), installs bridges, flushes on shutdown | Direct `Connection` use |

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
| `operation` | TEXT | no | LSP method, `index`, `spawn`, `watch`, `install`, `config`, `serve` |
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

**Not in v1:** a second sidecar ring file. Bounded WAL batching is the crash story.

**`ServeLogPath`** — Value object. `{log_dir}/serve-{unix_ms}-{pid}.sqlite`.

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

**poc-ide `StdioLsp`:** today `stderr(Stdio::null())`. LOG-4 may pipe into `RunLog`; server-side capture is the product gate.

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
3. Open `SqliteLogRepository`; on failure keep `MemoryLog` and continue serving.
4. Replay the ring into sqlite (best-effort).
5. Install `LogCrateBridge` / `TracingBridge`.
6. Load config; `ConfigWarnAdapter` emits warnings.
7. Serve / install.
8. On shutdown: `Flush` + join.

### CLI exception

`USAGE` / `--help` / unknown-command text stays on **stderr** (IT-1.7). Those strings are **also** `LogPort::warn` with `operation=cli`. Diagnostic fatals in `main` go to `LogPort` once the Facade exists.

### Hygiene

- Tests: `FakeLog` / `MemoryLog`; sqlite tests use tempfile or shared-cache memory URI. No `$HOME`. No `thread::sleep`. `BATCH_MAX=1` or explicit `Flush`.
- `progressive-lsp-log` is on the 95% llvm-cov denominator and the 80% mutants list.
- `check-static` on musl ELFs after rusqlite is linked. Fail closed if sqlite pulls `libdl` as `DT_NEEDED`.
- Hot path: channel send is non-blocking (`try_send`); full channel drops oldest.

### What we do not do (LOG-1–LOG-4)

- JSON files, syslog, journald, OpenTelemetry export.
- Logging LSP **message bodies** or buffer text.
- `window/logMessage` from **our** server to the editor (optional later Adapter).
- Sharing one sqlite file across two `serve` processes.
- Replacing poc-ide `RunLog`.
- Capturing engine **stdout** as text (it is LSP).
