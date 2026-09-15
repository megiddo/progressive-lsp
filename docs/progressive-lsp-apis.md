# progressive-lsp — public APIs (current)

Catalog of **what clients can call today** when talking to `progressive-lsp serve`.  
**Planned API additions (ABS program):** [architecture/abstraction-targets-plan.md](architecture/abstraction-targets-plan.md) (`CacheReady`, tier capabilities). Internal Rust traits and crates are out of scope; see [detailed-design.md](detailed-design.md).

**Normative detail:** [lsp-contract.md](lsp-contract.md), [control-protocol.md](control-protocol.md), [user/progressive-v1-api.md](user/progressive-v1-api.md), [`proto/progressive/v1/control.proto`](../proto/progressive/v1/control.proto).

---

## Process entry

```text
progressive-lsp serve [--prefix DIR] [--control-socket PATH] [--control-fd N] [--mux]
progressive-lsp install --prefix DIR --packs <names>
```

| Flag / env | Effect |
|------------|--------|
| `--prefix` / `PROGRESSIVE_LSP_HOME` | Install and runtime layout (`config.toml`, `engines/`, logs) |
| `--control-socket PATH` | Unix socket for `progressive.v1` protobuf |
| `--control-fd N` | Parsed; **unused** in v1 (warn logged) |
| `--mux` | Single stdio: channel 0 = LSP JSON-RPC, channel 1 = control protobuf |

---

## Channel A — Stock LSP (JSON-RPC)

**Transport:** Content-Length–framed JSON-RPC on stdio (channel 0 if `--mux`).

### Lifecycle

| Method | Direction | Purpose |
|--------|-----------|---------|
| `initialize` | req/resp | Workspace root, capabilities, `initializationOptions` |
| `initialized` | notification | Client ack |
| `shutdown` | req/resp | Graceful stop |
| `exit` | notification | Process exit |

**Discovery (progressive clients only):** `initialize` result includes:

```json
"experimental": {
  "progressiveLsp": {
    "version": "v1",
    "socket": "<absolute path or null>",
    "mux": false
  }
}
```

Stock clients ignore this block.

### Document sync

| Method | Purpose |
|--------|---------|
| `textDocument/didOpen` | Open buffer; index + forward to T3 packs when attached |
| `textDocument/didChange` | Incremental sync; re-index; forward to engines |
| `textDocument/didClose` | Close buffer in index |

Optional: client `workspace/didChangeWatchedFiles` (server also watches disk).

### Intelligence (resolve chain)

All map to internal `ResolveQuery` → **T3′ → T2 → T1** chain (see [types-cache/design.md](types-cache/design.md)).

| Method | Query kind | Notes |
|--------|------------|--------|
| `textDocument/definition` | Definition | |
| `textDocument/typeDefinition` | TypeDefinition | When capability advertised |
| `textDocument/implementation` | Implementation | When capability advertised |
| `textDocument/references` | References | |
| `textDocument/hover` | Hover | |
| `textDocument/documentSymbol` | DocumentSymbol | |
| `workspace/symbol` | WorkspaceSymbol | |
| `textDocument/semanticTokens/full` | (separate path) | Tree-sitter legend; not resolver chain |

**Response shape:** Standard LSP location arrays / hover / symbols / token data.

**Progressive extension (locations):** Optional `Location.data.tier` = `"syntax"` \| `"graph"` \| `"types"` ([lsp-contract.md](lsp-contract.md)).

**Not implemented (v1):** `workspace/filesSince`, `$/progressive/filesSince`, `textDocument/codeAction` (rejected if sent).

### Capabilities (typical)

Advertised when supported: `definitionProvider`, `referencesProvider`, `hoverProvider`, `documentSymbolProvider`, `workspaceSymbolProvider`, `semanticTokensProvider`, `typeDefinitionProvider`, `implementationProvider`, incremental `textDocumentSync`.

### Progress

Standard LSP `$/progress` / `window/workDoneProgress` during workspace ingest (not custom `$/` progressive methods).

---

## Channel B — Control (`progressive.v1` protobuf)

**Transport:** `u32be length | protobuf` on Unix socket **or** mux channel 1.

**Dispatch:** Every frame wrapped in `Envelope { method, request_id, body }` ([user/progressive-v1-api.md](user/progressive-v1-api.md)).

**Max payload:** 16 MiB.

### Unary RPC (client → server → client)

| Method | Purpose |
|--------|---------|
| `GetConfig` | Merged `config.toml` snapshot |
| `SetConfig` | Patch and persist config |
| `ReloadConfig` | Re-read config from disk |
| `InstallPacks` | Hash-gated pack install under prefix |
| `WatchSubscribe` | Subscribe to watch pushes on this connection |
| `FilesSince` | Catch-up paths since generation or unix ms |
| `IndexStatus` | Packages, generations, **ingest** state, **engines[]**, **cache_entries**, additive **tier_capabilities[]** (ABS-3) |
| `TierStatus` | Per-package current tier (`syntax` / `graph` / `types`) |
| `ReloadScripts` | Reload Rhai script hooks |

All responses include `Status { code, message }` (`code == 0` ok).

### Server pushes (no request)

| Method | Purpose |
|--------|---------|
| `WatchBatch` | Coalesced file events + `overflow`, `need_rescan`, `generation` |
| `TierReady` | Package upgraded to a new tier |
| `CacheReady` | T3′ query left `inflight` → `ready` on builder worker (`file`, `query_kind`, `location_count`) |

---

## Channel C — CLI / install (same binary, separate mode)

| Command | Purpose |
|---------|---------|
| `progressive-lsp install --prefix --packs` | Verify and place engine artifacts (no network in-tree) |

Consumers may use crate `progressive-lsp-install` programmatically ([consumer.md](consumer.md)).

---

## What is *not* a public API

| Item | Notes |
|------|--------|
| Rust `LspIntelligence` trait | In-process; used by `progressive-lsp-protocol` |
| `EngineSupervisor` / pack adapters | Internal |
| `$PREFIX` script hooks (`Rhai`) | Config-driven; not a wire API |
| `./build`, `xtask` | Maintainer tooling |
| POC IDE RunLog sqlite schema | Sample consumer diagnostic; separate from serve WAL |

---

## Readiness vs intelligence

| Concern | API |
|---------|-----|
| “Is ingest done?” | `IndexStatus.ingest`, LSP progress |
| “Is javacs ready?” | `IndexStatus.engines[]` |
| “What tier is package X?” | `TierStatus`, push `TierReady` |
| “How many T3′ entries?” | `IndexStatus.cache_entries` |
| “Go to definition now” | LSP `textDocument/definition` (chain; see ADR 001/002) |

---

## Related docs

- [consumer.md](consumer.md) — stock vs progressive client
- [architecture/n-tier-framework.md](architecture/n-tier-framework.md) — target N-tier model (future refactor)
- [adr/002-serve-readiness-fsm.md](adr/002-serve-readiness-fsm.md) — serve-side FSM
