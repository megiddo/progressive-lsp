# SERVE-ABS — implementation checklist

Fine-grained work packages for **ABS-0 … ABS-4**. Depends-on is strict.

**Base branch:** `main` with TCACHE merged. **Branch stack:** [README.md](README.md) and [implementation-plan.md](../implementation-plan.md) SERVE-ABS section.

**Sign-off:** Copy checklist from [milestones.md](../milestones.md) SERVE-ABS for each milestone (tests, cov, mutants, no sleep, APIs doc, phase exit below).

**Agent workflow:** [agent-context.md](agent-context.md).

---

## Phase ABS-0 — Documentation & ADR alignment (docs only)

**Status:** **Signed off** on `main`.

**Exit:** Requirements and design accepted; checklist + agent-context on disk; no mandatory crate changes for phase exit.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| ABS-0.1 | [requirements.md](requirements.md) normative FR/NFR | TCACHE on main | IDs stable; traceability table | [x] |
| ABS-0.2 | [design.md](design.md) architecture | ABS-0.1 | FSM, hub, registry, proto sketch | [x] |
| ABS-0.3 | [ADR 002](../adr/002-serve-readiness-fsm.md) accepted | ABS-0.1 | Status Accepted | [x] |
| ABS-0.4 | [ADR 003](../adr/003-file-event-hub.md) accepted | ABS-0.2 | Status Accepted (impl ABS-2) | [x] |
| ABS-0.5 | [agent-context.md](agent-context.md) orchestrator payloads | ABS-0.3 | Meta + phase blocks | [x] |
| ABS-0.6 | Cross-links: plan, milestones, implementation-plan, n-tier | ABS-0.5 | All point at `serve-abs/` | [x] |

**Phase exit:** ABS-0.1–ABS-0.6; human ack to start **`serve-abs-1`**.

---

## Phase ABS-1 — Readiness FSM (ADR 002)

**Branch:** `serve-abs-1` (stack on `main`).

**Exit:** Integration harness asserts control FSM + discover `cache_state`; no sync engine on mux; `CacheReady` documented.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| ABS-1.1 | `ServeReadiness` (or extend session): workspace ingest, package tier, engine rows, query-key inflight map | TCACHE on main | Unit: transitions on ingest complete, engine ready | [x] |
| ABS-1.2 | Remove synchronous `try_engine_discover` / mux `supervisor.resolve` | ABS-1.1 | No mux resolve; IT uses NotReady + builder path | [x] |
| ABS-1.3 | T3′ `inflight` keys; mux chain returns NotReady when pending (not `Ready([])` at T1 for pending types) | ABS-1.1 | Unit: miss + inflight → NotReady; hit → Ready | [x] |
| ABS-1.4 | Proto `CacheReady` + control push; wire builder completion | ABS-1.3 | Unit + control round-trip | [x] |
| ABS-1.5 | Audit: `lsp_child` only on builder/warm threads | ABS-1.2 | Grep gate or test: no resolve from mux thread | [x] |
| ABS-1.6 | Update [progressive-lsp-apis.md](../progressive-lsp-apis.md) | ABS-1.4 | Doc lists `CacheReady` | [x] |

**Phase exit:** ABS-1.1–ABS-1.6 signed off in [milestones.md](../milestones.md).

---

## Phase ABS-2 — File event hub (ADR 003)

**Branch:** `serve-abs-2`.

**Exit:** Disk ghost edit updates index/cache without relying on poll-only tree scan; `didChange` enqueues hub.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| ABS-2.1 | `FileEventHub` struct + dedicated thread; mpsc in, coalesced batches out | ABS-1.3 | Unit: FakeWatcher → one batch, generation bump | [x] |
| ABS-2.2 | `Subscriber` trait; index subscriber → `apply_watch_batch` | ABS-2.1 | Reuse existing index tests path | [x] |
| ABS-2.3 | T3′ subscriber → invalidate / generation | ABS-2.1 | Types-cache invalidation test | [x] |
| ABS-2.4 | Engine forward subscriber → `supervisor.forward_did_change` | ABS-2.1 | Fake supervisor records URIs | [x] |
| ABS-2.5 | Control subscriber → journal + `pending_batches` | ABS-2.1 | WatchBatch push without `poll_disk_watch` diff | [x] |
| ABS-2.6 | Wire hub at `ServeHost` initialize; demote `poll_disk_watch` to fallback/rescan only | ABS-2.2–2.5 | Integration: disk modify updates index | [x] |
| ABS-2.7 | `didChange` enqueues hub (buffer path) | ABS-2.1 | Unit: coalesced with disk events | [x] |
| ABS-2.8 | Live `NotifyWatcher` (or document platform scope) | ABS-2.1 | Manual/Linux CI note if Darwin limited | [x] |

**Phase exit:** ABS-2.6; ghost edit IT without serve-side sleep for tier truth.

---

## Phase ABS-3 — Tier registry + capabilities

**Branch:** `serve-abs-3`.

**Exit:** Harness reads tier capabilities; [control-protocol.md](../control-protocol.md) updated.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| ABS-3.1 | `TierDescriptor` value object (id, latency class, quality class, query kinds) | ABS-1 | Unit: sort order | [x] |
| ABS-3.2 | Static registry for syntax / graph / types tiers | ABS-3.1 | Matches T1/T2/T3′ today | [x] |
| ABS-3.3 | Extend `IndexStatus` or add `TierCapabilities` RPC (additive proto) | ABS-3.2 | progressive-lsp-apis.md updated | [x] |
| ABS-3.4 | Per-tier FSM exposed in control snapshot | ABS-1, ABS-3.2 | Harness reads capabilities | [x] |

**Phase exit:** ABS-3.3 documented in control-protocol + APIs catalog.

---

## Phase ABS-4 — TierPort (optional depth)

**Branch:** `serve-abs-4`. **Human gate:** lang plugin crate boundaries.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| ABS-4.1 | `TierPort` trait: `readiness`, `resolve_step`, `on_ingest`, `on_edit_event` | ABS-3 | Fake tier chain test | [ ] |
| ABS-4.2 | Refactor T1/T2/types read steps behind ports (no lang crate split) | ABS-4.1 | Behavior parity tests | [ ] |
| ABS-4.3 | Dynamic chain: drop types tier when engine `missing` | ABS-4.2 | Unit: chain length | [ ] |

**Phase exit:** ABS-4.1–ABS-4.3 or human defers ABS-4.2/4.3 with ABS-4.1 merged.

---

## Integration tests by phase

| ID | Phase | Scenario | Pass criteria |
|----|-------|----------|---------------|
| IT-ABS-1a | ABS-1 | `run-discover-container` references after definition | No mux engine sync; WAL `cache_state` miss → hit or NotReady; optional wait on **control** `CacheReady`, not LSP sleep for semantics |
| IT-ABS-1b | ABS-1 | Open workspace before file buffer | T3′ builder enqueues for ingested paths (ingest-at-open) |
| IT-ABS-2a | ABS-2 | Modify file on disk in harness workspace | Index generation bumps via hub; T3′ invalidates |
| IT-ABS-2b | ABS-2 | `didChange` only (no disk) | Hub batch; same generation rules as ABS-2.7 unit |
| IT-ABS-3 | ABS-3 | Control `IndexStatus` / `TierCapabilities` | Static tier list order; engine row matches FSM |
| IT-ABS-4 | ABS-4 | Engine pack disabled fixture | Chain skips types tier; T2/T1 still answer |

---

## Milestone → branch

| Milestone | Branch | WPs |
|-----------|--------|-----|
| ABS-0 | `main` (docs) | ABS-0.* |
| ABS-1 | `serve-abs-1` | ABS-1.* |
| ABS-2 | `serve-abs-2` | ABS-2.* |
| ABS-3 | `serve-abs-3` | ABS-3.* |
| ABS-4 | `serve-abs-4` | ABS-4.* |

**Legacy WP table:** [architecture/abstraction-checklist.md](../architecture/abstraction-checklist.md) mirrors ABS-1–4 rows; **canonical** checklist is this file.
