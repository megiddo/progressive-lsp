# Plugin SDK

Extension surface for first-party language crates and for operators who compile this binary or drop Rhai files.

**v1 loading:** link-time (`inventory` or `register_builtins()` in the bin) + scripts listed in config / `initializeOptions.scripts`. **No `dlopen`.** Feature flags: `--features lang-php`. WASM is post-v1 (same trait names).

Related: [design-patterns.md](design-patterns.md), [detailed-design.md](detailed-design.md).

## What must not be a plugin

- Go-to-definition / references **implementation**. Scripts may filter watches or hint roots; they may not replace the `Resolver` chain.
- SSH, git, PTY, file-tree CRUD.
- IDE hooks: `on_pre_save`, `on_action`, `on_key`.

## Traits (compiled-in)

### `LanguageFactory`

Produces grammar id, `LanguageId`, and the resolver chain (T1 required; T2/T3 optional). Empty slot → `UnsupportedLanguage`.

M0: `register_builtins()` is a no-op. `KNOWN_LANGUAGE_SLOTS` lists the v1 languages; `PluginRegistry::get` returns `UnsupportedLanguage` for each without panic. No `dlopen`. No process-global registry — the bin constructs one `PluginRegistry` and injects it.

### `WorkspaceSource`

Maps a workspace root to `WorkspaceModel`. First-party adapters live in this repo because intelligence owns project shape.

### `EngineAdapter`

Discover binary under `$PREFIX/engines/`, spawn, ready-signal, stdio. Supervisor owns lifecycle.

### `WatchFilter`

Runs on a coalesced batch **before** `DirtySet`. Identity is valid. Rhai `on_watch` is one implementation behind this trait.

### `ScriptEngineFactory`

Tests inject a fake engine; production uses Rhai. Do not hard-code Rhai in watch tests.

### `ArtifactTransport` / `PackSelector`

Install crate. Not “language plugins,” but the same Factory/Strategy rules. Consumers implement `ArtifactTransport`; they do not fork resolvers.

## Rhai hook catalog (v1)

Scripts live on the `.progressivelsp` merge chain. Sandbox: ops limit, string cap, no I/O unless `allow_shell` **and** the hook opted in. `now()` from `ClockPort`.

| Hook | When | Abort means |
|---|---|---|
| `on_bootstrap` | After `initialize`, before first index | Fail initialize with a clear message |
| `on_workspace_discover` | After adapters; may add/deny **source roots** | Skip those roots; **cannot invent classpath jars not on disk** |
| `on_pre_index` | Per package, before index | Skip that package |
| `on_post_index` | Per package, after | Logging only; cannot unwind intelligence |
| `on_watch` | Before coalesced batch hits dirty-set | Drop those paths |
| `on_engine_spawn` | Before spawn | Skip that engine (T2 remains). Allowlisted argv/cwd/env tweaks only — no arbitrary RCE |
| `on_tier_ready` | After package upgrades tier | Cannot Abort intelligence; logging/metrics only |
| `on_install_verify` | After hash check, before first exec of a new binary | Refuse the new binary |

**Explicitly not hooks:** `on_pre_save`, `on_action`, `on_key`, SSH, git, implementing `textDocument/definition`.

zeds-dead keeps its **own** Rhai catalog. Operators who use both write two small hook files in one language.

## Tests that protect the catalog

- Abort prevents the documented side effect (mutation-tested sandbox flags).
- A script cannot register LSP methods.
- `allow_shell` default false; a hook without opt-in cannot exec.
