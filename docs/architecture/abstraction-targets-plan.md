# Serve abstraction targets — master plan

**Status:** Planned (post–TCACHE merge to `main`).  
**Prerequisite:** TCACHE-0 … TCACHE-5 merged ([implementation-plan.md](../implementation-plan.md)).

Two coupled refactors on the **progressive-lsp backend** (POC IDE timing stays UX-only):

| Target | Summary | Primary ADR / doc |
|--------|---------|-------------------|
| **A — N-tier + readiness FSM** | Generic tier shape (FSM, ingest worker, cache, edit worker); ordered chain by **latency → quality**; control plane publishes capability + readiness | [ADR 002](../adr/002-serve-readiness-fsm.md), [n-tier-framework.md](n-tier-framework.md) |
| **B — File event hub** | One thread coalesces OS + buffer events; **pub/sub** to tier edit workers, index, control journal | [ADR 003](../adr/003-file-event-hub.md) (draft) |

**Public API catalog (current wire):** [progressive-lsp-apis.md](../progressive-lsp-apis.md)  
**Extended control RPC prose:** [user/progressive-v1-api.md](../user/progressive-v1-api.md)  
**Proto:** [`proto/progressive/v1/control.proto`](../../proto/progressive/v1/control.proto)

**Canonical doc package:** [serve-abs/README.md](../serve-abs/README.md) (requirements, design, checklist, agent-context).  
Fine-grained WPs: [serve-abs/implementation-checklist.md](../serve-abs/implementation-checklist.md) (mirror: [abstraction-checklist.md](abstraction-checklist.md)).

---

## Program phases (stack on `main` after TCACHE)

```text
main
  └── serve-abs-1   # ABS-1 readiness (ADR 002 B1–B5)
        └── serve-abs-2   # ABS-2 FileEventHub (ADR 003)
              └── serve-abs-3   # ABS-3 Tier registry + capabilities RPC
                    └── serve-abs-4   # ABS-4 TierPort + optional dynamic sort
```

| Phase | Milestone | Delivers | Depends on |
|-------|-----------|----------|------------|
| **ABS-1** | `serve-abs-1` | Honest mux `NotReady`; no sync engine on discover; `inflight` T3′; `CacheReady` proto + push; `ServeReadiness` FSM v1 | TCACHE on `main` |
| **ABS-2** | `serve-abs-2` | `FileEventHub` thread; live `WatchBackend`; subscribers (index, T3′, engine forward, journal); retire tree-scan as primary ghost path | ABS-1.3 (invalidation hooks) |
| **ABS-3** | `serve-abs-3` | `TierDescriptor`; `IndexStatus` or `TierCapabilities` extension; per-tier FSM in control; merge T3/T3′ naming in docs | ABS-1, ABS-2 |
| **ABS-4** | `serve-abs-4` | `TierPort` trait; tier crate boundary sketch; dynamic chain when engine missing | ABS-3 |

**Do not** block ABS-1 on ABS-4. **Do** block published **update latency** capabilities on ABS-2.

---

## Target A — N-tier readiness (detail)

See [n-tier-framework.md](n-tier-framework.md). Phase mapping:

| ADR 002 track | ABS-1 WP |
|---------------|----------|
| B1 Central FSM | ABS-1.1 |
| B2 Remove mux `EngineSupervisor::resolve` | ABS-1.2 |
| B3 `inflight` + NotReady | ABS-1.3 |
| B4 `CacheReady` push | ABS-1.4 |
| B5 Engine I/O on builder/warm threads only | ABS-1.5 |

Exit: integration harness asserts **control FSM** + discover `cache_state`; no new serve-side wall-clock discover timeouts.

---

## Target B — File event hub (detail)

**Problem:** Ghost disk uses `poll_disk_watch` (tree walk + mtime) on the control idle loop; `didChange` runs on mux; tiers share index generation ad hoc.

**Solution:** `FileEventHub` in `progressive-lsp-watch` (or adjacent crate):

```text
  OS notify / optional client feed
           │
           ▼
    [Hub thread: coalesce + generation]
           │
     ┌─────┼─────┬─────────────┐
     ▼     ▼     ▼             ▼
  Index  T3′   EngineFwd   Control journal
  subscriber subscribers   → WatchBatch push
```

| Component | Responsibility |
|-----------|----------------|
| Hub | Single `WatchBackend` (real notify); `WatchCoalescer`; monotonic **file generation** |
| `subscribe(SubscriberId, filter)` | Tier edit workers register; no per-tier OS handles |
| LSP `didChange` | **Enqueue** hub event (buffer generation), not only inline index on mux |
| Control | Hub subscriber writes journal + pending `WatchBatch` (replaces poll-driven diff) |

Exit: unit tests with `FakeWatcher`; serve integration test: modify file on disk → index + cache invalidation without `poll_disk_watch` tree scan.

---

## API evolution (serve-side)

| Area | ABS-1 | ABS-2 | ABS-3 |
|------|-------|-------|-------|
| LSP discover | `cache_state` in WAL; internal NotReady | unchanged | optional experimental not-ready cap |
| Control | `CacheReady` push | same | `TierCapabilities` or extended `IndexStatus` |
| Internal | `ServeReadiness` | `FileEventHub` | `TierRegistry` |

Update [progressive-lsp-apis.md](../progressive-lsp-apis.md) on each milestone sign-off.

---

## Agent workflow

Same hygiene as TCACHE ([types-cache/agent-context.md](../types-cache/agent-context.md)): branchless stack, 95% cov / 80% mutants on touched crates, no `sleep`, checklist row sign-off.

**Human gates:** Lang plugin crate splits (ABS-4); proto breaking changes (none planned — additive only).

---

## References

- [runtime-timeout-inventory.md](../spikes/runtime-timeout-inventory.md)
- [types-cache/requirements.md](../types-cache/requirements.md) FR-6, FR-7, REQ-NFR-1.4
