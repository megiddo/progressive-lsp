# Types cache — requirements

Normative requirements for the **T3′ (types cache)** overlay. Satisfies progressive semantics: fast interactive tier, slow engine as **builder**, T2/T1 as **miss fallbacks**.

Related: [design.md](design.md), [ADR 001](../adr/001-types-cache-overlay.md), [implementation-checklist.md](implementation-checklist.md).

---

## Definitions

| Term | Meaning |
|------|---------|
| **T3 engine** | Pack child (e.g. javacs) reached via `EngineSupervisor` / `lsp_child` |
| **T3′ (types cache)** | Progressive-owned indexed store served at **`Tier::Types`** on the **request path** |
| **Liveness budget** | Maximum wall time a **single chain step** may spend before returning `NotReady` |
| **Cache hit** | T3′ returns `Ready` with locations (or authoritative empty) without calling the engine on the request path |
| **Builder** | Background component that invokes T3 engine and **writes** T3′ |

---

## Functional requirements

### FR-1 Request path chain

- **FR-1.1** Discover and equivalent resolve operations on the serve mux thread **MUST** use chain order: **T3′ → T2 → T1**.
- **FR-1.2** Each step **MUST** return within the liveness budget (**REQ-NFR-1**) or **`NotReady`**; the chain **MUST** continue to the next step.
- **FR-1.3** The first **`Ready`** result **MUST** win (including `Ready` with empty locations). Lower tiers **MUST NOT** merge with or append to a higher tier’s result.
- **FR-1.4** **`EngineResolver` (live T3 RPC) MUST NOT** run on the mux request path for production discover once T3′ is enabled.

### FR-2 T3′ read behavior

- **FR-2.1** T3′ **MUST** answer from **query cache** and/or **relation graph** (see [design.md](design.md)), never from blocking engine I/O on the request path.
- **FR-2.2** **Unknown** (no entry, stale generation, in-flight build): T3′ **MUST** return **`NotReady`** (not `Ready([])`).
- **FR-2.3** **Known empty** (engine or builder recorded “resolved, zero locations” at a valid generation): T3′ **MAY** return **`Ready([])`** with `tier=Types`.
- **FR-2.4** Cache hits **MUST** expose `tier=Types` to the client (same wire semantics as engine-backed types today).

### FR-3 Builder (background T3)

- **FR-3.1** On T3′ miss (and when engine is ready and document sync allows), the system **MUST** be able to **queue** a builder job that calls T3 engine **off** the mux thread.
- **FR-3.2** Builder **MUST** write query entries and **SHOULD** write relation edges (definition, references, implementation, typeDefinition) as specified in [design.md](design.md).
- **FR-3.3** Builder **MUST** use **single-flight** per logical key (or per package) so repeated misses do not spawn unbounded parallel javacs RPCs for the same query.
- **FR-3.4** Builder failure **MUST NOT** panic serve; stale/missing cache continues to yield T2/T1 fallbacks.

### FR-4 Invalidation

- **FR-4.1** T3′ keys **MUST** include a **content generation** (or equivalent) tied to the same dirty/generation source as `IndexService` / `DirtySet` (one source of truth).
- **FR-4.2** `textDocument/didChange` and watch-driven reindex for a file **MUST** invalidate T3′ entries and graph slices for that file (and **MAY** invalidate dependent keys per language policy).
- **FR-4.3** Engine restart or `abort_language` **MUST** mark engine-sourced cache entries stale; builder **MUST** be allowed to refill.

### FR-5 Query kinds (v1 scope)

- **FR-5.1** v1 builder/read path **MUST** cover: `Definition`, `Implementation`, `References`, `TypeDefinition`.
- **FR-5.2** Hover **MAY** be deferred to a later WP; if not cached, existing chain behavior applies.

### FR-6 Control / readiness API

- **FR-6.1** Serve **MUST** expose subsystem readiness through the **control plane** ([ADR 002](../adr/002-serve-readiness-fsm.md)): `IndexStatus`, `TierStatus`, push `TierReady`, and (additive) **`CacheReady`** or equivalent when a T3′ key leaves `inflight`.
- **FR-6.2** Clients **MAY** poll or subscribe; serve **MUST NOT** require client-side wall-clock timeouts for correct tier or cache state.
- **FR-6.3** POC IDE timing is UX-only; RunLog **SHOULD** record cache hit/miss and timings (see REQ-NFR-3).

### FR-7 Serve readiness FSM

- **FR-7.1** `ServeHost` / `WorkspaceSession` **MUST** maintain deterministic states for workspace ingest, package tier, engine pack, document generation, and T3′ query keys ([ADR 002](../adr/002-serve-readiness-fsm.md)).
- **FR-7.2** Background threads (ingest, engine supervisor, types-cache builder, watch poll) **MUST** only **transition** FSM and enqueue work — not block mux discover waiting for completion.
- **FR-7.3** Mux discover **MUST NOT** call synchronous `EngineSupervisor::resolve` or blocking `lsp_child` I/O.

---

## Non-functional requirements

### REQ-NFR-1 Liveness

- **REQ-NFR-1.1** **Liveness budget** for each of T3′, T2, and T1 on the request path: **≤ 20 ms** wall time (p99 on fixture corpora in CI — see [implementation-checklist.md](implementation-checklist.md) TC-5).
- **REQ-NFR-1.2** Exceeding the budget **MUST** be treated as **`NotReady`** for that step, not a blocking wait.
- **REQ-NFR-1.3** Arbitrary multi-second **recv timeouts** on the request path **MUST NOT** be used as a UX strategy (remove/replace legacy budgets in `EngineResolver` when T3′ lands).
- **REQ-NFR-1.4** **Serve** **MUST NOT** use wall-clock I/O timeouts as tier or discover semantics; use FSM **`NotReady`** and control-plane state instead ([ADR 002](../adr/002-serve-readiness-fsm.md)). Client UX timeouts are non-normative. Inventory: [runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md).

### REQ-NFR-2 Eventual consistency

- **REQ-NFR-2.1** First request after open **MAY** be T2/T1; a later request **SHOULD** be T3′ cache hit after builder completes without user waiting on the mux thread.
- **REQ-NFR-2.2** Stale-while-revalidate: serving slightly stale T3′ **MUST** be explicit in metadata (generation / `filled_at`); default policy is **do not serve** stale for discover until invalidation rules say otherwise (fail to T2/T1).

### REQ-NFR-3 Observability

- **REQ-NFR-3.1** Serve WAL discover rows **MUST** include: `resolve_ms`, `tier`, `location_count`, and **`cache_state`** ∈ {`hit`, `miss`, `stale`, `n/a`}.
- **REQ-NFR-3.2** Builder **SHOULD** log: `builder_ms`, `query_kind`, key fingerprint (no file bodies), outcome.
- **REQ-NFR-3.3** Integration traces **MUST** assert timing columns (see TC-5).

### REQ-NFR-4 Architecture hygiene

- **REQ-NFR-4.1** New types **MUST** appear in [design-patterns.md](../design-patterns.md) before merge (see [pattern-targets.md](pattern-targets.md) after audit spike).
- **REQ-NFR-4.2** T3′ **MUST** live in a dedicated crate or clearly bounded module — not spread across `serve_host` ad hoc.
- **REQ-NFR-4.3** **`progressive-lsp-lang-*` crate boundaries** remain human-gated per [productization/agent-context.md](../productization/agent-context.md); T3′ is language-agnostic.

---

## Out of scope (v1)

- Persisting T3′ to disk under `$PREFIX/cache/` (optional later WP; `IndexCache` pattern may apply).
- Replacing javacs/project import inside the engine pack.
- Merging T2 graph and T3′ graph into one store (may converge later; v1 may duplicate with clear ownership).

---

## Traceability

| Requirement | Verified by |
|-------------|-------------|
| FR-1, FR-2, REQ-NFR-1 | Unit: chain + cache resolver timing tests |
| FR-3, FR-4 | Unit: builder + invalidation tests |
| FR-5, REQ-NFR-2 | Integration: `run-discover-container.sh` + TC-5 golden |
| REQ-NFR-3 | WAL schema + RunLog payload tests |
