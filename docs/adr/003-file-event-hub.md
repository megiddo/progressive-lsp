# ADR 003: File event hub (pub/sub)

**Status:** Accepted (design); **implementation** tracked as ABS-2  
**Date:** 2026-09-15  
**Related:** [002-serve-readiness-fsm.md](002-serve-readiness-fsm.md), [abstraction-targets-plan.md](../architecture/abstraction-targets-plan.md)

## Context

File changes arrive from **LSP buffers** and **disk**. Today serve mixes mux-inline `didChange`, control-idle **tree scan** (`poll_disk_watch`), and an **unwired** `WatchCoalescer` path. Tier caches do not subscribe to a common event service.

N-tier component **(4)** requires a single edit pipeline so **expected update latency** is meaningful.

## Decision

1. Introduce **`FileEventHub`**: one coalescing thread, abstracting `WatchBackend` (OS notify + test fakes).
2. **Subscribers** (index, T3′ invalidation, engine LSP forward, control journal / `WatchBatch`) register with the hub; they do not open their own watches.
3. **LSP `didChange`** publishes into the hub (same generation model as disk) instead of only mutating index on the mux thread.
4. **`poll_disk_watch`** becomes **fallback** (rescan / `need_rescan`) when notify overflows or hub not started — not the primary ghost path.
5. Control **80 ms** socket read timeout remains an **I/O scheduling hook**, not file-event semantics.

## Consequences

**Positive:** One place to coalesce; tiers align with N-tier edit workers; real notify on Linux dogfood.

**Negative:** Thread lifecycle in serve; ordering guarantees must be documented (per-path generation monotonic).

**Implementation:** [abstraction-checklist.md](../architecture/abstraction-checklist.md) ABS-2.*

## References

- [n-tier-framework.md](../architecture/n-tier-framework.md) — File events section
- [progressive-lsp-apis.md](../progressive-lsp-apis.md) — `WatchBatch`, `FilesSince`
