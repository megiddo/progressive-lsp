# ADR 001: Types cache overlay on T3 substrate

**Status:** Accepted  
**Date:** 2026-03-14  
**Deciders:** Product owner (human); implementation via TCACHE orchestration  

## Context

T3 pack engines (e.g. javacs) are the **authoritative** source for types-tier discover, but RPC latency is unbounded and incompatible with interactive IDE discover on the mux thread. Progressive product semantics require:

- Fast answers when possible (**≤ 20 ms** per chain step on the request path).
- **T2/T1 only when T3′ misses or T3 is in-progress** — not as the primary “cache.”
- **Eventual consistency:** background T3 fills a progressive-owned index; repeat queries read **Types** from cache.

Prior ad-hoc timeouts (multi-second discover deadlines, hundreds-of-ms engine try budgets) are **rejected** as product policy.

## Decision

1. Introduce **T3′ (types cache)**: `TypesCacheStore` + `TypesCacheResolver` as the **first** chain step for discover-class queries.
2. **Remove live `EngineResolver` from the mux request path** after cutover; T3 engine serves **only** `TypesCacheBuilder` (background).
3. Implement **query cache first**, then **relation graph** for references/implementation/definition enrichment ([design.md](../types-cache/design.md)).
4. **Invalidate** via shared index/generation with `IndexService` / dirty set — no parallel watch semantics.
5. **Requirements** [requirements.md](../types-cache/requirements.md) are normative for TCACHE milestones.

## Consequences

**Positive**

- Discover latency decoupled from javacs cold/warm RPC on the hot path.
- Clear ownership: cache = types tier; engine = builder.
- Testable timing gates (TC-5).

**Negative**

- First click may be T2/T1 until builder fills cache (expected; document in UX).
- Additional crate and invalidation complexity.
- Short period of dual behavior behind feature flag during TC-2/TC-3.

**Follow-up**

- [implementation-checklist.md](../types-cache/implementation-checklist.md) TC-0 through TC-5.
- [design-pattern-audit spike](../spikes/design-pattern-audit.md) before large refactors (TC-1, TC-4).

## References

- [types-cache/requirements.md](../types-cache/requirements.md)
- [types-cache/design.md](../types-cache/design.md)
- [types-cache/agent-context.md](../types-cache/agent-context.md)
- [design-patterns.md](../design-patterns.md)
