# Types cache — design

Architecture for **T3′**: indexed cache overlay on the T3 substrate. Requirements: [requirements.md](requirements.md).

---

## Problem

T3 engines (Java/javacs, etc.) are **inherently slow** (JVM, project model, project-wide references). Blocking the mux thread on engine RPC violates interactive discover. Progressive intent: **serve fast answers from cache**; **fill cache in background** from T3; **fall back** to T2/T1 on miss.

---

## High-level data flow

```text
                    ┌─────────────────────────────────────┐
  mux thread        │  ResolverChain (discover)           │
  (one click)       │  1. TypesCacheResolver   (T3′ read) │
                    │  2. HeuristicResolver    (T2)       │
                    │  3. TreeSitterResolver   (T1)       │
                    └──────────────┬──────────────────────┘
                                   │ miss @ T3′
                                   ▼
                    ┌─────────────────────────────────────┐
  background        │  TypesCacheBuilder                  │
                    │  → EngineSupervisor.resolve (T3)    │
                    │  → TypesCacheStore.write            │
                    └─────────────────────────────────────┘
```

**EngineResolver is not a chain step** after cutover; it is **only** the builder’s adapter to the pack child.

---

## Storage model (two layers)

### Layer A — Query cache (ship first)

Point lookup of LSP-shaped answers.

| Field | Purpose |
|-------|---------|
| Key | `(QueryKind, FileId, Position, CacheGeneration)` + optional `PackageId` |
| Value | `ResolveResult` (locations, `tier=Types`, metadata) |

Metadata (recommended):

- `filled_at_unix_ms`
- `source`: `engine` | `promoted` (if ever merged from T2)
- `engine_generation`: invalidates when child restarts

**Read algorithm (≤ 20 ms):**

1. Compute generation for `q.file` from shared index/dirty state.
2. Lookup key; if hit and generation matches → `Ready`.
3. If in-flight builder for key → `NotReady`.
4. Else → `NotReady` + enqueue builder (if engine ready and doc open policy satisfied).

### Layer B — Relation graph (phase 2)

Structural facts T3 repeatedly recomputes. Stored as typed edges/nodes, language-agnostic **schema**, language-specific **population** via builder.

| Query kind | Graph content (examples) |
|------------|---------------------------|
| Definition | Binding site → declaration; type use → type decl |
| TypeDefinition | Use → type decl (type-only filter) |
| Implementation | Supertype/interface → implementor decls |
| References | Symbol identity → use sites (not name-only collision) |

**Read algorithm:** optional second step inside T3′ if query cache misses but graph can answer within liveness budget; otherwise `NotReady`.

Population: builder parses engine `Location`/`LocationLink` responses into edges; future: incremental graph merge per file.

---

## Invalidation

Single **generation source** aligned with `IndexService`:

| Event | Action |
|-------|--------|
| File dirty / `didChange` | Bump file generation; drop query keys for file; mark graph incident edges stale |
| Watch batch | Same via existing dirty paths |
| Package ingest completes (T2) | Optional: prioritize builder warm for that package |
| Engine abort/restart | Bump `engine_generation`; all `source=engine` entries stale |

**Priority warm:** use existing `PriorityIndex` — focused buffer, imports closure, then breadth.

---

## Crate / module placement (target)

| Component | Suggested home | Pattern |
|-----------|----------------|---------|
| `TypesCacheStore` | `progressive-lsp-types-cache` (new crate) | Repository + Facade |
| `TypesCacheResolver` | same | Chain step |
| `TypesCacheBuilder` | same | Command + background worker |
| `InvalidationPolicy` | same | Strategy |
| `CacheGeneration` | same | Value object |
| Wiring | `WorkspaceSession` | Facade composes chain |
| Engine RPC | `progressive-lsp-engine` | Adapter (builder only) |

Dependencies: `progressive-lsp-resolve` (query types), `progressive-lsp-core` (Tier, FileId), index facade for generation (port, not circular imports into session).

---

## Chain integration

Today (`WorkspaceSession`): T3 `EngineResolver` prepended when supervisor attached.

Target:

```text
TypesCacheResolver::new(store, builder_handle)
HeuristicResolver (T2)
TreeSitterResolver (T1)
```

`TypesCacheBuilder` holds `Arc<EngineSupervisor>` and writes to `Arc<TypesCacheStore>`.

Document sync: builder only runs `resolve_query` when URI is in engine **opened** set (inbox drain completed) — same rule as today’s `lsp_child` fix; never `didOpen` empty body on builder path. Implementation: [`IndexOpenPort`](../../progressive-lsp-types-cache/src/engine_builder.rs) checks `IndexService::is_open` before enqueueing engine RPC (TC-3.3).

---

## Concurrency (single user)

One interactive discover at a time is sufficient. Builder still needs:

- **Single-flight** map: `Key → in_flight` to coalesce duplicate misses.
- **Store mutex** or sharded locks: read path must stay ≤ 20 ms.

No requirement for parallel engine RPCs from one user gesture.

---

## Control plane (optional)

Proto extension or reuse `TierReady`-style push:

- `TypesCacheReady { package_id, generation }` or per-key notification (heavier).

IDE: optional re-request discover on push; must not block UI.

---

## Migration / cutover

1. Land crate + resolver with feature flag or empty store (always `NotReady`).
2. Enable builder async fill; measure WAL `cache_state=hit`.
3. Remove `EngineResolver` from chain on dogfood flavor.
4. Delete request-path timeout constants in engine resolve.

---

## Testing hooks

- `FakeTypesCacheStore` / `FakeBuilder` for chain tests.
- `FakeEngine` already exists for supervisor tests — builder uses it in unit tests.
- Integration: golden JSON + max `resolve_ms` — [implementation-checklist.md](implementation-checklist.md) TC-5.

See [design-patterns-addendum.md](design-patterns-addendum.md) for named patterns.
