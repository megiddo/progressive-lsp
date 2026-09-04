# Design patterns

**Rule:** every component and major type maps to a named pattern in this file. Ad-hoc “manager” / “helper” / “util” layers that hide a missing pattern are a defect. Tests should be able to name the invariant.

Every type named in [detailed-design.md](detailed-design.md) appears in this table. Plugin traits from [plugin-sdk.md](plugin-sdk.md) that are not a `Resolver` step are here too. Types from [logging.md](logging.md) are in the Global logging section (locked on LOG-0; Rust lands LOG-1+).

Related: [detailed-design.md](detailed-design.md), [plugin-sdk.md](plugin-sdk.md), [logging.md](logging.md).

## Pattern map

| Component / type | Pattern | Invariant (testable) |
|---|---|---|
| `progressive-lsp` bin | Composition root | Only the bin wires the graph; libs take traits |
| `PluginRegistry` | Factory / Registry | Lookup by `LanguageId` / pack name is deterministic; unknown → `UnsupportedLanguage`, no panic; `with_log` emits `info` `operation=resolve` on `get` fail (LOG-8) |
| `LanguageFactory` | Abstract Factory | Produces grammar id + resolver chain for one language |
| `ScriptEngineFactory` | Abstract Factory | Tests inject a fake engine; production is Rhai; watch tests do not hard-code Rhai |
| `LanguageId`, `PackageId`, `FileId`, `WorkspaceId` | Identity / interned newtype | Equality is id equality; `WorkspaceId` is a hash of the canonical absolute path; `FileId::from_uri` percent-decodes so ingest and resolve share the OS path |
| `path_to_file_uri` / `path_from_file_uri` | Adapter | Same percent-encoding as poc-ide `file_uri`; spaces and `@` round-trip; incoming `file:` URIs decode before index lookup |
| `Tier`, `LanguageVersion` | Value object | `effective` = `min(window, grammar, engine)`; never panic on newer syntax |
| `PrefixLayout`, prefix / `PROGRESSIVE_LSP_HOME` | Scoped Singleton (process) | One layout per process; tests inject prefix |
| `Config`, `ConfigOverlay`, `ConfigLoad` | Chain / Builder | Later overlay wins for keys it sets; empty TOML is valid; unknown keys warn; `[t2]` merges per language; `[log]` merges `level` / `path` independently; invalid `level` → warn + default `info`; `PROGRESSIVE_LSP_LOG_LEVEL` is resolved on `LogLevel`, not in the overlay |
| `apply_worktree_excludes` / `GitExcludeReport` | Command | Writes `.git/info/exclude` + overlay belt `.gitignore`; never edits the project’s committed `.gitignore` |
| `ProgressiveLspCap` | Value object / DTO | `version` is `v1`; `socket` may be null; stock clients ignore it |
| `InstallPlan` | Command | `apply` hashes tmp before rename; mismatch deletes tmp |
| `Manifest`, `ManifestArtifact` | Schema / DTO | Relative `rel_path` only; sha256 is 32 bytes |
| `ExplicitPacks`, `CensusSelector` | Strategy (`PackSelector`) | Explicit CSV vs census; Java census selects no T3 pack |
| Frame codec (`encode_frame` / `decode_frame`) | Adapter | `u32be` length; payload > 16 MiB fails; no silent truncate |
| Typed errors (`UnsupportedLanguage`, `EngineNotReady`, `InstallError`, `StaticLinkError`, `ScriptAbort`, `ScriptSandbox`, `ConfigError`, `WatchOverflow`, `InitializeFailed`, `EngineError`) | Domain Result | User paths never `unwrap`; T3 `EngineNotReady` falls back, does not panic; LOG-11: operational `Err` emits or is client-visible-only (`tests/log_hygiene.rs`) |
| `Resolver` chain (`TreeSitterResolver` → `HeuristicResolver` / `StackGraphResolver` → T3 adapter) | Chain of Responsibility | First capable handler wins; T3 `NotReady` does not drop T2 |
| `HeuristicResolver` vs `StackGraphResolver` | Strategy | Same `Resolver` trait; pick is config/eval, not a fork of definition; heuristics are the default |
| `T2Backend` | Value object | `heuristic` (default) or `stack-graphs`; omit = heuristic; unknown backend warns |
| `T2Table` | Value object | Per-language map on `Config`; missing language → heuristic |
| `T2Strategy` | Strategy factory | `LanguageFactory` asks `from_backend` / `inject`; tests inject `FakeResolver`; scripts cannot register definition |
| `TsgPin` | Value object | Git URL + SHA + rel path; fetch-at-SHA; never a `third_party/` dump |
| `TsgLoadState` | Value object | `Unused` vs `SourceLoaded` / `RuntimeReady` / `FetchFailed`; selected backend is never the unused slot |
| `ResolveQuery`, `QueryKind`, `ResolveResult`, `LspLocation` | Query / Command | Protocol crate builds a query; resolvers do not parse JSON-RPC; `LspLocation.data.tier` when we set `data` |
| `WorkspaceSource` adapters | Adapter | Disk/build files → `WorkspaceModel`; no compiler invocation except documented one-shots |
| `WorkspaceModel` | Domain model / DTO | Roots and classpath-like entries **exist on disk**; scripts cannot invent jars |
| `EngineAdapter` | Adapter | Child argv/stdio/ready → supervisor API |
| `EngineBinary`, `SpawnCtx`, `ChildHandle`, `ReadyKind` | Value objects for Adapter | Discover/spawn/ready go through `EngineAdapter`; supervisor does not parse pack layouts ad hoc |
| `EngineSupervisor` | Supervisor | Crash → backoff; core stays up; T2 remains. Takes `Arc<dyn LogPort>` like `ClockPort` (LOG-6); `try_spawn` / `note_crash` emit; `last_error` is still queryable. Serve must not drop the supervisor. LOG-10: `attach_if_stderr_pipe` + `FakeChildStderr` drain; `LogFileTailAdapter` / `LspLogMessageAdapter` only when a tail path / proxied logMessage exists; never attach to engine stdout |
| `LspFacade` | Facade | JSON-RPC in; domain queries out; no watch internals leak. Takes `Arc<dyn LogPort>` (LOG-7); parse / method-not-found / framing / mux emit **without** message bodies; `InitializeFailed` warn `operation=initialize` with JSON-RPC `-32002` (LOG-8); `shutdown` debug (LOG-8) |
| `ControlServer` | Facade | Proto RPCs; same domain services as LSP, different encoding; takes `Arc<dyn LogPort>` (LOG-7) or emit from `ServeHost` plane; unknown method / `Status::error` without logging body bytes |
| `IndexService` | Facade | Owns `DirtySet` + `PriorityIndex` + `IndexCache`; not a god server |
| `WatchCoalescer` | Observer + Scheduler | N events in window → 1 batch; FakeClock advances window; overflow drop emits `LogPort` warn via `LogScope` (`operation=watch`) |
| `WatchBackend` | Port / Adapter | Prod uses `notify`; tests use `FakeWatcher`; coalescer does not call OS APIs directly |
| `WatchBatch` | Event / DTO | Overflow / `need_rescan` set; never silently drop FilesSince rows without `truncated` |
| `WatchFilter` / `on_watch` | Decorator / Filter | Dropped paths never enter `DirtySet` |
| `IdentityWatchFilter` | Decorator / Filter | Pass-through; identity is a valid v1 filter |
| `DefaultIgnoreFilter` | Decorator / Filter | Drops ignore globs; manifests still pass |
| `DenyListFilter` | Decorator / Filter | Explicit drops never enter `DirtySet` |
| `FilesSinceJournal` / `FilesSinceAnswer` | Repository + DTO | Overflow or generation gap ⇒ `truncated`; never silent drop; `ServeHost::files_since` emits `LogPort` info `operation=filesSince` when truncated (LOG-8; one place, not journal+host) |
| `FilesSincePort` / `SharedCoalescer` | Port / Adapter | Control proto calls the journal; no `$/` JSON-RPC |
| `NotifyWatcher` | Adapter | Maps notify-style kinds; coalescer never calls OS APIs |
| `SharedIndex` | Adapter | `IndexService` behind a mutex is a `SymbolIndex` |
| `LanguageIndexer` / `JavaIndexer` | Visitor + Strategy | CST walk extracts symbols; index does not parse JSON-RPC |
| `JavaLanguageFactory` | Abstract Factory | `language_id` = java; T2 Strategy from config (`heuristic` default) then T1 |
| `ResolverChain` | Chain of Responsibility | First `Ready` wins; `NotReady` continues; `prepend` puts T3 `EngineResolver` first (LOG-8) |
| `NotReadyResolver` | Test double | T3 skip; must not drop a later T2 `FakeResolver` |
| `DirectoryAdapter` / `MavenAdapter` / `GradleAdapter` / `EclipseAdapter` | Adapter | Detect from files only; no host JDK |
| `WorkspaceSession` | Facade | Composition root wires watch + index + resolve; not a god `LspServer`; `LogScope` around didOpen/didChange/definition (Context Object); `didClose` debug (LOG-8); `indexer_for` None → once-per-id `info` `operation=resolve`; holds `EngineSupervisor` when serve attaches it (LOG-6); `attach_supervisor` prepends `EngineResolver` (skip-once, never fail the user) |
| `LspIntelligence` | Port | JSON-RPC facade calls domain resolve; no watch internals |
| `DirtySet` + `PriorityIndex` | Command queue + Priority | Open buffers before vendor; generation monotonic |
| `IndexCache` | Repository | Same `(grammar_ver, lang, hash)` remembered on disk under `$PREFIX/cache/` only; I/O miss emits `LogPort` warn via `LogScope` (`operation=index`); disk marker does **not** skip extract — only an in-memory symbol hit for that path skips Tree-sitter |
| `CacheKey` | Value object / identity | Path is `sanitize(grammar)/sanitize(lang)/hex(hash)`; `.`/`..` cannot escape the prefix |
| `IndexedFile.has_error` / `unparsed_note` | Value object | ERROR/MISSING nodes ⇒ note; server stays up, no panic |
| `sample_rss_bytes` / `rss_sample_label` | Value object | Darwin/Linux host sample; not an allocator-matrix CI-arch winner |
| `Config` merge | Chain / Builder | Later file wins for keys it sets; unset key falls through |
| `Installer` | Builder (plan) + Command (apply) | Hash fail → no rename to final path; hash/verify refuse emits `LogPort` warn via `LogScope` (`operation=install`) **before** cleanup `remove_file` (LOG-7); cleanup miss still emits (LOG-4) |
| `ArtifactTransport` | Strategy | `LocalFs` vs consumer SSH; install crate has no SSH types |
| `LocalFs` | Concrete Strategy | In-tree transport; no network |
| `HostProbe`, `BuildCensus`, `PackId` | Value objects | Census → packs is `PackSelector`, not a hardcoded match in the bin |
| `PackSelector` | Strategy | Explicit list vs census |
| `ScriptHost` | Interpreter + Sandbox (Proxy) | Ops cap exceeded → error, no I/O; Abort skips documented side effect. Takes `Arc<dyn LogPort>` (LOG-6); bootstrap Abort / spawn Skip / pre_index skip emit; never `OnceLock` |
| `ClockPort` | Dependency injection / Port | Tests never call `thread::sleep` |
| `FakeClock`, `FakeWatcher`, `FakeTransport`, `FakeResolver` | Test double | Same traits as prod |
| Tree-sitter CST walk | Visitor | Query/highlight via named visitors, not ad-hoc recursion in protocol |
| Mux demux / `MuxFrame` | Adapter | Opaque LSP bytes (ch 0) vs proto control (ch 1) on one pipe; 16 MiB cap |
| `DistManifest` / `DistArtifact` | Schema / DTO | Core semver ≠ engine SHA; Darwin `payload_kind=stub`; triples are musl CI targets |
| `FakeRemoteTransport` | Test double | Same `ArtifactTransport` as prod; logs put/chmod/rename/hash; no SSH types |
| Feature `lang-*` | Product variants | Disabled language → Factory missing, not a stub that panics |
| `PackageIngest` / `IngestReport` | Command | One package per step; `didChange` never waits on remaining packages |
| `WorkDoneProgress` / `ProgressKind` | Event / DTO | Standard LSP `$/progress` begin/report/end; not a `$/` FilesSince shim |
| `GraphFacts` / `ImportDecl` / `TypeEdge` / `CallSite` | Value objects | LanguageIndexer Visitor fills them; resolvers do not parse JSON-RPC; `TypeEdge` parent is a simple type name (`type_ref_simple` strips type arguments) |
| `GraphIndex` | Port | Same store as `SymbolIndex`; package tier is Graph only after ingest |
| `StackGraphResolver` | Strategy | `unused()` is NotReady; `load_java` / `with_tsg_source` loads pinned Java TSG when selected |
| `ComposerAdapter` / `GoModAdapter` / `ZigBuildAdapter` | Adapter | Manifest files only; no host php/go/zig |
| `PhpLanguageFactory` / `HtmlLanguageFactory` / `CssLanguageFactory` / `JavaScriptLanguageFactory` / `GoLanguageFactory` / `ZigLanguageFactory` | Abstract Factory | `language_id` is stable; T3 when supervisor ready (Go/Zig also require project manifest) |
| `PhpIndexer` / `HtmlIndexer` / `CssIndexer` / `JavaScriptIndexer` / `GoIndexer` / `ZigIndexer` | Visitor + Strategy | CST walk extracts symbols; index does not parse JSON-RPC |
| `HookName` / `ScriptContext` / `ScriptDecision` | Command / DTO | Abort skips the documented side effect; scripts cannot register `textDocument/definition` |
| `RhaiEngineFactory` / `FakeEngineFactory` | Abstract Factory | Tests inject a fake engine; production is Rhai |
| `ControlServer::push_tier_ready` | Observer | Push only when progressive connected; stock clients get `workDoneProgress` only |
| `FakeEngineAdapter` | Test double | Same `EngineAdapter` trait as prod; crash/backoff tests never `thread::sleep` |
| `EngineCapabilities` | Value object | Merge is OR; empty has no methods |
| `EngineResolver` | Adapter / Chain step | `NotReady` unless supervisor is ready for `(language, package)`; `with_log` + skip-once `HashSet` invariant: second `definition` for the same pair does not duplicate `info` `operation=resolve` “pack skipped” (LOG-8); `with_file_language` for the session chain; never fail the user; never emit every `definition` |
| `discover_pack` / `EngineBinary` | Repository + Value object | Missing pack or hash mismatch → no spawn; path is `$PREFIX/engines/<pack>/` |
| `BackoffPolicy` | Strategy | Delay doubles then caps; `can_respawn` uses `ClockPort.unix_ms` |
| `SpawnTweak` / `SpawnDecision` | Command / DTO | Only allowlisted argv/cwd/env apply; Abort spawn skips the engine |
| `EngineHooks` / `ScriptHookBridge` / `NoopHooks` | Port / Adapter | Supervisor does not hard-code Rhai; tests inject Abort/Noop |
| `PackAdapter` | Adapter | Discover + hash; stub bytes never exec; Darwin / non-Linux `EngineError::Spawn` (“not this OS”); Linux `Command` via `CommandSpawnPort` + `ChildIo::lsp_with_stderr_pipe`; tests inject `RecordingSpawnPort` (would-have-spawned, no exec) |
| `EngineMessage` | Event / DTO | Forwarded didChange/watch recorded on `ChildHandle` inbox |
| `PythonLanguageFactory` / `RustLanguageFactory` | Abstract Factory | `language_id` is stable; T3 only when supervisor ready (Rust also requires sysroot) |
| `PythonIndexer` / `RustIndexer` | Visitor + Strategy | CST walk extracts symbols; index does not parse JSON-RPC |
| `PyprojectAdapter` / `CargoTomlAdapter` | Adapter | Manifest files only; no host CPython/rustc |
| `RustT1Resolver` | Decorator | Missing sysroot/pack annotates T1 hover; never a dedicated Rust T2 |
| `CompileCommandsAdapter` | Adapter | Reads `compile_commands.json` only; cmake argv only if `CMakeLists.txt` already exists |
| `CsprojAdapter` | Adapter | `*.csproj` manifest only; no host `dotnet` |
| `CLanguageFactory` / `CppLanguageFactory` | Abstract Factory | T1 Tree-sitter; T3 clangd when supervisor ready; same pack serves C and C++ (`extra_languages`) |
| `CSharpLanguageFactory` | Abstract Factory | T1 + T2 heuristics; no T3 pack (matrix ceiling) |
| `CIndexer` / `CppIndexer` / `CSharpIndexer` | Visitor + Strategy | CST walk extracts symbols; index does not parse JSON-RPC |
| `EngineAdapter::extra_languages` | Adapter extension | clangd also serves `cpp`; tsgo also serves `javascript` |
| `slim_pack_names` / `full_pack_names` / `is_heavy_pack` | Strategy helpers | Slim default excludes clangd/tsgo/gopls/zls; census is still `PackSelector` |
| `ServeHost` | Facade | Composition-root serve: prefix `Config` + overlay merge + `apply_worktree_excludes` on initialize; cache stays in prefix; unknown keys do not fail; `ConfigWarnAdapter` emits; `LogScope` around didOpen/didChange/definition; initialize success `info` `operation=initialize`; truncated FilesSince `info` `operation=filesSince` (LOG-8) |
| `root_from_params` | Adapter | `rootUri` / `rootPath` / `workspaceFolders` → workspace path; no `$/` FilesSince |
| `LspStdioDriver` (`plsp-it1`) | Adapter | initialize → shutdown over Content-Length; integration only; no `$/` FilesSince |
| `ServeDiskWatch` | Observer + Adapter | Stock ghost-disk: on-disk bytes change → reindex; no progressive client; no `thread::sleep` in unit tests |
| `CorpusPin` (`integration/corpora/pins.json`) | Value object / Schema | URL + peeled SHA + entry; fetch-at-SHA; never a submodule mirror |
| `ExpectedGolden` | Schema / DTO | 0-based `find` → line/character; integration only |
| `It2BackendDriver` (`plsp-it1 backend`) | Adapter | Stock initialize/didOpen/def/hover/tokens/didChange/ghost; `$/` FilesSince must be method-not-found |
| `It2ReportRow` | DTO | `language`, `corpus_sha`, `pack`, `tier_observed`, `definition_ok`, `tokens_ok`, `ghost_edit_ok`, `notes`; T3 stub → `skip_pack_missing` |
| `Envelope` | DTO / public dispatch | `method` + `request_id` + `body`; replies echo id; pushes use `request_id == 0`; never `$/` |
| `IngestState` | Value object | `not_started` \| `running` \| `done`; unknown / empty parse → `not_started`; never panic |
| `ControlPlane` | Port | Proto RPCs call the composition-root host; control crate does not own config/watch/install internals |
| `ControlServer::dispatch_envelope` | Command | Case-sensitive method names match the API RPC table; unknown method → non-zero `Status` |
| `bind_control_socket` / `spawn_control_accept` | Adapter | Unix socket beside stdio LSP; length-prefixed Envelope; stock serve without `--control-socket` still works; bind/accept/`PayloadTooLarge` emit `operation=control` (LOG-7) without payload bytes |
| `It3ProgressiveDriver` (`plsp-it1 progressive`) | Adapter | LSP stdio + Envelope socket; IT-3.1–3.7; `--mux` is `pending_mux` (do not silently retest socket) |
| `It3ReportRow` | DTO | `backend`, `rpc`, `result`, `notes`; T3 stub → `skip_pack_missing`; mux → `pending_mux` |

## Global logging (LOG-1+)

Types from [logging.md](logging.md). LOG-1 landed Port / DTO / scope / doubles in `progressive-lsp-core`. LOG-2 landed sqlite Actor types in `progressive-lsp-log`. LOG-3 landed capture Adapters + `ChildIo`. LOG-4 wired serve/install bootstrap. LOG-6 landed supervisor/`ScriptHost` `LogPort`. LOG-9 landed `LogOpenPlan` (Command). poc-ide `RunLog` stays a separate schema ([POC IDE](#poc-ide-consumer-sample)); do not merge rows or columns.

| Component / type | Pattern | Invariant (testable) |
|---|---|---|
| `LogPort` | Dependency injection / Port | `fn emit(&self, record: LogRecord)` returns `()`; no `Result`; same injection rule as `ClockPort`; libs take `Arc<dyn LogPort>`; process-wide `OnceLock<LogPort>` is forbidden |
| `LogFacade` | Facade | Documented composition of `LogSink` + `ReentrancyGuard`; min-level filter is `LevelFilter` (this type is not a Rust Facade yet); never logs onto stdout |
| `LogRecord` | DTO | Construction never fails; omit unknown fields (sqlite NULL); `message` truncates at 64 KiB, lossy UTF-8; `sanitize_extras` drops `text` / `content` / `body` / `clipboard` / `password` / `secret` / `token`; indexes `(ts_unix_ms)`, `(level)`, `(component)`, `(content_path)`, `(source_repo)` |
| `LogLevel` | Value object | `error` `warn` `info` `debug` `trace`; unknown parse → `info` (never fail); `from_env_or_config` empty/unset → config; known env → that level; unknown env → `info` + warning (never fail boot) |
| `LevelFilter` | Decorator / Filter | Wraps `LogPort`; drops records where `!record.level.at_least(min)`; composition root sets `min` from `LogLevel::from_env_or_config` (unset/empty env + omitted `[log].level` → `info`; invalid env still boots at `info`) |
| `LogOrigin` | Value object | `FirstParty` (`progressive-lsp`) vs `ThirdParty`; `source_repo` is one of those two strings |
| `LogComponent` | Value object | Stable strings only: `core`, `protocol`, `control`, `engine`, `index`, `watch`, `install`, `script`, `lang-<id>`, pack name, `xtask` (only if a lib path logs) |
| `LogScope` / `LogScopeGuard` | Context Object | Task-local / thread-local: `content_path`, `content_line`, `operation`, `component`; `emit` copies scope fields when the caller left them unset; Drop of `LogScopeGuard` restores the previous scope (stack) |
| `LogSink` | Port | Durable append; prod `SqliteLogRepository`; tests `FakeLog` (mutex `Vec` for assertions) |
| `NeverFailLog` | Decorator | Wraps a `LogSink` that may return `Result`; swallows errors so a full disk cannot panic `serve`; `emit` still returns `()` |
| `FakeLog` | Test double | Same `LogPort` / `LogSink`; records into a mutex `Vec`; tests never open `$HOME` |
| `MemoryLog` | Test double / bootstrap ring | Cap 4096; used before prefix exists and when **all** `LogOpenPlan` WAL opens fail (LOG-9); replay into the WAL that opened is best-effort |
| `NullLog` | Test double | Never-fail no-op; `emit` is a no-op and does not panic |
| `SqliteLogRepository` | Adapter / Repository | One WAL file per serve/install process; `Drop` sends `Shutdown` and joins the Actor without `thread::sleep` (`BATCH_MAX=1` so join is immediate); composition root `Flush` before process end |
| `WriterActor` | Actor | Owns the `rusqlite::Connection`; receives `LogRecord` / `Flush` / `Shutdown` on mpsc; `check_same_thread` stays true; the writer thread **never** calls `LogPort` |
| `CrashSafeBatch` | Unit of Work | Commit when `len >= BATCH_MAX` (default 32) **or** `ClockPort` elapsed ≥ `BATCH_MS` (default 50; production only) **or** incoming `level == Error` (including that record) **or** `Flush` / `Shutdown` / `Drop`; each commit is `BEGIN IMMEDIATE` … `COMMIT`; tests set `BATCH_MAX = 1` or call `Flush` — never `thread::sleep`; COMMIT failure keeps a retry `Vec` (cap 1024), overflow drops oldest, increments `dropped_count`, inserts one `warn` meta row (`operation = "log"`) on the next successful commit; pragmas: `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, `wal_autocheckpoint=1000` |
| `ServeLogPath` | Value object | `{log_dir}/serve-{unix_ms}-{pid}.sqlite`; `fallback` is `{log_dir}/serve-fallback-{unix_ms}-{pid}.sqlite`; `in_temp` is `{temp_dir}/progressive-lsp-serve-{unix_ms}-{pid}.sqlite` (LOG-9); tests inject `:memory:` (shared-cache URI) or a tempfile; empty / unset `PROGRESSIVE_LSP_LOG` / `[log].path` → primary default; never `$HOME` |
| `LogOpenPlan` | Command | Ordered WAL open: primary `ServeLogPath` → same-dir `fallback` → `in_temp`; first success wins; emit `warn` `operation=log` naming the path and the prior failure; all three fail → keep `MemoryLog`; `emit` still returns `()`; tests inject both directories — no `thread::sleep` |
| `ReentrancyGuard` | Proxy / Guard | Thread-local `IN_EMIT`; if `emit` is already on the stack, enqueue on the Actor channel without taking Facade locks that could deadlock |
| `StderrEmitAdapter` | Adapter | Former diagnostic `eprintln!` sites; after LOG-3, grep of diagnostic `eprintln!` in `src/` and `progressive-lsp-*` is empty except tests and the CLI usage exception |
| `LogCrateBridge` | Adapter | `log::Log::log`; origin is third-party unless target starts with `progressive_lsp`; installed once in the composition root |
| `TracingBridge` | Adapter | `tracing` `Event`s; same origin rule as `LogCrateBridge`; server default features have no tracing emitters |
| `ChildStderrAdapter` | Observer + Adapter | Line-delimited stderr of a pack; origin third-party; stdout of the child is **never** this Adapter; bounded drain so stderr cannot stall LSP; invalid UTF-8 → lossy; no regex panic; `attach_if_stderr_read` is `Some` only when a stderr pipe **and** a `Read`/`FakeChildStderr` exist (LOG-10) |
| `LogFileTailAdapter` | Adapter | Engine log **file**; origin third-party; prefer `$PREFIX/log/<pack>/`; do not parse LSP from the file; `attach_if_tail_path` is `Some` only when a tail path exists (LOG-10) |
| `LspLogMessageAdapter` | Adapter | `window/logMessage` / `window/showMessage` / `$/logTrace`; origin third-party; secondary — never a substitute for crash/panic on stderr; `attach_if_proxied` is `Some` only for those methods (LOG-10) |
| `ConfigWarnAdapter` | Adapter | `ConfigLoad.warnings`; first-party; unknown keys emit `warn` + `operation=config` |
| `CliUsageAdapter` | Adapter | `--help` / usage; first-party; **also** writes stderr (IT-1.7); `LogPort::warn` with `operation=cli` |
| `NullStderrAdapter` | Adapter | `stderr(Stdio::null())`; **Forbidden** on production pack spawn |
| `InheritStderrAdapter` | Adapter | `stderr(Stdio::inherit())`; operator/CI harness bins only — never `serve` |
| `ChildIo` | Value object | stdout is always LSP JSON-RPC (never a log Adapter); stderr is an optional capture pipe; prod pack spawn is `lsp_with_stderr_pipe` — never `NullStderrAdapter`; LOG-10 attaches `ChildStderrAdapter` when a `Read` exists (tests: `FakeChildStderr`; Linux `Command` leaves an OS stderr pipe on `ChildHandle`) |
| `SpawnPlan` | Value object | argv + cwd + env + `ChildIo` for the Linux `Command`; Darwin unit tests assert the plan without `Command`; production plan is always `lsp_with_stderr_pipe` |
| `SpawnPort` / `CommandSpawnPort` / `RecordingSpawnPort` | Port + Adapter / test double | Production Port is Linux `Command` (stdin/stdout/stderr piped); Darwin / non-Linux refuse; tests inject `RecordingSpawnPort` so “would have spawned” does not exec |
| `MuslBuildPlan` | Value object | triple + docker platform (`linux/amd64` / `linux/arm64`) + dockerfile + dest `target/musl/<triple>/progressive-lsp` + `RUST_TARGET`; Darwin unit tests name the pattern and cover both triples without docker; unknown triple / missing dockerfile fail closed |
| `PackPin` | Value object | Pack name + binary + upstream git URL + **40-hex SHA** (not `latest`); `xtask/pack-pins.toml`; not core crate semver |
| `PackKind` | Value object | `rust`, `zig`, `go`, `cached`, or `cmake`; unknown kind fail closed; host php/Node/JVM/CPython forbidden as our runtime; `cached` is clangd default (cache COPY, never cmake); `cmake` is clangd `--cache-fill` only |
| `RustToolchainPin` | Value object | Pack-build rustc channel (`1.98.0`) when upstream has no `rust-toolchain.toml`; not core crate MSRV |
| `ZigToolchainPin` | Value object | Zig version + per-arch tarball SHA256 for the **pack build container** only — not a shipped `.so` |
| `GoToolchainPin` | Value object | Go version for the **pack build container** only; `CGO_ENABLED=0`; not a shipped SDK |
| `PackBuildPlan` | Value object | pack name, binary name, triple, platform, dockerfile, dest `target/musl/<triple>/engines/<pack>/<binary>`, pinned SHA, cache key `sha:triple`; Darwin unit tests name the pattern and cover slim **and** heavy plans (gopls/tsgo/zls/clangd) without docker; unknown packs fail closed; clangd cache miss is a documented gap, not cmake |
| `PackBuildPlan::cache_fill_for_pin` | Command | clangd only; kind becomes `Cmake`; dest is the cache ELF `target/pack-cache/clangd/<sha>/<triple>/clangd`; default `xtask pack` never constructs this; non-clangd refuses |
| `PackOutcome` | Value object / Domain Result | `Pass` after extract or cache COPY; `Miss` is a documented clangd cache gap (not cmake, not a Mach-O green, not `Err`) |
| `RuntimeImagePlan` | Value object | platform + triple + dockerfile + core dest + pack dests + image tag `progressive-lsp-runtime:local`; prefix inside the image is `/opt/plsp`; Darwin unit tests name the pattern and cover both triples without docker; unknown triple / missing dockerfile / missing required core ELF fail closed; `superhtml` × x86_64 may be omitted (HOST-3 miss); full packs optional (HOST-7 miss) |
| `PackImageCopy` | Value object | one pack ELF to copy (`pack`, `binary`, `src`, required); dest under `/opt/plsp/engines/<pack>/<binary>`; slim required except `superhtml` on `x86_64-unknown-linux-musl`; full (clangd/tsgo/gopls/zls) always optional |
| `DockerPort` / `CommandDockerPort` / `RecordingDockerPort` | Port + Adapter / test double | Production Port is `docker build --output type=local` (BuildKit extract) **and** `docker build -t` (runtime image tag). Same Port for core musl, slim + full packs (`extract`), and `xtask runtime-image` (`tag_image`); tests inject `RecordingDockerPort` so “would have built / tagged” writes a fixture ELF or records args — not a musl green, no daemon |
| `FakeChildStderr` | Test double | Bounded line source (`STDERR_DRAIN_CAP`) for `ChildStderrAdapter`; overflow drops oldest so stderr cannot stall LSP |

## Patterns we do not use (v1)

| Pattern | Why not |
|---|---|
| Plugin `dlopen` | Fights musl-static |
| Singleton global `REGISTRY` without injection | Untestable; use composition root |
| Process-wide `OnceLock<LogPort>` | Untestable; the bin injects `Arc<dyn LogPort>` |
| God `LspServer` that owns watches, engines, and Rhai | Split Facades + Supervisor |
| Scripts as Strategy for `textDocument/definition` | Forbidden; tests assert |
| Ad-hoc manager / helper / util crates | Missing pattern; add a row here instead |

## POC IDE (consumer sample)

In-tree editor in `poc-ide/`. Types live there only. The server map above is unchanged. Architecture: [poc-ide/architecture.md](poc-ide/architecture.md).

| Component / type | Pattern | Invariant (testable) |
|---|---|---|
| `poc-ide` bin (`main.rs`) | Composition root | Only the bin wires eframe/`rfd` / `ArboardClipboard`; lib takes Ports |
| `ArboardClipboard` | Adapter | Bin-only `ClipboardPort`; lib tests use `FakeClipboard` |
| `IdeError` | Domain Result | User paths never `unwrap`; each variant has a Display + classifier test. `NoFileOpen` is the discover / context-menu empty-tab error |
| `DirEntry` | DTO | Immediate child name + path + `is_dir`; `FsPort.read_dir` only |
| `DialogPort` / `RfdDialog` | Port / Adapter | Open folder/file goes through the Port; tests never call `rfd` |
| `PendingDialog` / `DialogAction` / `DialogOutcome` | Command / value | File-menu click records Open Folder / Open Folder in Container / Open File; apply runs the Port after the menu closes; cancel is `Cancelled`, not an error |
| `FakeDialog` | Test double | Same `DialogPort`; returns queued paths |
| `WorkspaceRoot` | Value object / identity | Canonical absolute path; equality is path equality |
| `FsPort` / `StdFs` | Port / Adapter | Tree/read/write go through the Port; tests use `MemFs` |
| `MemFs` | Test double | Same `FsPort`; no host disk |
| `CountingFs` | Decorator / test double | Wraps `MemFs`; records `read_dir` paths; inner Port is unchanged; used to prove shallow load / idempotent expand |
| `FileTree` / `TreeNode` | Composite | Directories contain children; files are leaves; skip `.git`/`target`/`node_modules` display filter. `load` is shallow (immediate children only); child dirs start unloaded (`children: None`); `expand` / `load_children` fills one level (`Some(vec![])` is an empty loaded folder). `load_compact_chain` loads a single-child-dir chain for a compact row without changing `TreeExpansion`. `apply_listing` grafts a [`CompactChainListing`] without calling `FsPort`. Listing order is non-dot dirs, non-dot files, dot dirs, dot files (lexicographic within each group). |
| `ExpandChainCommand` | Command | Lists a compact single-child directory chain via `FsPort`; does not mutate `FileTree`. Tests use `MemFs` / `CountingFs` — no `thread::sleep` |
| `CompactChainListing` | DTO | Path + listed children per level; `apply_listing` is the UI apply. Empty listing is a no-op on the workspace root |
| `TreeIoRequest` | Command | Expand path sent on the tree IO channel |
| `TreeIoEvent` | Event | Inbox yield: expanded listing or failed path + error |
| `TreeIoMailbox` / `TreeIoHandle` | Command queue + Event inbox | UI `submit` / `poll` never call `FsPort::read_dir`; the tree worker (or test `pump_tree_io`) owns the Port |
| `TreeExpandFlight` | Value object | Pending expand paths; second `begin` of the same path is a no-op; `can_expand` is false while loading; `loading_label` is `loading…`; not a Manager |
| `CompactChain` | Value object / view of Composite | `/`-joined names of already-loaded single-child directories; `path` is the innermost directory. Unloaded / empty / one file child / 2+ children stop the chain. Length 1 is a non-compact directory. Skip-filtered names cannot be the "one child." |
| `TreeExpansion` | Value object / collection | A path is expanded iff explicitly expanded; default is collapsed at every level. `for_root` / a new `FileTree` starts empty. `expand` / `collapse` are Commands. Collapse of a missing path is a no-op. Expanding a file is a no-op. Expanding a parent does not expand children. Expanding a compact row expands the innermost path only — nested names in the chain are not auto-expanded. |
| `LayoutState` | Value object | `left_width` > 0; clamp on set; no window handle in the lib |
| `TabStrip` / `TabId` | Identity + collection | Focus is at most one tab; close missing id is a no-op |
| `OpenBuffer` / `BufferMap` | Entity + Identity | One buffer per canonical path; rope is source of truth. `generation` bumps on insert / delete / reload, not on selection or save; the highlighter cache keys on it |
| `Selection` | Value object | Range is ordered `start <= end` in char offsets |
| `CursorOffsets` | Value object | Editor char offsets → `Selection`; apply writes the caret onto `OpenBuffer` without dirtying; offsets → `position_at` is not always line 0 character 0 |
| `DirtyFlag` | Value object | Edit sets dirty; successful save clears it |
| `EditCommand` | Command | Insert/delete/cut/copy/paste mutate rope only via this Command |
| `DiscoverKind` | Value object | Definition / Implementation / References; `lsp_method` is the stock JSON-RPC name |
| `DiscoverCommand` | Command | Focused tab + cursor → IO `LspIoRequest` (`to_io_request`) or blocking `apply` for FakeLsp tests; `apply_locations` jumps when the inbox yields. No file open / missing client are domain errors, not panics; empty location list is valid. UI-facing `to_io_request` / `apply_locations` never call `LspTransport::request` |
| `PendingDiscover` | Command / value | Click records a `DiscoverKind`; `to_io_request` queues an `LspIoRequest` after the menu closes; close does not panic |
| `LspIoRequest` | Command | didOpen / didChange / didSave / didClose / discover / shutdown / initialize sent on the LSP IO channel |
| `LspIoEvent` | Event | Inbox yield: initialized, notify acks, discover locations, `$/progress`, `window/logMessage`, child stderr, shutdown, failed |
| `LspIoMailbox` / `LspIoHandle` | Command queue + Event inbox | UI `submit` / `poll` never call `LspTransport::request`; the IO thread (or test `pump_lsp_io`) owns the transport |
| `ProgressEvent` / `LspProgressKind` | Event / DTO | `$/progress` begin/report/end kept by the reader; token may be string or number; unknown kind is dropped |
| `LogMessageEvent` | Event / DTO | `window/logMessage` kept by the reader; missing `type` is 0 |
| `DiscoverFlight` | Value object | Idle vs in-flight kind; second `begin` is a no-op; `waiting_label` is `waiting for server` iff in flight; not a Manager |
| `ClipboardPort` / `FakeClipboard` | Port / Adapter + test double | Cut/copy/paste never call OS clipboard in tests |
| `Highlighter` | Adapter + Cache | syntect `InspiredGitHub` (dark token colors on the light egui editor); unknown syntax → empty/plain spans, no panic. `highlight(path, text, generation)` hits [`HighlightCache`]; a second call with the same path + generation does not re-tokenize (`tokenize_count` stays) |
| `HighlightKey` | Value object / identity | Path + rope generation; equality is both fields |
| `HighlightCache` | Cache | Stores `Vec<HighlightSpan>` for one key; hit returns the spans; miss tokenizes and stores; `clear` drops the entry |
| `HighlightSpan` | Value object / DTO | Char range `start <= end`; RGB from syntect; unknown syntax yields empty list; unhighlighted text uses `PLAIN_TEXT_RGB` |
| `WatchPort` / `NotifyWatch` | Port / Adapter | Prod uses `notify`; coalescer/IDE does not call OS APIs directly |
| `WatchDepth` | Value object | `immediate` vs `recursive`; folder open uses immediate so a large tree does not block on a recursive OS watch |
| `LspSessionState` | Value object | `idle` / `connecting` / `ready` / `failed`; connecting is not ready; tree paint must not wait for `ready` |
| `FakeWatch` | Test double | Same `WatchPort`; tests inject events; no `thread::sleep` |
| `DiskEvent` / `DiskEventKind` | Event / DTO | path + kind + mtime; `KeepMemory` ignores a later event with the same mtime |
| `ClockPort` / `FakeClock` (poc-ide) | Port / test double | Tests never `thread::sleep`; advance with FakeClock |
| `SystemClock` (poc-ide) | ClockPort production | Wall `unix_ms`; tests use `FakeClock` |
| `DiskWatch` | Observer | Watch events for an open path enqueue at most one pending `ConflictModal` per path |
| `ConflictModal` / `ConflictChoice` | Command | `LoadDisk` replaces rope from `FsPort` and clears dirty; `KeepMemory` keeps rope and records `ignored_mtime` |
| `HostOs` | Value object | `linux` vs `other`; container File-menu item only on `other` |
| `OpenMode` | Strategy | `native` vs `container`; `for_host` forces native on Linux; T3 offered for native-on-Linux or container; never two LSP processes |
| `T3HostOffer` | Value object | `Offered` vs `NeedsContainer`; strip skip + discover `open folder in container` |
| `LaunchFlags` / `parse_launch_args` | DTO + parser | `--folder` / `--file` / `--container` / `--control-socket`; tests parse strings |
| `RuntimePort` / `FakeRuntime` / `DockerRuntime` | Port / test double / Adapter | Tests inject `FakeRuntime`. `DockerRuntime` uses a missing binary or a scripted CLI in tests; no daemon, registry, or AWS. `start` validates [`DockerRunPlan`] and does not exec; `MuxStdio::from_command` is the single container `docker run` |
| `DockerRunPlan` | Value object | docker binary + `run -i --rm` + `-v WS:WS` + `-w WS` + image `progressive-lsp-runtime:local` + `serve --prefix /opt/plsp --mux`; never `-t`; empty / relative workspace and missing absolute docker binary fail closed; Darwin unit tests name the pattern and cover the plan without exec |
| `RuntimeInfo` | Value object / DTO | `available` + platform string; `is_linux_pack_platform` is `linux/arm64` / `linux/amd64` (and `aarch64`/`x86_64` aliases); empty platform is not available |
| `RuntimeSession` | Value object | Workspace path of a validated container plan; Clone; does not own Child; tests never hold a live Docker id |
| `LaunchJournal` / `LaunchStep` / `StepState` | Value objects | Ordered `pending`/`running`/`ok`/`fail`/`skipped`; container plan: probe → platform → image → mount → start → T3 preflight |
| `StatusModal` / `StatusModalKind` | Value object | Closed or open T1/T2/T3/container; Close does not cancel work |
| `RuntimeIoRequest` / `RuntimeIoEvent` / `RuntimeIoMailbox` / `RuntimeIoHandle` | Command + Event mailbox | UI submits launch; worker yields `Progress` then `Finished` journal; tests `pump_runtime_io` / `FakeRuntime` |
| `LanguageCatalog` | Registry | Extension lookup is deterministic; unknown → `plaintext`; plaintext skips `didOpen`. `discover_offers` is method × min tier × ceiling from the language matrix; Java has no T3 offers; C# ceiling is T1/T2 |
| `WireTier` | Value object | `syntax` / `graph` / `types`; unknown parse → `None`; `meets` is `>=` |
| `DiscoverOffer` | Value object | One LSP method + `min_tier` + language `ceiling`; Java/C# ceiling is `graph`; typed-only methods have `min_tier == types` |
| `ServeMode` | Strategy | `StockStdio` vs `ControlSocket` vs `Mux`; **default is `ControlSocket`**; `StockStdio` remains an explicit variant; `ControlSocket` spawn takes a separate `ControlSocketPath` (the enum does not own the path); `Mux` argv is `serve --mux`; container attach is `Mux` |
| `LspIoAttach` | Strategy | `Native(ServeSpawn)` vs `Container(DockerRunPlan)`; native keeps `ControlSocket`; container is `Mux` + docker Command (`serve --prefix /opt/plsp --mux`); one process |
| `MuxStdio` / `MuxLsp` / `MuxControl` | Adapter | One stdio pipe, protocol `MuxFrame` (`u8` + `u32be` + payload). Channel 0 = opaque JSON-RPC body (same as `serve_mux`, no Content-Length wrapper). Channel 1 = length-prefixed Envelope. Unknown channel and payload > 16 MiB fail closed. Pair / Cursor tests; no daemon |
| `ControlAttach` | Strategy | `Socket(path)` vs `Mux`. `advertised_control(cap, mode)` returns `Mux` when `ServeMode::Mux` is selected. `pending_mux` only when mux is advertised but not selected. `advertised_control_socket` stays socket-only |
| `ControlSocketPath` | Value object | CLI path wins; else `$PREFIX/run/poc-ide.sock`; else `$HOME/.progressivelsp/run/poc-ide.sock`; else `{temp}/poc-ide.sock`; tests inject prefix / home / temp — never require `$HOME` |
| `ServeWalPath` | Value object | Unique `{log_dir}/serve-{unix_ms}-{pid}.sqlite` the IDE sets on `PROGRESSIVE_LSP_LOG`; tests inject dirs + FakeClock |
| `ServeSpawn` | Value object | Child argv + `PROGRESSIVE_LSP_LOG_LEVEL=debug` + optional `PROGRESSIVE_LSP_LOG`; `fn build_serve_command` is a function (not a type) that applies this onto `std::process::Command` — tests inspect env/argv and do not spawn a live serve; stderr is piped, never inherited |
| `ChildStderrDrain` | Observer + Adapter | Line-delimited child stderr → `RunLog` (`category=lsp`, `event=child_stderr`); `STDERR_DRAIN_CAP=1024` overflow drops oldest; tests drain a `Cursor` / `push_line` without a thread; never attached to child stdout |
| `RunStart` | DTO | `run_start` payload always has `binary`, `argv`, `log_level`, `run_log_path`, `serve_wal_path` (`not open yet` when unset) |
| `ProofStatus` | DTO / Value object | Footer: binary basename, log level, RunLog path, serve WAL path, last discover (`definition L23:88 → 0 locations`); last discover is filled from `RunLog` discover rows after the inbox yields |
| `LspTransport` / `StdioLsp` | Port / Adapter | Content-Length JSON-RPC; lib does not parse via `egui`. Reader keeps no-id `$/progress` and `window/logMessage` on `take_notifications` — it does not drop them |
| `LspCall` | DTO | Recorded request or notification on `FakeLsp`; method is the JSON-RPC name |
| `FakeLsp` | Test double | Same `LspTransport`; scripted responses; missing binary is a Result |
| `LspClient` | Facade | JSON-RPC in; domain locations out; no watch internals |
| `ProgressiveLspCap` (poc-ide) | Value object / DTO | version is `v1`; socket may be null; `mux` is true on container `--mux`; `LspClient` never opens a socket; `ControlClient` uses Unix socket in `ControlSocket` and `MuxControl` in `Mux` |
| `ControlTransport` / `UnixControl` / `MuxControl` | Port / Adapter | Envelope + `u32be` frames; payload > 16 MiB fails. `MuxControl` wraps the same inner frame in protocol channel 1 |
| `FakeControl` | Test double | Same `ControlTransport`; pushes use `request_id == 0` |
| `ControlClient` | Adapter | Unary RPCs + push dispatch; never `$/` FilesSince |
| `ControlPush` | Event / DTO | `WatchBatch` or `TierReady`; `request_id` is always 0 |
| `ControlPushInbox` | Observer | UI `ingest` / `poll` of `ControlPush`; never calls `index_status` / `tier_status` |
| `ControlIoEvent` / `ControlIoHandle` | Event + inbox | Control IO thread yields Connected / IndexStatus / TierStatus / Push / Failed; `fn ui` only `poll`s; unary snapshots are requested on the IO thread |
| `PackageTierMap` | Value object / collection | Applies IndexStatus ingest + TierStatus / TierReady; focused path picks a package id in the path, else workspace max tier. `ingest_for_strip` treats Connecting + no IndexStatus as Running and Ready + no IndexStatus as Done (initialize ingest is sync) |
| `TierCell` / `TierCellKind` / `TierCellState` | Value objects | Cell is T1/T2/T3 × `processing` / `done` / `not supported` / `skipped` / `n/a`; `n/a` is waiting (or matrix-no-T2); Java T3 is `not supported`; Rust/CSS T2 is `n/a`; native non-Linux T3 is `skipped`; stub refuse is `skipped`, never `done` |
| `TierStrip` | Value object | Three cells from LanguageCatalog × ingest × current wire tier × `T3HostOffer`; no focused file still paints T1/T2 from workspace ingest; ingest running is T2 `processing` without waiting for Syntax; `ui.rs` renders cells as buttons that open `StatusModal` |
| `DiscoverMenu` / `DiscoverMenuItem` / `MenuDisableReason` | Value objects | Same ground truth for Navigate and context menus; disabled labels are `connecting language server` / `building T1 index` / `waiting for server` / `needs T2` / `needs T3` / `not supported` / `T3 skipped (stub pack)` / `open folder in container`; `to_io_request` is `None` while disabled so FakeLsp is not called |
| `ProtocolConsole` / `TranscriptEntry` | Facade + DTO | Append-only transcript; send does not panic on server error |
| `TranscriptKind` | Value object | Lsp vs Control vs error; `is_push` only for `ControlPush` with `request_id == 0` |
| `IdeError::Control` | Domain Result | missing socket / payload too large / `pending_mux` only when mux is advertised but not selected; stock LSP remains |
| `LspLocation` (poc-ide) | Value object / DTO | uri + range from the client; jump opens or focuses a tab; empty list is valid |
| `file_uri` | Adapter | Absolute path → `file:` URI with percent-encoding; spaces and other reserved bytes are `%XX`; same codec as core `path_to_file_uri` |
| `SpawnSpec` | Value object | Binary from env, then `target/…/progressive-lsp`, then `PATH`; missing → error not panic |
| `RunLog` | Repository | One sqlite file (or `:memory:`) per run; append + query; write failure is `IdeError::Log`, never a panic. Discover rows include `path`, `uri`, `line`, `character`, `location_count` |
| `RunLogPath` | Value object | `{dir}/poc-ide-{unix_ms}-{pid}.sqlite`; tests inject dir / path |
| `LogRow` | DTO | `timestamp_ms` + `category` + `event` + optional JSON; payload is structured, never file bodies |
| `LogCategory` | Value object | `run` / `ui` / `tree` / `tab` / `buffer` / `lsp` / `control` / `conflict` / `runtime`; unknown parse → `None` |
| `IdeError::Log` | Domain Result | Classifier `is_log`; composition root ignores write failures |
| `IdeError::Runtime` | Domain Result | Classifier `is_runtime`; Docker probe / image / start / T3 preflight |

## xtask (operator CLI)

llvm-cov excludes `xtask/`. Spawn shells are not on the 95% denominator.

| Component / type | Pattern | Invariant (testable) |
|---|---|---|
| `PocArgs` | Value object | Split at the first `--`; left side is xtask flags (`-h` / `--help` only); right side is forwarded to `poc-ide`; leftover without `--` is an error. `poc::run` spawn is a thin shell (N/A for unit tests). |

## Adding a type

1. Name the pattern in this table (PR must update the table).
2. Put the invariant in a unit test next to the type.
3. If you cannot name the pattern, the type should not exist yet.
