# Integration seams — request limits, response meta, trace API

**Status:** In progress (SEAMS-1…3 landed on `integ-seams-*`; IT-TAM harness follows).  
**Driver:** Tier API Matrix needs deterministic tier caps, uniform observability on every API, and correlated deep traces — not WAL scraping alone.

**Related:** [README.md](README.md), [progressive-lsp-apis.md](../../docs/progressive-lsp-apis.md), [lsp-contract.md](../../docs/lsp-contract.md), [control-protocol.md](../../docs/control-protocol.md), [ADR 002](../../docs/adr/002-serve-readiness-fsm.md).

---

## Goals

1. **Request seams** — cap resolver chain behavior for validation (`maxChainIter`, `maxTier`).
2. **Optional response envelope** — same shape for **all** LSP intelligence + control unary RPCs when enabled: tier, backend language, backend version, internal timing.
3. **Meta-trace control RPC** — given `trace_id` from a response, fetch the serve log slice for that operation (real-time debug; UX TBD).

Stock clients stay unchanged when progressive options are off.

---

## 1. Request parameters (`maxChainIter`, `maxTier`)

### Semantics

| Parameter | Scope | Meaning |
|-----------|--------|---------|
| **`maxChainIter`** | Per request (optional) | Maximum **chain steps attempted** (T3′ = 1, T2 = 2, T1 = 3 in today’s prepend order). Stops after N steps even if all returned `NotReady`. Validation / IT-TAM use `1` = types step only, `2` = types+graph, etc. |
| **`maxTier`** | Per request (optional) | **Ceiling** on tier quality: never return a result whose winning tier is **above** ceiling (`syntax` < `graph` < `types`). Implemented by skipping chain steps above ceiling **and** clamping `Location.data.tier`. |

Both may appear in:

- `initializationOptions.progressiveLsp` (session defaults), and
- **`progressiveLsp` on each intelligence request** (overrides for one call).

Integration / test profile may also read env `PROGRESSIVE_LSP_TEST_MAX_CHAIN_ITER` / `PROGRESSIVE_LSP_TEST_MAX_TIER` when request fields absent (test-only; same pattern as yolo workspace — not for production editors unless explicitly set).

### Internal wiring

- Extend `ResolveQuery` (or session wrapper) with `chain_policy: ChainPolicy { max_iter: Option<u8>, max_tier: Option<Tier> }`.
- `ResolverChain::resolve` in `progressive-lsp-resolve` counts steps; returns `NotReady` terminal if budget exhausted without `Ready`.
- Unit tests: fake chain of 3 steps; `maxChainIter: 1` never invokes T2/T1; `maxTier: graph` never selects types step even when ready.

---

## 2. Optional response meta (all APIs)

### Enablement

Client advertises at initialize:

```json
"initializationOptions": {
  "progressiveLsp": {
    "emitResultMeta": true,
    "emitTiming": true
  }
}
```

Server echoes support in capabilities:

```json
"experimental": {
  "progressiveLsp": {
    "version": "v1",
    "extendedResults": true,
    "resultMetaFields": ["tier", "backendLanguage", "backendVersion", "timing", "traceId"]
  }
}
```

Per-request override: `"progressiveLsp": { "emitResultMeta": false }` on a single RPC params object.

### Wire shape — LSP intelligence

When `emitResultMeta` is true, **wrap** the normal LSP result (stock shape unchanged in the `value` field):

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "value": [ { "uri": "...", "range": {}, "data": { "tier": "graph" } } ],
    "progressiveMeta": {
      "traceId": "7c4e2f1a-...",
      "tier": "graph",
      "backendLanguage": "java",
      "backendVersion": "javacs@sha256:abc…",
      "timing": {
        "resolve_ms": 8,
        "chain_steps": 2,
        "cache_state": "miss"
      }
    }
  }
}
```

| Method | `value` holds |
|--------|----------------|
| definition / references / implementation / typeDefinition | `Location[]` |
| hover | `Hover` |
| documentSymbol | `DocumentSymbol[]` |
| workspace/symbol | `SymbolInformation[]` |
| semanticTokens/full | `{ data, resultId? }` |

When `emitResultMeta` is **false**, `result` is the **legacy** bare LSP value (array or object) — stock clients and old progressive clients.

**`progressiveMeta` fields (normative when enabled):**

| Field | Always when meta on? | Source |
|-------|----------------------|--------|
| `traceId` | yes | UUID v4 per operation; keys log ring buffer |
| `tier` | yes | Winning `ResolveResult.tier` or equivalent for non-chain paths |
| `backendLanguage` | yes | `language_id` for file / package (e.g. `java`) |
| `backendVersion` | when T3 or engine touched | Pack id + content hash from `EngineSupervisor` / builder; T1/T2: `"tree-sitter"` / `"heuristic-graph"` + crate version |
| `timing.resolve_ms` | when `emitTiming` | Mux wall for handler |
| `timing.chain_steps` | discover + chain | Steps attempted |
| `timing.cache_state` | T3′ participated | Same enum as WAL |

Extend **`Location.data`** when meta enabled: always set `tier`; optional `backendLanguage` per hit if cross-language (future).

Hover / symbols / tokens: today lack `data.tier` — **`progressiveMeta.tier`** is the authoritative tier for IT-TAM when meta is on.

### Wire shape — control unary RPCs

Same pattern: protobuf responses gain optional **`ProgressiveMeta`** message (field reserved high number, additive):

```protobuf
message ProgressiveMeta {
  string trace_id = 1;
  string tier = 2;              // workspace-level or "n/a"
  string backend_language = 3;
  string backend_version = 4;
  Timing timing = 5;
}
message Timing {
  uint64 handler_ms = 1;
}
```

Attach to `IndexStatusResponse`, `GetConfigResponse`, etc., when client sent `emit_result_meta` on the request wrapper (new optional fields on requests).

---

## 3. Meta-trace API (control)

**RPC:** `FetchTrace` (name TBD; avoid collision with LSP `$/logTrace`).

```protobuf
message FetchTraceRequest {
  string trace_id = 1;
  uint32 max_rows = 2;   // default 500
}
message FetchTraceResponse {
  Status status = 1;
  repeated TraceRow rows = 2;
}
message TraceRow {
  uint64 unix_ms = 1;
  string level = 2;
  string component = 3;
  string operation = 4;
  string message = 5;
  map<string, string> extras = 6;
}
```

- **Population:** On each operation with a `traceId`, serve appends `LogRecord` copies (or references) into an in-memory **ring buffer** keyed by `trace_id` (TTL e.g. 10 min / 10k ids — document in ADR).
- **Client flow:** IT-TAM / POC IDE reads `progressiveMeta.traceId` → `FetchTrace` on mux channel 1 → attach to report JSON or show in debug UI later.
- **Security:** Same trust as control socket (local Unix socket / mux); no trace bodies in LSP when meta off.

---

## 4. Unit tests (mandatory with implementation)

| Area | Tests |
|------|--------|
| `progressive-lsp-resolve` / chain | `maxChainIter` stops early; `maxTier` skips types step; combined |
| `progressive-lsp-protocol` | `result_to_lsp` wrap vs bare; hover/symbols get meta wrapper |
| `session` / `serve_host` | `traceId` unique; WAL extras match `progressiveMeta.timing` |
| `progressive-lsp-control` | encode/decode `FetchTrace`; empty id → not found |
| Golden JSON | Snapshot tests for one definition + one IndexStatus with meta on |

Existing tests **must keep passing** with default options (meta off, no chain policy).

---

## 5. IT-TAM integration

Update [suites/java-junit4.tam.yaml](suites/java-junit4.tam.yaml) and [README.md](README.md):

```yaml
session:
  progressive_lsp:
    emitResultMeta: true
    emitTiming: true

cases:
  - apis:
      - method: textDocument/definition
        progressive_lsp:
          maxChainIter: 2
          maxTier: graph
        expect:
          progressive_meta:
            tier_at_most: graph
            backend_language: java
          timing:
            max_resolve_ms: 50
        trace:
          fetch: true   # harness calls FetchTrace; store rows in report
```

Report columns: `trace_id`, `meta_tier`, `backend_version`, `resolve_ms`, `trace_row_count`.

**Phase coupling:**

| Phase | Deliverable |
|-------|-------------|
| **SEAMS-1** | `ChainPolicy`, chain unit tests, session applies defaults |
| **SEAMS-2** | LSP extended results + `ProgressiveLspCap.extendedResults` |
| **SEAMS-3** | Control `ProgressiveMeta` + `FetchTrace` + ring buffer |
| **SEAMS-4** | Docs: lsp-contract, control-protocol, progressive-lsp-apis, ADR 004 |
| **SEAMS-5** | IT-TAM TAM-2 runner requires meta + uses `maxChainIter` / `maxTier` |
| **SEAMS-6** | Java matrix green in container; trace dump on failure |

**Order:** SEAMS-1…3 before or in parallel with TAM-2; TAM-3 Java POC depends on SEAMS-5.

---

## 6. ADR / docs checklist

- [x] [ADR 004](../../docs/adr/004-progressive-result-meta-and-trace.md) — extended results, stock compatibility, ring buffer TTL
- [x] [lsp-contract.md](../../docs/lsp-contract.md) — `value` + `progressiveMeta` wrapper
- [x] [control-protocol.md](../../docs/control-protocol.md) — `FetchTrace`, request meta flags
- [x] [progressive-lsp-apis.md](../../docs/progressive-lsp-apis.md) — catalog entries
- [x] [design-patterns.md](../../docs/design-patterns.md) — `ChainPolicy`, `ProgressiveMeta`, `TraceRing`

---

## 7. Non-goals (this program)

- Streaming trace to clients without `FetchTrace`
- Meta on stock clients that never set `emitResultMeta`
- Persisting trace ring across process restart
- HTTP transport
