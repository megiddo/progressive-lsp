# LSP contract

Vanilla [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) over JSON-RPC. This is the **only** requirement for a working editor.

Related: [control-protocol.md](control-protocol.md), [consumer.md](consumer.md), [requirements.md](requirements.md).

## Transport

- Default: **stdio**, Content-Length headers (LSP). Control is **off**.
- `--mux`: one stdio pipe. Frame is `u8 channel | u32be length | payload`. Channel `0` (`lsp`) = opaque JSON-RPC message body (still JSON-RPC, not protobuf). Channel `1` (`control`) = the same length-prefixed proto as a dedicated socket.

## Capabilities (v1 intent)

Advertise what we actually implement for the active languages. Do not advertise T3-only methods as globally available if no engine can ever answer (return empty instead of lying).

**Always (stock):**

- `textDocumentSync` incremental
- `definitionProvider`, `referencesProvider`, `documentSymbolProvider`, `workspaceSymbolProvider`
- `hoverProvider`
- `semanticTokensProvider` (Tree-sitter legend; T3 may overlay)
- `workspace.workspaceFolders` if we support multi-root (document in M1+)
- `experimental.progressiveLsp` (see control protocol) — **harmless for stock clients**

**When the active tier can:**

- `typeDefinitionProvider`, `implementationProvider`

**Progress:** standard `workDoneProgress` / `window/workDoneProgress/create` during ingest. Not a custom `$/` method.

**Locations:** optional `Location.data.tier` = `"syntax"` | `"graph"` | `"types"`.

## Stock vs progressive

| Concern | Stock LSP client | Progressive client |
|---|---|---|
| Intelligence (def/refs/hover/tokens) | stdio LSP | same |
| Open-buffer edits | `didChange` | same |
| Ghost disk edits (vim, scripts) | **server `notify`** | `WatchBatch` preferred; server `notify` off or coalesced if client already watches |
| Client `didChangeWatchedFiles` | optional; we coalesce | optional; do not double-walk |
| Catch-up after overflow / reconnect | server rescan + progress warning | **`FilesSince` on protobuf** |
| Live config / pack install / script reload | edit `.progressivelsp` on disk, restart or wait for file watch of config | control RPCs |
| Tier upgrade notice | `workDoneProgress`; refresh tokens | `TierReady` push |
| FilesSince / WatchBatch / InstallPacks | **not available** (do not implement `$/` shims in v1) | protobuf |

## Why not `$/` for FilesSince

`$/` methods are spec-legal extensions. They are still **nonstandard**. Neovim and VS Code will not implement them for free. Watch storms over JSON-RPC are a poor fit. Stock clients get **server-side watching** instead. A generated JSON-RPC mirror of `progressive.v1` is **post-v1** only if a real LSP-only host cannot open a second channel.

## Initialize options (documented subset)

May include: `prefix`, `packs`, `scripts` (paths or names on the merge chain). Full schema in `config.toml` (M0 stub). Unknown keys ignored with a warning.

## Conformance

Per-language, per-tier pass rates: [conformance.md](conformance.md) (generated from `fixtures/matrix/`). A stock client test harness must pass without opening a control socket. C# is T1/T2 only. Java has no T3. T3 is 0% on Darwin stubs; Linux CI with real musl packs is the place to re-score T3.
