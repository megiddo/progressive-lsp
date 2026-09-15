# Serve abstraction checklist (ABS-1 … ABS-4)

**Canonical checklist:** [serve-abs/implementation-checklist.md](../serve-abs/implementation-checklist.md) (includes ABS-0, integration tests, phase exits). This file is a **compact mirror** for quick WP lookup.

Depends-on is strict. Stack branches per [abstraction-targets-plan.md](abstraction-targets-plan.md).

---

## ABS-1 — Readiness FSM (ADR 002)

| WP | Work | Depends | Exit | Done |
|----|------|---------|------|------|
| ABS-1.1 | `ServeReadiness` (or extend session): workspace ingest, package tier, engine rows, query-key inflight map | TCACHE on main | Unit: transitions on ingest complete, engine ready | [ ] |
| ABS-1.2 | Remove synchronous `try_engine_discover` / mux `supervisor.resolve` | ABS-1.1 | `references_falls_back_*` test removed or replaced with NotReady + builder | [ ] |
| ABS-1.3 | T3′ `inflight` keys; mux chain returns NotReady when pending (not `Ready([])` at T1) | ABS-1.1 | Unit: miss + inflight → NotReady; hit → Ready | [ ] |
| ABS-1.4 | Proto `CacheReady` + control push; wire builder completion | ABS-1.3 | Unit + control round-trip | [ ] |
| ABS-1.5 | Audit: `lsp_child` only on builder/warm threads | ABS-1.2 | Grep gate or test: no resolve from mux thread | [ ] |
| ABS-1.6 | Update [progressive-lsp-apis.md](../progressive-lsp-apis.md) | ABS-1.4 | Doc lists `CacheReady` | [ ] |

**Phase exit:** ABS-1.1–ABS-1.5; [milestones.md](../milestones.md) ABS-1 signed off.

---

## ABS-2 — File event hub (ADR 003)

| WP | Work | Depends | Exit | Done |
|----|------|---------|------|------|
| ABS-2.1 | `FileEventHub` struct + dedicated thread; mpsc in, coalesced batches out | ABS-1.3 | Unit: FakeWatcher → one batch, generation bump | [ ] |
| ABS-2.2 | `Subscriber` trait; index subscriber → `apply_watch_batch` | ABS-2.1 | Reuse existing index tests path | [ ] |
| ABS-2.3 | T3′ subscriber → invalidate / generation | ABS-2.1 | Types-cache invalidation test | [ ] |
| ABS-2.4 | Engine forward subscriber → `supervisor.forward_did_change` | ABS-2.1 | Fake supervisor records URIs | [ ] |
| ABS-2.5 | Control subscriber → journal + `pending_batches` | ABS-2.1 | WatchBatch push without `poll_disk_watch` diff | [ ] |
| ABS-2.6 | Wire hub at `ServeHost` initialize; demote `poll_disk_watch` to fallback/rescan only | ABS-2.2–2.5 | Integration: disk modify updates index | [ ] |
| ABS-2.7 | `didChange` enqueues hub (buffer path) | ABS-2.1 | Unit: coalesced with disk events | [ ] |
| ABS-2.8 | Live `NotifyWatcher` (or document platform scope) | ABS-2.1 | Manual/Linux CI note if Darwin limited | [ ] |

**Phase exit:** ABS-2.6; ghost edit IT without 2s sleep loop where FSM allows.

---

## ABS-3 — Tier registry + capabilities

| WP | Work | Depends | Exit | Done |
|----|------|---------|------|------|
| ABS-3.1 | `TierDescriptor` value object (id, latency class, quality class, query kinds) | ABS-1 | Unit: sort order | [ ] |
| ABS-3.2 | Static registry for syntax / graph / types tiers | ABS-3.1 | Matches T1/T2/T3′ today | [ ] |
| ABS-3.3 | Extend `IndexStatus` or add `TierCapabilities` RPC (additive proto) | ABS-3.2 | progressive-lsp-apis.md updated | [ ] |
| ABS-3.4 | Per-tier FSM exposed in control snapshot | ABS-1, ABS-3.2 | Harness reads capabilities | [ ] |

**Phase exit:** ABS-3.3 documented in [control-protocol.md](../control-protocol.md).

---

## ABS-4 — TierPort (optional depth)

| WP | Work | Depends | Exit | Done |
|----|------|---------|------|------|
| ABS-4.1 | `TierPort` trait: `readiness`, `resolve_step`, `on_ingest`, `on_edit_event` | ABS-3 | Fake tier chain test | [ ] |
| ABS-4.2 | Refactor T1/T2/types read steps behind ports (no lang crate split) | ABS-4.1 | Behavior parity tests | [ ] |
| ABS-4.3 | Dynamic chain: drop types tier when engine `missing` | ABS-4.2 | Unit: chain length | [ ] |

**Human gate:** ABS-4.2 must not refactor `progressive-lsp-lang-*` boundaries without sign-off.

---

## Milestone → branch

| Milestone | Branch | WPs |
|-----------|--------|-----|
| ABS-1 | `serve-abs-1` | ABS-1.* |
| ABS-2 | `serve-abs-2` | ABS-2.* |
| ABS-3 | `serve-abs-3` | ABS-3.* |
| ABS-4 | `serve-abs-4` | ABS-4.* |
