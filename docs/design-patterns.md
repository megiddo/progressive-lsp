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
| `Config`, `ConfigOverlay`, `ConfigLoad` | Chain / Builder | Later overlay wins for keys it sets; empty TOML is valid; unknown keys warn |
| `apply_worktree_excludes` / `GitExcludeReport` | Command | Writes `.git/info/exclude` + overlay belt `.gitignore`; never edits the project’s committed `.gitignore` |
| `ProgressiveLspCap` | Value object / DTO | `version` is `v1`; `socket` may be null; stock clients ignore it |
| `InstallPlan` | Command | `apply` hashes tmp before rename; mismatch deletes tmp |
| `Manifest`, `ManifestArtifact` | Schema / DTO | Relative `rel_path` only; sha256 is 32 bytes |
| `ExplicitPacks`, `CensusSelector` | Strategy (`PackSelector`) | Explicit CSV vs census; Java census selects no T3 pack |
| Frame codec (`encode_frame` / `decode_frame`) | Adapter | `u32be` length; payload > 16 MiB fails; no silent truncate |
| Typed errors (`UnsupportedLanguage`, `EngineNotReady`, `InstallError`, `StaticLinkError`, `ScriptAbort`, `ScriptSandbox`, `ConfigError`, `WatchOverflow`, `InitializeFailed`, `EngineError`) | Domain Result | User paths never `unwrap`; T3 `EngineNotReady` falls back, does not panic |
| `Resolver` chain (`TreeSitterResolver` → `HeuristicResolver` / `StackGraphResolver` → T3 adapter) | Chain of Responsibility | First capable handler wins; T3 `NotReady` does not drop T2 |
| `HeuristicResolver` vs `StackGraphResolver` | Strategy | Same `Resolver` trait; pick is config/eval, not a fork of definition; heuristics are the default |
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
| `JavaLanguageFactory` | Abstract Factory | `language_id` = java; chain is T1 `TreeSitterResolver` |
| `ResolverChain` | Chain of Responsibility | First `Ready` wins; `NotReady` continues |
| `NotReadyResolver` | Test double | T3 skip; must not drop a later T2 `FakeResolver` |
| `DirectoryAdapter` / `MavenAdapter` / `GradleAdapter` / `EclipseAdapter` | Adapter | Detect from files only; no host JDK |
| `WorkspaceSession` | Facade | Composition root wires watch + index + resolve; not a god `LspServer` |
| `LspIntelligence` | Port | JSON-RPC facade calls domain resolve; no watch internals |
| `DirtySet` + `PriorityIndex` | Command queue + Priority | Open buffers before vendor; generation monotonic |
| `IndexCache` | Repository | Same `(grammar, lang, hash)` → skip parse |
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
| Mux demux | Adapter | Opaque LSP bytes vs proto control on one pipe |
| Feature `lang-*` | Product variants | Disabled language → Factory missing, not a stub that panics |
| `PackageIngest` / `IngestReport` | Command | One package per step; `didChange` never waits on remaining packages |
| `WorkDoneProgress` / `ProgressKind` | Event / DTO | Standard LSP `$/progress` begin/report/end; not a `$/` FilesSince shim |
| `GraphFacts` / `ImportDecl` / `TypeEdge` / `CallSite` | Value objects | LanguageIndexer Visitor fills them; resolvers do not parse JSON-RPC |
| `GraphIndex` | Port | Same store as `SymbolIndex`; package tier is Graph only after ingest |
| `StackGraphResolver` | Strategy (unused slot) | Always `NotReady` unless a language binds a winning TSG backend |
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

## Patterns we do not use (v1)

| Pattern | Why not |
|---|---|
| Plugin `dlopen` | Fights musl-static |
| Singleton global `REGISTRY` without injection | Untestable; use composition root |
| God `LspServer` that owns watches, engines, and Rhai | Split Facades + Supervisor |
| Scripts as Strategy for `textDocument/definition` | Forbidden; tests assert |
| Ad-hoc manager / helper / util crates | Missing pattern; add a row here instead |

## Adding a type

1. Name the pattern in this table (PR must update the table).
2. Put the invariant in a unit test next to the type.
3. If you cannot name the pattern, the type should not exist yet.
