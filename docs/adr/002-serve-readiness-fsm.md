# ADR 002: Serve-side readiness as a deterministic state machine

**Status:** Accepted  
**Date:** 2026-09-15  
**Deciders:** Product owner (human)  
**Related:** [001-types-cache-overlay.md](001-types-cache-overlay.md), [control-protocol.md](../control-protocol.md), [runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md)

## Context

Interactive discover must not depend on **wall-clock timeouts** or **blocking engine RPC** on the mux thread. Clients (POC IDE, harness, future editors) may use their own UX polling; that is **out of scope** for this ADR.

The **progressive-lsp serve process** must own a single, authoritative model of subsystem readiness: workspace ingest, per-package tier, T3 engine packs, document sync, and T3′ query cache. Transitions are **events** (initialize, ingest complete, engine warm, builder put, invalidation). Observers read state via **control RPC/push** or infer from **discover metadata** — not from “waited N ms.”

Today:

- Mux **chain** already returns `NotReady` per step (T3′ → T2 → T1) within liveness budget; session still maps terminal `NotReady` to **`Ready([])` at Syntax** on the LSP wire.
- **IndexStatus** / **TierReady** / **engines[]** partially expose backend state; there is no unified FSM type or **cache-key readiness**.
- **Engine I/O** (`lsp_child`, optional sync fallback on the session path) can **block threads indefinitely** — not a state, just stuck I/O.
- Control socket **80 ms** read timeout is an **idle scheduling hook** (`poll_disk_watch`), not a tier outcome (see [runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md)).

## Decision

1. **Serve owns the readiness FSM.** All subsystems report into one composition root (`ServeHost` / `WorkspaceSession` + `TypesCacheStack` + `EngineSupervisor`). No client-side timeout is required for correctness.

2. **Request path (mux LSP discover)** remains **pure poll of local state**:
   - Each chain step: **`Ready(result)`** or **`NotReady`** within **REQ-NFR-1** liveness (≤ 20 ms).
   - **No** `recv_timeout`, **no** synchronous `EngineSupervisor::resolve` on the mux thread after cutover.
   - **No** wall-clock “try budget” as product policy.

3. **Background monitors** (threads or event loops) advance the FSM and perform slow work:
   - Package **ingest** (`ingest_workspace`).
   - **EngineSupervisor** spawn, warm, health (`poll_health`), forward LSP notifications.
   - **TypesCacheBuilder** queue + engine fill.
   - **Disk watch** batching (`poll_disk_watch`; optional coalescer later).

4. **Control plane is the readiness API** for clients that care:
   - Today: `IndexStatus` (ingest, packages, `engines[]`, `cache_entries`), `TierStatus`, push **`TierReady`**.
   - **Add** (v1 additive): query-scoped or region-scoped **`CacheReady`** / extended `IndexStatus` rows when T3′ keys transition **inflight → ready** (FR-6.1 in [requirements.md](../types-cache/requirements.md)).
   - Clients **poll** `IndexStatus` or subscribe to pushes; they **do not** implement parallel timeout state machines for tier truth.

5. **LSP wire vs internal `NotReady`:** Until an experimental LSP capability exposes explicit “not ready,” mux may return an empty location list **only** when the FSM records **authoritative empty** at T1/T2/T3′. When the FSM says **pending** (engine warming, cache inflight, ingest incomplete for that query class), the serve layer **must** treat the operation as **`NotReady` internally** and **must not** block waiting for subsystems. Mapping that case to LSP (empty vs null vs `$`/progress) is a **protocol follow-up**; control plane still reports the true state.

## State machine (normative sketch)

```text
Workspace
  not_started → running (ingest) → done

Package (per package_id)
  syntax → graph (post-ingest) → types (engine ready + policy)

Engine (per language/pack)
  missing | pending | warming | ready | error

Document (per FileId)
  unknown | indexed | lsp_open | dirty(generation)

Query cache (per TypesCacheKey)
  absent | inflight | ready(empty) | ready(locations) | stale
```

**Transitions (examples):**

| Event | Transition |
|-------|------------|
| `initialize` + discover | Workspace → running; engines → pending/spawn |
| Ingest package completes | Package → graph; may enqueue builder warm |
| `poll_health` / warm complete | Engine → ready; Package may → types; push **TierReady** |
| T3′ miss | Query → inflight; builder queued |
| Builder put | Query → ready(*); bump `cache_entries`; push **CacheReady** (future) |
| `didChange` / watch | Document → dirty; invalidate query keys → absent/stale |

## Consequences

**Positive**

- One place to reason about “why is references empty?” — read FSM + WAL `cache_state`, not client timers.
- Harness can assert on **IndexStatus** and future **CacheReady** instead of second-scale LSP deadlines for product behavior.
- Aligns with ADR 001 (T3′ + builder only).

**Negative**

- Requires removing **sync engine fallback** and any remaining blocking T3 on mux (see spike gap **G3–G4**).
- LSP clients may still see ambiguous empty arrays until experimental not-ready is wired; control plane must be complete enough for IDE/harness.

**Implementation tracks (non-exhaustive)**

| Track | Work |
|-------|------|
| B1 | Central `ServeReadiness` (or extend session) holding FSM; update on existing hooks |
| B2 | Remove mux-thread engine resolve; builder-only T3 |
| B3 | `inflight` keys in `TypesCacheStore` → `NotReady` without blocking |
| B4 | Proto + `CacheReady` push; document in [control-protocol.md](../control-protocol.md) |
| B5 | `lsp_child`: worker-only blocking; mux never waits on child stdin |

POC IDE mux deadlines remain **UX-only**; do not use them to define serve correctness.

## References

- [types-cache/requirements.md](../types-cache/requirements.md) FR-1, FR-2, FR-6, REQ-NFR-1
- [types-cache/design.md](../types-cache/design.md)
- [architecture/n-tier-framework.md](../architecture/n-tier-framework.md)
- [spikes/runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md)
