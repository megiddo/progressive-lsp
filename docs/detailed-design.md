# Detailed design

Types, traits, merge rules, errors. Patterns: [design-patterns.md](design-patterns.md). Proto: [control-protocol.md](control-protocol.md). Plugins: [plugin-sdk.md](plugin-sdk.md). Logging: [logging.md](logging.md).

Names below are the intended Rust surface. Implement them rather than inventing parallel layers.

## Composition root

The `progressive-lsp` bin crate is the **composition root**. It:

1. Builds `ClockPort` (real vs test).
2. Starts `MemoryLog` (ring, cap 4096) so prefix/config failures are not silent.
3. Resolves prefix (`PROGRESSIVE_LSP_HOME`, `--prefix`, else `$HOME/.progressivelsp`); `ensure_dirs` (creates `log/`).
4. `LogOpenPlan`: opens `SqliteLogRepository` on primary `ServeLogPath`; on failure retries same-dir fallback then a temp WAL (LOG-9). Only if all three fail: keep `MemoryLog`.
5. Replays the ring into whichever WAL opened (best-effort).
6. Installs `LogCrateBridge` / `TracingBridge`.
7. Calls `register_builtins()` / `inventory` into `PluginRegistry`.
8. Loads config merge chain into `Config`; `ConfigWarnAdapter` emits warnings.
9. Constructs `ScriptHost`, `WatchCoalescer`, `IndexService`, `EngineSupervisor` (libs take `Arc<dyn LogPort>`). Serve **holds** the supervisor (LOG-6).
10. Starts `LspFacade` on stdio; optionally `ControlServer`.
11. On shutdown: `Flush` + join the writer.

No other crate `new()`s the whole graph. Process-wide `OnceLock<LogPort>` is forbidden. The bin is the only place that constructs the sqlite Adapter. Spec: [logging.md](logging.md). `LogPort::emit` returns `()` (never `Result`). `LogRecord` construction never fails. Tests inject `FakeLog` / `MemoryLog` / `NullLog`. Production uses `NeverFailLog` around `SqliteLogRepository`.

## Core types (`progressive-lsp-core`)

```rust
pub struct LanguageId(/* interned, e.g. "java" */);
pub struct PackageId(/* interned */);
pub struct FileId(/* interned */);

pub enum Tier { Syntax, Graph, Types } // T1 T2 T3

pub struct LanguageVersion {
    pub language: LanguageId,
    pub effective: semver::Version, // min(window, grammar, engine)
    pub window_latest: semver::Version,
    pub grammar_pin: String,
    pub engine_pin: Option<String>,
}

pub trait ClockPort: Send + Sync {
    fn now(&self) -> std::time::Instant;
    fn unix_ms(&self) -> u64;
}

pub struct PrefixLayout { /* bin, engines, cache, log, run, scripts */ }

pub trait LogPort: Send + Sync {
    fn emit(&self, record: LogRecord);
}
```

Tests inject `FakeClock` (same trait). Production uses the wall clock. Never `thread::sleep` in tests. `LogPort` is the same injection rule: tests use `FakeLog` / `MemoryLog` / `NullLog`; never open `$HOME`.

`WorkspaceId` is a hash of the canonical absolute workspace path (stable across reconnects).

## Errors

Use typed errors with `thiserror`. Do not `unwrap` on user paths.

| Type | When |
|---|---|
| `UnsupportedLanguage` | Factory slot empty or feature off |
| `EngineNotReady` | T3 requested/implied but child not ready; resolver falls back |
| `InstallError::Hash` | SHA mismatch; do not exec |
| `InstallError::Transport` | `ArtifactTransport` failed |
| `InstallError::Refused` | `on_install_verify` Abort; tmp deleted, no rename |
| `StaticLinkError` | `check-static` failed (xtask, not runtime) |
| `ScriptAbort` | Hook returned Abort; documented skip |
| `ScriptSandbox` | ops/string cap exceeded |
| `ConfigError` | TOML invalid or unknown key policy |
| `WatchOverflow` | generation gap; caller must `FilesSince` |
| `InitializeFailed` | `on_bootstrap` Abort |

## Config merge

Files (later wins on duplicate keys that the overlay **sets**):

1. `$PREFIX/config.toml` (user global)
2. `$PREFIX/workspaces/<workspace-id>/config.toml`
3. `<workspace>/.progressivelsp/config.toml`

`initialize` `initializationOptions` may override a documented subset (scripts list, pack list, prefix). Overlay still wins over home for on-disk keys.

Unknown keys: ignore with a log at warn (forward compatible) via `ConfigWarnAdapter`. Required keys: none for boot (empty config is valid). `[log].level` / `[log].path` merge on this same chain; invalid `level` → warn + default `info`. `PROGRESSIVE_LSP_LOG` env overrides `path` for the process.

## LSP Facade (`progressive-lsp-protocol`)

- JSON-RPC 2.0 over stdio (or mux `lsp` channel).
- `initialize` result includes `capabilities.experimental.progressiveLsp`:

```json
{
  "version": "v1",
  "socket": "/home/u/.progressivelsp/run/control.sock",
  "mux": false
}
```

`socket` may be null if control is off. Stock clients ignore experimental caps.

`textDocument/definition` (and siblings) go through `Resolver::resolve`. Response locations may set `data: { "tier": "syntax"|"graph"|"types" }`.

## Resolver (`progressive-lsp-resolve`)

```rust
pub struct ResolveQuery { pub file: FileId, pub position: Position, pub kind: QueryKind }
pub enum QueryKind { Definition, References, TypeDefinition, Implementation, Hover, DocumentSymbol, WorkspaceSymbol }

pub trait Resolver: Send + Sync {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult;
}

pub struct ResolveResult {
    pub locations: Vec<LspLocation>,
    pub tier: Tier,
}
```

**Chain of Responsibility** (order fixed):

1. If `EngineSupervisor` reports ready for `(language, package)` → T3 adapter.
2. Else `HeuristicResolver` / optional `StackGraphResolver` (Strategy pick).
3. Else `TreeSitterResolver`.

Scripts cannot insert a step. Tests: a `FakeResolver` at T2 must not be skipped when T3 is `NotReady`.

## Index (`progressive-lsp-index`)

`IndexService` is a **Facade** over the types below. It is not a god object; LSP and control call into it, they do not own watches or engines.

- `DirtySet`: paths + generation.
- `PriorityIndex`: open > recent > same package > other > vendor.
- Per-file: generation, content hash, grammar version.
- `IndexCache` (Repository): key = `(grammar_ver, language_id, file_hash)` under `$PREFIX/cache/`. Disk persist is the same type (no CacheManager). Tests inject `PrefixLayout`. Never write cache into the git worktree.
- Progressive ingest: package completion emits `workDoneProgress` and optional `TierReady`.

## Watch (`progressive-lsp-watch`)

```rust
pub trait WatchBackend { /* start, stop; tests use FakeWatcher */ }
pub trait WatchFilter {
    fn filter(&self, batch: WatchBatch) -> WatchBatch; // drop paths
}
```

Coalesce window: `WatchCoalescer` uses `ClockPort`. Overflow sets `need_rescan` / bumps generation. `FilesSince { since_generation | since_unix_ms }` returns a bounded path list + `truncated`.

## Workspace (`progressive-lsp-workspace`)

```rust
pub trait WorkspaceSource {
    fn detect(&self, root: &Path) -> Option<WorkspaceModel>;
}
```

`WorkspaceModel`: packages, source roots, classpath-like entries that **exist on disk**. `on_workspace_discover` may add/deny roots; it cannot invent jars that were not found.

## Engine (`progressive-lsp-engine`)

```rust
pub trait EngineAdapter {
    fn pack_name(&self) -> &str;
    fn discover(&self, prefix: &PrefixLayout) -> Option<EngineBinary>;
    fn spawn(&self, ctx: SpawnCtx) -> Result<ChildHandle, EngineError>;
    fn ready_signal(&self) -> ReadyKind; // initialize result, or "indexed package X"
}
```

`EngineSupervisor`: spawn, stdio proxy, crash/backoff, capability merge, forward didChange/watches. `on_engine_spawn` may tweak allowlisted argv/cwd/env or Abort (skip engine, stay T2).

## Install (`progressive-lsp-install`)

```rust
pub trait ArtifactTransport {
    fn put(&self, dest: &Path, bytes: &[u8]) -> Result<(), InstallError>;
    fn chmod_exec(&self, path: &Path) -> Result<(), InstallError>;
    fn rename_atomic(&self, from: &Path, to: &Path) -> Result<(), InstallError>;
    fn read_hash(&self, path: &Path) -> Result<[u8; 32], InstallError>;
    fn probe(&self) -> Result<HostProbe, InstallError>;
}

pub struct HostProbe { pub arch, pub os, pub libc_hint, pub existing_hashes, pub census: BuildCensus }

pub trait PackSelector {
    fn select(&self, probe: &HostProbe) -> Vec<PackId>;
}
```

`Installer::plan` / `apply`: write `.tmp-*`, chmod, rename, re-hash. Mismatch → `InstallError::Hash`, no exec. **LocalFs** ships here. URL fetch **off by default**. SSH is not implemented here.

**Census → packs (auto):** `Cargo.toml` → rust-analyzer; `compile_commands.json`/`CMakeLists.txt` → clangd; `pyproject.toml` → ty; `*.csproj` → csharp-ls; `tsconfig.json`/`package.json` → tsgo; `composer.json` → PHP T3; `go.mod`/`go.work` → gopls; `build.zig` → zls. Java → no T3 pack.

## Script host (`progressive-lsp-script`)

Rhai, ops limit, string cap, no I/O unless `allow_shell` **and** the hook opted in. `now()` from `ClockPort`. Abort semantics per hook in [plugin-sdk.md](plugin-sdk.md). Tests: Abort prevents the side effect; scripts cannot `register_method("textDocument/definition")`.

## Language crates

Each `progressive-lsp-lang-*` exports `LanguageFactory`. Unshipped T3: Factory still parses (T1) and may resolve T2; type queries return empty or T2, never panic. Empty M0 slots: `UnsupportedLanguage` without panic.

## Mux (optional)

`--mux`: one stdio stream, two channels. Frame: `u8 channel | u32be length | payload` (16 MiB cap). Channel `0` (`lsp`) = opaque JSON-RPC bytes. Channel `1` (`control`) = length-prefixed protobuf as in [control-protocol.md](control-protocol.md). Types: `MuxFrame`, `CHANNEL_LSP`, `CHANNEL_CONTROL`.

`DistManifest` / `DistArtifact`: dist `manifest.json`. `core_version` is crate semver (0.1.0). Engine SHAs stay on pack `Manifest` artifacts. Darwin `payload_kind` is `stub`. Real musl ELFs are Linux CI per-triple tarballs.
