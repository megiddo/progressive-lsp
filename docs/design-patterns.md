# Design patterns

**Rule:** every component and major type maps to a named pattern in this file. Ad-hoc “manager” / “helper” / “util” layers that hide a missing pattern are a defect. Tests should be able to name the invariant.

Every type named in [detailed-design.md](detailed-design.md) appears in this table. Plugin traits from [plugin-sdk.md](plugin-sdk.md) that are not a `Resolver` step are here too.

Related: [detailed-design.md](detailed-design.md), [plugin-sdk.md](plugin-sdk.md).

## Pattern map

| Component / type | Pattern | Invariant (testable) |
|---|---|---|
| `progressive-lsp` bin | Composition root | Only the bin wires the graph; libs take traits |
| `PluginRegistry` | Factory / Registry | Lookup by `LanguageId` / pack name is deterministic; unknown → `UnsupportedLanguage`, no panic |
| `LanguageFactory` | Abstract Factory | Produces grammar id + resolver chain for one language |
| `ScriptEngineFactory` | Abstract Factory | Tests inject a fake engine; production is Rhai; watch tests do not hard-code Rhai |
| `LanguageId`, `PackageId`, `FileId`, `WorkspaceId` | Identity / interned newtype | Equality is id equality; `WorkspaceId` is a hash of the canonical absolute path |
| `Tier`, `LanguageVersion` | Value object | `effective` = `min(window, grammar, engine)`; never panic on newer syntax |
| `PrefixLayout`, prefix / `PROGRESSIVE_LSP_HOME` | Scoped Singleton (process) | One layout per process; tests inject prefix |
| `Config`, `ConfigOverlay`, `ConfigLoad` | Chain / Builder | Later overlay wins for keys it sets; empty TOML is valid; unknown keys warn; `[t2]` merges per language |
| `apply_worktree_excludes` / `GitExcludeReport` | Command | Writes `.git/info/exclude` + overlay belt `.gitignore`; never edits the project’s committed `.gitignore` |
| `ProgressiveLspCap` | Value object / DTO | `version` is `v1`; `socket` may be null; stock clients ignore it |
| `InstallPlan` | Command | `apply` hashes tmp before rename; mismatch deletes tmp |
| `Manifest`, `ManifestArtifact` | Schema / DTO | Relative `rel_path` only; sha256 is 32 bytes |
| `ExplicitPacks`, `CensusSelector` | Strategy (`PackSelector`) | Explicit CSV vs census; Java census selects no T3 pack |
| Frame codec (`encode_frame` / `decode_frame`) | Adapter | `u32be` length; payload > 16 MiB fails; no silent truncate |
| Typed errors (`UnsupportedLanguage`, `EngineNotReady`, `InstallError`, `StaticLinkError`, `ScriptAbort`, `ScriptSandbox`, `ConfigError`, `WatchOverflow`, `InitializeFailed`, `EngineError`) | Domain Result | User paths never `unwrap`; T3 `EngineNotReady` falls back, does not panic |
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
| `EngineSupervisor` | Supervisor | Crash → backoff; core stays up; T2 remains |
| `LspFacade` | Facade | JSON-RPC in; domain queries out; no watch internals leak |
| `ControlServer` | Facade | Proto RPCs; same domain services as LSP, different encoding |
| `IndexService` | Facade | Owns `DirtySet` + `PriorityIndex` + `IndexCache`; not a god server |
| `WatchCoalescer` | Observer + Scheduler | N events in window → 1 batch; FakeClock advances window |
| `WatchBackend` | Port / Adapter | Prod uses `notify`; tests use `FakeWatcher`; coalescer does not call OS APIs directly |
| `WatchBatch` | Event / DTO | Overflow / `need_rescan` set; never silently drop FilesSince rows without `truncated` |
| `WatchFilter` / `on_watch` | Decorator / Filter | Dropped paths never enter `DirtySet` |
| `IdentityWatchFilter` | Decorator / Filter | Pass-through; identity is a valid v1 filter |
| `DefaultIgnoreFilter` | Decorator / Filter | Drops ignore globs; manifests still pass |
| `DenyListFilter` | Decorator / Filter | Explicit drops never enter `DirtySet` |
| `FilesSinceJournal` / `FilesSinceAnswer` | Repository + DTO | Overflow or generation gap ⇒ `truncated`; never silent drop |
| `FilesSincePort` / `SharedCoalescer` | Port / Adapter | Control proto calls the journal; no `$/` JSON-RPC |
| `NotifyWatcher` | Adapter | Maps notify-style kinds; coalescer never calls OS APIs |
| `SharedIndex` | Adapter | `IndexService` behind a mutex is a `SymbolIndex` |
| `LanguageIndexer` / `JavaIndexer` | Visitor + Strategy | CST walk extracts symbols; index does not parse JSON-RPC |
| `JavaLanguageFactory` | Abstract Factory | `language_id` = java; T2 Strategy from config (`heuristic` default) then T1 |
| `ResolverChain` | Chain of Responsibility | First `Ready` wins; `NotReady` continues |
| `NotReadyResolver` | Test double | T3 skip; must not drop a later T2 `FakeResolver` |
| `DirectoryAdapter` / `MavenAdapter` / `GradleAdapter` / `EclipseAdapter` | Adapter | Detect from files only; no host JDK |
| `WorkspaceSession` | Facade | Composition root wires watch + index + resolve; not a god `LspServer` |
| `LspIntelligence` | Port | JSON-RPC facade calls domain resolve; no watch internals |
| `DirtySet` + `PriorityIndex` | Command queue + Priority | Open buffers before vendor; generation monotonic |
| `IndexCache` | Repository | Same `(grammar_ver, lang, hash)` → skip parse; disk under `$PREFIX/cache/` only |
| `CacheKey` | Value object / identity | Path is `sanitize(grammar)/sanitize(lang)/hex(hash)`; `.`/`..` cannot escape the prefix |
| `IndexedFile.has_error` / `unparsed_note` | Value object | ERROR/MISSING nodes ⇒ note; server stays up, no panic |
| `sample_rss_bytes` / `rss_sample_label` | Value object | Darwin/Linux host sample; not an allocator-matrix CI-arch winner |
| `Config` merge | Chain / Builder | Later file wins for keys it sets; unset key falls through |
| `Installer` | Builder (plan) + Command (apply) | Hash fail → no rename to final path |
| `ArtifactTransport` | Strategy | `LocalFs` vs consumer SSH; install crate has no SSH types |
| `LocalFs` | Concrete Strategy | In-tree transport; no network |
| `HostProbe`, `BuildCensus`, `PackId` | Value objects | Census → packs is `PackSelector`, not a hardcoded match in the bin |
| `PackSelector` | Strategy | Explicit list vs census |
| `ScriptHost` | Interpreter + Sandbox (Proxy) | Ops cap exceeded → error, no I/O; Abort skips documented side effect |
| `ClockPort` | Dependency injection / Port | Tests never call `thread::sleep` |
| `FakeClock`, `FakeWatcher`, `FakeTransport`, `FakeResolver` | Test double | Same traits as prod |
| Tree-sitter CST walk | Visitor | Query/highlight via named visitors, not ad-hoc recursion in protocol |
| Mux demux / `MuxFrame` | Adapter | Opaque LSP bytes (ch 0) vs proto control (ch 1) on one pipe; 16 MiB cap |
| `DistManifest` / `DistArtifact` | Schema / DTO | Core semver ≠ engine SHA; Darwin `payload_kind=stub`; triples are musl CI targets |
| `FakeRemoteTransport` | Test double | Same `ArtifactTransport` as prod; logs put/chmod/rename/hash; no SSH types |
| Feature `lang-*` | Product variants | Disabled language → Factory missing, not a stub that panics |
| `PackageIngest` / `IngestReport` | Command | One package per step; `didChange` never waits on remaining packages |
| `WorkDoneProgress` / `ProgressKind` | Event / DTO | Standard LSP `$/progress` begin/report/end; not a `$/` FilesSince shim |
| `GraphFacts` / `ImportDecl` / `TypeEdge` / `CallSite` | Value objects | LanguageIndexer Visitor fills them; resolvers do not parse JSON-RPC |
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
| `EngineResolver` | Adapter / Chain step | `NotReady` unless supervisor is ready for `(language, package)` |
| `discover_pack` / `EngineBinary` | Repository + Value object | Missing pack or hash mismatch → no spawn; path is `$PREFIX/engines/<pack>/` |
| `BackoffPolicy` | Strategy | Delay doubles then caps; `can_respawn` uses `ClockPort.unix_ms` |
| `SpawnTweak` / `SpawnDecision` | Command / DTO | Only allowlisted argv/cwd/env apply; Abort spawn skips the engine |
| `EngineHooks` / `ScriptHookBridge` / `NoopHooks` | Port / Adapter | Supervisor does not hard-code Rhai; tests inject Abort/Noop |
| `PackAdapter` | Adapter | Discover + hash; stub bytes never exec (CI/Docker builds real musl ELFs) |
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
| `ServeHost` | Facade | Composition-root serve: prefix `Config` + overlay merge + `apply_worktree_excludes` on initialize; cache stays in prefix; unknown keys do not fail |
| `root_from_params` | Adapter | `rootUri` / `rootPath` / `workspaceFolders` → workspace path; no `$/` FilesSince |
| `LspStdioDriver` (`plsp-it1`) | Adapter | initialize → shutdown over Content-Length; integration only; no `$/` FilesSince |
| `ServeDiskWatch` | Observer + Adapter | Stock ghost-disk: on-disk bytes change → reindex; no progressive client; no `thread::sleep` in unit tests |
| `CorpusPin` (`integration/corpora/pins.json`) | Value object / Schema | URL + peeled SHA + entry; fetch-at-SHA; never a submodule mirror |
| `ExpectedGolden` | Schema / DTO | 0-based `find` → line/character; integration only |
| `It2BackendDriver` (`plsp-it1 backend`) | Adapter | Stock initialize/didOpen/def/hover/tokens/didChange/ghost; `$/` FilesSince must be method-not-found |
| `It2ReportRow` | DTO | `language`, `corpus_sha`, `pack`, `tier_observed`, `definition_ok`, `tokens_ok`, `ghost_edit_ok`, `notes`; T3 stub → `skip_pack_missing` |
| `Envelope` | DTO / public dispatch | `method` + `request_id` + `body`; replies echo id; pushes use `request_id == 0`; never `$/` |
| `ControlPlane` | Port | Proto RPCs call the composition-root host; control crate does not own config/watch/install internals |
| `ControlServer::dispatch_envelope` | Command | Case-sensitive method names match the API RPC table; unknown method → non-zero `Status` |
| `bind_control_socket` / `spawn_control_accept` | Adapter | Unix socket beside stdio LSP; length-prefixed Envelope; stock serve without `--control-socket` still works |
| `It3ProgressiveDriver` (`plsp-it1 progressive`) | Adapter | LSP stdio + Envelope socket; IT-3.1–3.7; `--mux` is `pending_mux` (do not silently retest socket) |
| `It3ReportRow` | DTO | `backend`, `rpc`, `result`, `notes`; T3 stub → `skip_pack_missing`; mux → `pending_mux` |

## Patterns we do not use (v1)

| Pattern | Why not |
|---|---|
| Plugin `dlopen` | Fights musl-static |
| Singleton global `REGISTRY` without injection | Untestable; use composition root |
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
| `PendingDialog` / `DialogAction` / `DialogOutcome` | Command / value | File-menu click records Open Folder / Open File; apply runs the Port after the menu closes; cancel is `Cancelled`, not an error |
| `FakeDialog` | Test double | Same `DialogPort`; returns queued paths |
| `WorkspaceRoot` | Value object / identity | Canonical absolute path; equality is path equality |
| `FsPort` / `StdFs` | Port / Adapter | Tree/read/write go through the Port; tests use `MemFs` |
| `MemFs` | Test double | Same `FsPort`; no host disk |
| `CountingFs` | Decorator / test double | Wraps `MemFs`; records `read_dir` paths; inner Port is unchanged; used to prove shallow load / idempotent expand |
| `FileTree` / `TreeNode` | Composite | Directories contain children; files are leaves; skip `.git`/`target`/`node_modules` display filter. `load` is shallow (immediate children only); child dirs start unloaded (`children: None`); `expand` / `load_children` fills one level (`Some(vec![])` is an empty loaded folder). `load_compact_chain` loads a single-child-dir chain for a compact row without changing `TreeExpansion`. Listing order is non-dot dirs, non-dot files, dot dirs, dot files (lexicographic within each group). |
| `CompactChain` | Value object / view of Composite | `/`-joined names of already-loaded single-child directories; `path` is the innermost directory. Unloaded / empty / one file child / 2+ children stop the chain. Length 1 is a non-compact directory. Skip-filtered names cannot be the "one child." |
| `TreeExpansion` | Value object / collection | A path is expanded iff explicitly expanded; default is collapsed at every level. `for_root` / a new `FileTree` starts empty. `expand` / `collapse` are Commands. Collapse of a missing path is a no-op. Expanding a file is a no-op. Expanding a parent does not expand children. Expanding a compact row expands the innermost path only — nested names in the chain are not auto-expanded. |
| `LayoutState` | Value object | `left_width` > 0; clamp on set; no window handle in the lib |
| `TabStrip` / `TabId` | Identity + collection | Focus is at most one tab; close missing id is a no-op |
| `OpenBuffer` / `BufferMap` | Entity + Identity | One buffer per canonical path; rope is source of truth |
| `Selection` | Value object | Range is ordered `start <= end` in char offsets |
| `CursorOffsets` | Value object | Editor char offsets → `Selection`; apply writes the caret onto `OpenBuffer` without dirtying; offsets → `position_at` is not always line 0 character 0 |
| `DirtyFlag` | Value object | Edit sets dirty; successful save clears it |
| `EditCommand` | Command | Insert/delete/cut/copy/paste mutate rope only via this Command |
| `DiscoverKind` | Value object | Definition / Implementation / References; `lsp_method` is the stock JSON-RPC name |
| `DiscoverCommand` | Command | Focused tab + cursor → `LspClient` method + `jump`; no file open / missing client are domain errors, not panics; empty location list is valid |
| `PendingDiscover` | Command / value | Click records a `DiscoverKind`; apply runs `DiscoverCommand` once after the menu closes; close does not panic |
| `ClipboardPort` / `FakeClipboard` | Port / Adapter + test double | Cut/copy/paste never call OS clipboard in tests |
| `Highlighter` | Adapter | syntect tokens; unknown syntax → empty/plain spans, no panic |
| `HighlightSpan` | Value object / DTO | Char range `start <= end`; RGB from syntect; unknown syntax yields empty list |
| `WatchPort` / `NotifyWatch` | Port / Adapter | Prod uses `notify`; coalescer/IDE does not call OS APIs directly |
| `WatchDepth` | Value object | `immediate` vs `recursive`; folder open uses immediate so a large tree does not block on a recursive OS watch |
| `LspSessionState` | Value object | `idle` / `connecting` / `ready` / `failed`; connecting is not ready; tree paint must not wait for `ready` |
| `FakeWatch` | Test double | Same `WatchPort`; tests inject events; no `thread::sleep` |
| `DiskEvent` / `DiskEventKind` | Event / DTO | path + kind + mtime; `KeepMemory` ignores a later event with the same mtime |
| `ClockPort` / `FakeClock` (poc-ide) | Port / test double | Tests never `thread::sleep`; advance with FakeClock |
| `SystemClock` (poc-ide) | ClockPort production | Wall `unix_ms`; tests use `FakeClock` |
| `DiskWatch` | Observer | Watch events for an open path enqueue at most one pending `ConflictModal` per path |
| `ConflictModal` / `ConflictChoice` | Command | `LoadDisk` replaces rope from `FsPort` and clears dirty; `KeepMemory` keeps rope and records `ignored_mtime` |
| `LanguageCatalog` | Registry | Extension lookup is deterministic; unknown → `plaintext`; plaintext skips `didOpen` |
| `ServeMode` | Strategy | `StockStdio` vs `ControlSocket`; `serve_args` never includes `--mux` (`pending_mux`) |
| `LspTransport` / `StdioLsp` | Port / Adapter | Content-Length JSON-RPC; lib does not parse via `egui` |
| `LspCall` | DTO | Recorded request or notification on `FakeLsp`; method is the JSON-RPC name |
| `FakeLsp` | Test double | Same `LspTransport`; scripted responses; missing binary is a Result |
| `LspClient` | Facade | JSON-RPC in; domain locations out; no watch internals |
| `ProgressiveLspCap` (poc-ide) | Value object / DTO | version is `v1`; socket may be null; `LspClient` never opens it; `ControlClient` does in `ControlSocket` |
| `ControlTransport` / `UnixControl` | Port / Adapter | Envelope + `u32be` frames; payload > 16 MiB fails |
| `FakeControl` | Test double | Same `ControlTransport`; pushes use `request_id == 0` |
| `ControlClient` | Adapter | Unary RPCs + push dispatch; never `$/` FilesSince |
| `ControlPush` | Event / DTO | `WatchBatch` or `TierReady`; `request_id` is always 0 |
| `ProtocolConsole` / `TranscriptEntry` | Facade + DTO | Append-only transcript; send does not panic on server error |
| `TranscriptKind` | Value object | Lsp vs Control vs error; `is_push` only for `ControlPush` with `request_id == 0` |
| `IdeError::Control` | Domain Result | missing socket / payload too large / `pending_mux`; stock LSP remains |
| `LspLocation` (poc-ide) | Value object / DTO | uri + range from the client; jump opens or focuses a tab; empty list is valid |
| `file_uri` | Adapter | Absolute path → `file:` URI with percent-encoding; spaces and other reserved bytes are `%XX` |
| `SpawnSpec` | Value object | Binary from env, then `target/…/progressive-lsp`, then `PATH`; missing → error not panic |
| `RunLog` | Repository | One sqlite file (or `:memory:`) per run; append + query; write failure is `IdeError::Log`, never a panic. Discover rows include `path`, `uri`, `line`, `character`, `location_count` |
| `RunLogPath` | Value object | `{dir}/poc-ide-{unix_ms}-{pid}.sqlite`; tests inject dir / path |
| `LogRow` | DTO | `timestamp_ms` + `category` + `event` + optional JSON; payload is structured, never file bodies |
| `LogCategory` | Value object | `run` / `ui` / `tree` / `tab` / `buffer` / `lsp` / `control` / `conflict`; unknown parse → `None` |
| `IdeError::Log` | Domain Result | Classifier `is_log`; composition root ignores write failures |

## Adding a type

1. Name the pattern in this table (PR must update the table).
2. Put the invariant in a unit test next to the type.
3. If you cannot name the pattern, the type should not exist yet.
