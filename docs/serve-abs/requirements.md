# SERVE-ABS — requirements

**Normative** requirements for the serve abstraction program (ABS-1 … ABS-4).  
**Prerequisite:** TCACHE FR-1 … FR-5 and REQ-NFR-1 … REQ-NFR-3 on `main`.

**Shared readiness requirements** (already in types-cache; still binding):

- **FR-6** Control / readiness API — [types-cache/requirements.md](../types-cache/requirements.md#fr-6-control--readiness-api)
- **FR-7** Serve readiness FSM — [types-cache/requirements.md](../types-cache/requirements.md#fr-7-serve-readiness-fsm)
- **REQ-NFR-1.4** No serve-side wall-clock as tier semantics — [types-cache/requirements.md](../types-cache/requirements.md#req-nfr-1-liveness)

This document adds **ABS-specific** IDs. Design: [design.md](design.md). WPs: [implementation-checklist.md](implementation-checklist.md).

---

## FR-ABS-1 Readiness FSM (ABS-1)

- **FR-ABS-1.1** Serve **MUST** maintain a single composition root for readiness (`ServeReadiness` or equivalent on `ServeHost` / `WorkspaceSession`) covering: workspace ingest, per-package tier, per-engine pack, per-document generation, and T3′ query keys ([ADR 002](../adr/002-serve-readiness-fsm.md)).
- **FR-ABS-1.2** Mux discover **MUST NOT** invoke synchronous `EngineSupervisor::resolve`, blocking `lsp_child` recv, or any multi-second engine I/O on the request thread (**FR-7.3**).
- **FR-ABS-1.3** When T3′ has no ready entry and a builder job is **inflight** for that key, the chain **MUST** return **`NotReady`** at the types step — **MUST NOT** return **`Ready([])`** as a stand-in for pending (**FR-2.2**).
- **FR-ABS-1.4** When the FSM records **authoritative empty** at a tier (known zero locations at valid generation), **`Ready([])`** **MAY** be returned at that tier.
- **FR-ABS-1.5** Control plane **MUST** expose readiness deltas: existing `IndexStatus`, `TierStatus`, push **`TierReady`**; **MUST** add (additive proto) push **`CacheReady`** (or equivalent row) when a T3′ key transitions **inflight → ready** (**FR-6.1**).
- **FR-ABS-1.6** [progressive-lsp-apis.md](../progressive-lsp-apis.md) **MUST** document new control messages and fields at ABS-1 sign-off.

---

## FR-ABS-2 File event hub (ABS-2)

- **FR-ABS-2.1** Serve **MUST** route disk and buffer file events through a **`FileEventHub`** on a dedicated thread ([ADR 003](../adr/003-file-event-hub.md)).
- **FR-ABS-2.2** Subscribers (index, T3′ invalidation, engine LSP forward, control journal / `WatchBatch`) **MUST** register with the hub; they **MUST NOT** each own a primary OS watch for the same workspace.
- **FR-ABS-2.3** LSP **`textDocument/didChange`** **MUST** enqueue hub events (buffer generation) in addition to any minimal mux bookkeeping required for LSP correctness.
- **FR-ABS-2.4** **`poll_disk_watch`** **MAY** remain as **fallback** (rescan / overflow / hub not started); it **MUST NOT** be the only path that updates index generation for ghost (on-disk) edits after ABS-2 cutover.
- **FR-ABS-2.5** Per-path **generation** **MUST** be monotonic across coalesced batches (document ordering in [design.md](design.md)).

---

## FR-ABS-3 Tier registry and capabilities (ABS-3)

- **FR-ABS-3.1** Serve **MUST** define **`TierDescriptor`** values: stable tier id, latency class, quality class, supported discover/query kinds.
- **FR-ABS-3.2** A static registry **MUST** describe today’s T1, T2, and types tier (T3′ read + engine worker) with sort order **latency → quality** ([n-tier-framework.md](../architecture/n-tier-framework.md)).
- **FR-ABS-3.3** Control **MUST** expose tier capabilities additively: extend **`IndexStatus`** and/or new **`TierCapabilities`** RPC — no breaking proto changes.
- **FR-ABS-3.4** Per-tier FSM snapshots **MUST** be readable from control (for harness and POC IDE) without client-side timeout state machines.

---

## FR-ABS-4 TierPort abstraction (ABS-4)

- **FR-ABS-4.1** A **`TierPort`** trait (or equivalent module boundary) **MUST** define: `readiness`, `resolve_step`, `on_ingest`, `on_edit_event`.
- **FR-ABS-4.2** T1/T2/types read steps **SHOULD** be refactored behind ports without changing LSP wire behavior (parity tests).
- **FR-ABS-4.3** When an engine pack is **`missing`**, the discover chain **MUST** omit or skip the types tier dynamically (unit-tested).
- **FR-ABS-4.4** Refactors **MUST NOT** split or move **`progressive-lsp-lang-*`** crate boundaries without human sign-off ([productization/agent-context.md](../productization/agent-context.md)).

---

## Non-functional requirements

### REQ-ABS-NFR-1 Testing and hygiene

- **REQ-ABS-NFR-1.1** Same bar as TCACHE on touched crates: ≥95% line cov / ≥80% mutants where llvm-tools available; **no `sleep`** in tests ([testing.md](../testing.md)).
- **REQ-ABS-NFR-1.2** Integration harness **MUST** gain or extend rows per phase (see [implementation-checklist.md](implementation-checklist.md#integration-tests-by-phase)).
- **REQ-ABS-NFR-1.3** New public types **MUST** appear in [design-patterns.md](../design-patterns.md) before merge.

### REQ-ABS-NFR-2 Liveness (unchanged)

- **REQ-ABS-NFR-2.1** Each chain step **MUST** honor **REQ-NFR-1** (≤ 20 ms or **`NotReady`**).

### REQ-ABS-NFR-3 Observability

- **REQ-ABS-NFR-3.1** WAL discover **`cache_state`** remains authoritative for harness (**REQ-NFR-3.1**).
- **REQ-ABS-NFR-3.2** Control push for **`CacheReady`** **SHOULD** include key fingerprint / query kind (no document bodies).

---

## Out of scope (SERVE-ABS v1)

- Client-side (POC IDE) timeout removal — UX may poll; not normative for serve truth.
- Persisting hub journal across process restart.
- Full dynamic re-sort of tiers at runtime beyond “engine missing” (ABS-4.3).
- LSP experimental capability for wire-level **`NotReady`** (follow-up; control plane is v1 truth).

---

## Traceability

| Requirement | Verified by |
|-------------|-------------|
| FR-ABS-1.1–1.3 | Unit: FSM transitions; chain NotReady on inflight |
| FR-ABS-1.2, FR-ABS-1.5 | Grep gate / no mux resolve; control round-trip tests |
| FR-ABS-2.* | Unit: FakeWatcher; IT: disk edit without poll-only path |
| FR-ABS-3.* | Unit: descriptor sort; harness reads capabilities |
| FR-ABS-4.* | Fake tier chain; dynamic chain length |
| REQ-ABS-NFR-* | CI scripts + milestone sign-off in [milestones.md](../milestones.md) |
