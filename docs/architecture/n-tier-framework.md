# N-tier intelligence framework (target)

**Status:** Design direction (2026-09). **Implementation plan:** [serve-abs/README.md](../serve-abs/README.md) + [serve-abs/implementation-checklist.md](../serve-abs/implementation-checklist.md) (master: [abstraction-targets-plan.md](abstraction-targets-plan.md)).

Related: [ADR 001](../adr/001-types-cache-overlay.md), [ADR 002](../adr/002-serve-readiness-fsm.md), [ADR 003](../adr/003-file-event-hub.md), [types-cache/design.md](../types-cache/design.md), [progressive-lsp-apis.md](../progressive-lsp-apis.md).

---

## Generic tier shape

Each tier **i** in an ordered list is a **facade over the same public query API** (LSP discover methods today). Internally every tier implements the same four-part structure:

| # | Component | Role |
|---|-----------|------|
| 1 | **FSM readiness** | Published states (`pending`, `ready`, `error`, …); no wall-clock as semantics |
| 2 | **Background worker — ingest & externals** | Fill cache from parsers, heuristics, or 3rd-party processes (javacs, clangd, …) |
| 3 | **Persistent cache** | Query answers (+ optional relation graph) keyed by generation |
| 4 | **Background worker — edits** | Consume file/buffer events; invalidate or incrementally update cache + FSM |

Each tier **publishes capabilities** (control plane and/or capability doc):

| Capability field | Meaning |
|------------------|---------|
| **Expected ingestion latency** | Order-of-magnitude after workspace open (e.g. “seconds for full tree”, “minutes for JVM index”) — **not** a timeout |
| **Expected update latency** | After `didChange` / watch batch until cache reflects edit |
| **Response quality** | What the tier guarantees (name-only refs vs typed refs, cross-file, project-wide, …) |

**Ordering rule:** Sort tiers by **ingestion/update latency (low → high)**, break ties by **response quality (low → high)**. The **request path** walks the list: first tier that is **ready** and returns **`Ready`** wins; any step not ready within **liveness budget** returns **`NotReady`** and continues. **No** step blocks on a slower tier.

```text
Request (e.g. textDocument/references)
  → Tier[0] read cache / FSM     (fastest, lowest quality)
  → Tier[1] …
  → Tier[N-1]                  (slowest ingest, highest quality)
  → aggregate NotReady / best Ready
```

T3′ is **not** a separate “kind” of tier — it is the **cache + builder** half of a **types-equivalent tier**, with the **engine pack** as the external subsystem in (2).

---

## Mapping today’s stack

| Today | N-tier view |
|-------|-------------|
| **T1** (Tree-sitter / syntax index) | Tier 0 — lowest latency, syntax-only quality |
| **T2** (heuristic graph) | Tier 1 — graph heuristics |
| **T3′** (types cache read) | Tier 2 **read path** — cached types quality |
| **T3 engine** (builder only) | Tier 2 **worker (2)** — fills tier-2 cache |
| Fixed chain prepend order | **Sorted tier list** by published latency → quality |

**Control plane today:** `IndexStatus`, `TierStatus`, `TierReady`, per-engine rows — partial FSM exposure for **package/tier/engine**, not yet per-tier capability matrix or per-query cache readiness.

---

## Plan comparison: B1–B5 (ADR 002) vs N-tier refactor

| ADR 002 track | What it does | N-tier refactor lens |
|---------------|--------------|----------------------|
| **B1** Central `ServeReadiness` | One FSM for ingest, engine, cache | Becomes **registry of tier FSMs** + global workspace state |
| **B2** Remove mux engine resolve | Builder-only externals | Each tier’s worker (2) only; mux never calls tier (2) |
| **B3** `inflight` on T3′ keys | Honest NotReady on miss | **Every tier** exposes `inflight` / `ready` per query key or scope |
| **B4** `CacheReady` push | Notify when cache key ready | Generalize to **`TierReady`** / **`CacheReady`** per tier id |
| **B5** `lsp_child` on worker threads | Engine I/O off mux | Same for **any** external tier backend |

| Only in N-tier refactor (beyond B1–B5) | |
|----------------------------------------|---|
| **`TierDescriptor`** | Static: latency class, quality class, supported `QueryKind`s |
| **Dynamic sort** | Re-order chain when capabilities change (e.g. engine missing → drop tier) |
| **Uniform tier crate/module** | `TierPort`: `readiness()`, `resolve_step()`, `on_ingest`, `on_edit` |
| **Merge T3 + T3′** | Single “types” tier with cache (3) + javacs worker (2) + edit worker (4) |
| **Capability RPC** | Extend `IndexStatus` or new `TierCapabilities` with latency/quality enums |

**Program mapping:** See [abstraction-targets-plan.md](abstraction-targets-plan.md).

```text
ABS-1  serve-abs-1   ADR 002 B1–B5 (readiness, NotReady, CacheReady)
ABS-2  serve-abs-2   ADR 003 FileEventHub
ABS-3  serve-abs-3   TierDescriptor + capabilities RPC
ABS-4  serve-abs-4   TierPort + dynamic chain
```

---

## Request path vs workers (unchanged intent)

| Thread / loop | Allowed work |
|---------------|--------------|
| **Mux LSP** | Read caches + in-memory index; ≤20 ms per tier step; **never** block on child process |
| **Tier ingest worker** | Workspace scan, tree-sitter, package ingest |
| **Tier external worker** | javacs RPC, fill types cache |
| **Tier edit worker** | Watch + `didChange` → invalidate / incremental update |
| **Control I/O** | Poll/push FSM snapshots; idle hooks (e.g. disk watch) |

---

## File events (target vs today)

**Target (N-tier component 4):** One **file-event service** on a **dedicated thread**: abstracts OS notifications (and optional client `didChange`), coalesces, assigns **generation**, fans out to **subscribers** (T1 index, T2 graph, T3′ invalidation, engine `didChange` forward, control `WatchBatch` clients). Tiers do not walk the tree or register their own notify handles.

**Today:**

| Source | Path | Thread |
|--------|------|--------|
| Open buffer | LSP `textDocument/didChange` → `WorkspaceSession::did_change` | **Mux / LSP** (same thread as JSON-RPC) |
| Ghost disk | `ServeHost::poll_disk_watch` — mtime snapshot diff over `collect_sources`, not inotify | **Control accept loop** idle (80 ms read timeout hook); also after `initialize` |
| `WatchBackend` + `WatchCoalescer` | `session.apply_watch` → `IndexService::apply_watch_batch` | **Implemented but not wired** in production `serve` (tests only) |
| `NotifyWatcher` | Port for OS events | **Stub** (queued events; no live notify thread in crate) |
| Progressive client | `WatchSubscribe` / `WatchBatch` push | Batches queued when `poll_disk_watch` finds diffs |

So file updating is **not** a separate pub/sub server; it is **poll-driven ghost reindex** plus **inline didChange**. Tier caches learn about edits through **shared index generation** and ad hoc `apply_disk_path`, not through a common subscription API.

**Implementation:** ABS-2 in [abstraction-checklist.md](abstraction-checklist.md); ADR [003](../adr/003-file-event-hub.md).

---

## Open questions

1. **Authoritative empty** — which tiers may return `Ready([])` vs must return `NotReady` when a higher tier could still answer?
2. **Single vs per-language tier lists** — one chain per `LanguageId` or shared with skips?
3. **Persistent cache** — T3′ in-memory only in v1; disk cache becomes tier (3) for multiple tiers later?
4. **LSP not-ready wire** — experimental capability vs empty array until FSM says otherwise?

---

## References

- Implementation checklist: [types-cache/implementation-checklist.md](../types-cache/implementation-checklist.md) (TCACHE); future **TIER-N** milestones TBD
- Timeout inventory (serve vs UX): [spikes/runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md)
