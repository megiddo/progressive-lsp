# SERVE-ABS — design

Architecture for readiness FSM, file event hub, and tier registry. **Normative behavior:** [requirements.md](requirements.md). **ADRs:** [002](../adr/002-serve-readiness-fsm.md), [003](../adr/003-file-event-hub.md). **Concept:** [n-tier-framework.md](../architecture/n-tier-framework.md).

---

## Composition root

```text
ServeHost
  ├── WorkspaceSession     (packages, ingest, document sync)
  ├── EngineSupervisor     (spawn, warm, forward — worker threads only)
  ├── TypesCacheStack      (T3′ read + TypesCacheBuilder)
  ├── DiscoverChain        (T3′ → T2 → T1 steps)
  ├── ServeReadiness       (NEW ABS-1: unified FSM + observers)
  ├── FileEventHub         (NEW ABS-2: coalesce + pub/sub)
  └── ControlService       (IndexStatus, pushes, journal)
```

**Rule:** Mux threads call **`resolve_step`** on the chain only; they **read** `ServeReadiness` and cache stores — they **do not** block on engine RPC.

---

## ServeReadiness (ABS-1)

Central type (exact module TBD: likely `progressive-lsp` session or small `progressive-lsp-readiness` module) holding:

| Sub-FSM | States (sketch) | Updated by |
|---------|------------------|------------|
| Workspace | `not_started` → `running` → `done` | ingest worker |
| Package | `syntax` → `graph` → `types` | ingest, engine ready |
| Engine | `missing` \| `pending` \| `warming` \| `ready` \| `error` | supervisor |
| Document | `unknown` \| `indexed` \| `lsp_open` \| `dirty(gen)` | index, didOpen/Close, hub |
| Query key (T3′) | `absent` \| `inflight` \| `ready(*)` \| `stale` | builder, invalidation |

**Observers:**

- Discover chain: query key + package tier → **`NotReady`** vs cache read.
- Control: serialize subset into `IndexStatus` / pushes.
- Future ABS-3: one row per **tier id** in capabilities RPC.

**Anti-patterns removed (ABS-1.2):** `session.try_engine_discover` on mux; mux `supervisor.resolve`.

---

## Discover chain interaction

```text
mux request
  → for step in [Types(T3′), Graph(T2), Syntax(T1)]:
       if !liveness_budget: NotReady
       if step needs FSM pending: NotReady
       if Ready(locations): return Ready
       if Ready([]) && authoritative_empty: return Ready([])
  → terminal NotReady (internal; LSP mapping per ADR 002)
```

T3′ **`inflight` map** (ABS-1.3): on miss, builder enqueues; key → `inflight` until `put`; mux sees **`NotReady`**, not empty ready.

---

## Control proto additions (ABS-1 / ABS-3)

**Additive only** in `proto/progressive/v1/control.proto`:

| Message | Phase | Purpose |
|---------|-------|---------|
| **`CacheReady`** push | ABS-1 | T3′ key left inflight; optional `query_kind`, key hash, `location_count` |
| **`TierCapabilities`** or extended **`IndexStatus`** | ABS-3 | `TierDescriptor` list + per-tier FSM snapshot |

Document wire shapes in [progressive-lsp-apis.md](../progressive-lsp-apis.md) and [user/progressive-v1-api.md](../user/progressive-v1-api.md) on sign-off.

---

## FileEventHub (ABS-2)

**Crate home:** `progressive-lsp-watch` (or adjacent) — reuses `WatchBackend`, `WatchCoalescer`.

```text
  OS notify / FakeWatcher (tests)
           │
           ▼
    [Hub thread]
      coalesce by path
      bump workspace file generation
           │
     ┌─────┼─────┬─────────────┐
     ▼     ▼     ▼             ▼
  Index  T3′    EngineFwd    Control
  sub    inv   supervisor   journal → WatchBatch push
```

| API (sketch) | Behavior |
|--------------|----------|
| `Hub::start(backend)` | Spawns thread; owns single `WatchBackend` |
| `Hub::publish_buffer_edit(uri, gen)` | From LSP didChange path |
| `Hub::subscribe(id, filter, handler)` | Sync or channel delivery per subscriber |
| `Hub::stop()` | Join; flush pending batch |

**Ordering:** Events for the same path in one batch share one generation increment; cross-path order preserved as coalescer output order.

**Fallback:** `poll_disk_watch` sets `need_rescan` when notify queue overflows or hub stopped — full tree walk not on hot path.

---

## TierRegistry (ABS-3)

Static table at startup (dynamic updates limited to ABS-4.3):

| Tier id | Read step | Worker ingest | Worker edit | Latency class | Quality class |
|---------|-----------|---------------|-------------|---------------|---------------|
| `syntax` | T1 | package ingest | hub → index | low | syntax |
| `graph` | T2 | post-ingest graph | hub → graph | medium | heuristic refs |
| `types` | T3′ | builder + engine | hub → invalidate | high | engine-backed |

**Sort key:** `(latency_class, quality_class)` — matches chain walk order today.

---

## TierPort (ABS-4)

```rust
// Sketch — not final API
trait TierPort {
    fn tier_id(&self) -> TierId;
    fn readiness(&self) -> TierReadinessSnapshot;
    fn resolve_step(&self, ctx: &DiscoverCtx) -> StepOutcome; // Ready | NotReady
    fn on_ingest(&self, event: IngestEvent);
    fn on_edit_event(&self, batch: &FileEventBatch);
}
```

Chain becomes `Vec<Box<dyn TierPort>>` with optional filter when engine `missing`.

---

## Crate touch map (expected)

| Phase | Crates / areas |
|-------|----------------|
| ABS-1 | `progressive-lsp`, `progressive-lsp-types-cache`, `proto`, control push, session |
| ABS-2 | `progressive-lsp-watch`, `ServeHost` init, index, supervisor forward |
| ABS-3 | control RPC handlers, docs |
| ABS-4 | discover chain module, types-cache ports (no lang-* split) |

---

## Reconciliation with dogfood branch

Temporary **`try_engine_discover`** on mux violates **FR-ABS-1.2**; remove in **ABS-1.2**, replacing integration coverage with **NotReady + builder** assertions and control **`CacheReady`** waits (no serve-side sleep loops for product behavior).
