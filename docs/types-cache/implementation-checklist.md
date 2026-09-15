# Types cache — implementation checklist

Fine-grained work packages for **TCACHE** orchestration. Depends-on is strict: do not start a WP until dependencies are signed off.

**Base branch:** `pre-tcache-base`. **Milestone agents:** mandatory [seven-step workflow](agent-context.md#milestone-workflow-every-tcache-n-agent--mandatory-order) (branchless stack → work → ≥95% cov / ≥80% mutants → update this file → commit → sign off).

**Branch stack:** see [implementation-plan.md](../implementation-plan.md) (TCACHE section).

**Sign-off checklist (every milestone):** copy from [milestones.md](../milestones.md) TCACHE section and [implementation-plan.md](../implementation-plan.md) (tests, cov, mutants, no sleep, check-static if ELF, design-patterns, docs).

---

## Phase TC-0 — Documentation & ADR (docs only)

**Status: SIGNED OFF** on `pre-tcache-base` (no separate `tcache-0` branch).

**Exit:** Requirements and design accepted; agent-context and spike brief on disk; no crate changes.

| ID | Work package | Depends-on | Exit criteria |
|----|--------------|------------|---------------|
| TC-0.1 | [requirements.md](requirements.md) normative FR/NFR | — | Reviewed; IDs stable |
| TC-0.2 | [design.md](design.md) architecture | TC-0.1 | Chain + store + invalidation described |
| TC-0.3 | [ADR 001](../adr/001-types-cache-overlay.md) accepted | TC-0.1, TC-0.2 | Status Accepted |
| TC-0.4 | [agent-context.md](agent-context.md) orchestrator payloads | TC-0.3 | Meta + phase + milestone blocks |
| TC-0.5 | [design-patterns-addendum.md](design-patterns-addendum.md) | TC-0.2 | All planned types named |
| TC-0.6 | Replace misleading discover text in [productization/agent-context.md](../productization/agent-context.md) with pointers to types-cache | TC-0.1 | No “T1 as cache” wording |

**Phase exit:** TC-0.1–TC-0.6 checked; milestones TCACHE-0 signed off.

---

## Phase TC-1 — Design pattern audit spike (docs + inventory)

**Status: SIGNED OFF** on `tcache-1` (2026-09-14).

**Exit:** Repo-wide pattern/refactor backlog without mandatory code changes.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| TC-1.1 | Run [design-pattern-audit spike](../spikes/design-pattern-audit.md) | TC-0 | Output file populated | [x] |
| TC-1.2 | Publish `docs/types-cache/pattern-audit-report.md` from spike | TC-1.1 | Every workspace crate listed; each module mapped or flagged ad-hoc | [x] |
| TC-1.3 | Prioritize refactor candidates (P0/P1/P2) for TC-4 | TC-1.2 | Table with file paths + pattern target | [x] |
| TC-1.4 | Update [design-patterns.md](../design-patterns.md) gap list (missing types in code) | TC-1.2 | Appendix or linked section (report §6; main table deferred to TC-2) | [x] |

**Phase exit:** TC-1.2 report merged; TC-4 scope agreed (6 P0 items — human ack on report before TC-4.1).

---

## Phase TC-2 — Crate skeleton & ports

**Status: SIGNED OFF** on `tcache-2` (2026-09-14).

**Exit:** `progressive-lsp-types-cache` exists; store/resolver/builder stubs; unit tests green; always `NotReady` on read.

| ID | Work package | Depends-on | Exit criteria | Done |
|----|--------------|------------|---------------|------|
| TC-2.1 | Add crate `progressive-lsp-types-cache` to workspace | TC-0 | `cargo test -p progressive-lsp-types-cache` | [x] |
| TC-2.2 | `TypesCacheKey`, `CacheGeneration`, `TypesCacheEntry` | TC-2.1 | Value object tests | [x] |
| TC-2.3 | `TypesCacheStore` in-memory get/put/invalidate | TC-2.2 | Generation mismatch → miss | [x] |
| TC-2.4 | `TypesCacheResolver` chain step (stub miss) | TC-2.3 | Chain test: falls through to fake T2 | [x] |
| TC-2.5 | `TypesCacheBuilder` trait + `FakeBuilder` / queue | TC-2.3 | Enqueue on miss (hook only) | [x] |
| TC-2.6 | `InvalidationPolicy` wired to generation port | TC-2.3 | didChange fixture bumps gen | [x] |
| TC-2.7 | Pattern table rows in design-patterns.md | TC-2.1–TC-2.6 | Names match addendum | [x] |

**Phase exit:** Resolver in chain behind `types-cache-chain` feature + empty store; no engine calls.

---

## Phase TC-3 — Builder + engine adapter + session wiring

**Exit:** Background fill from fake/real engine; second click cache hit; engine off mux chain (feature flag OK).

| ID | Work package | Depends-on | Exit criteria |
|----|--------------|------------|---------------|
| TC-3.1 | `TypesCacheBuilder` calls `EngineSupervisor::resolve` off-thread | TC-2.5 | Single-flight test |
| TC-3.2 | Remove `EngineResolver` from hot `ResolverChain` in `WorkspaceSession` | TC-3.1 | Chain order T3′→T2→T1 |
| TC-3.3 | Document open gating for builder (no empty didOpen) | TC-3.1 | Matches lsp_child policy |
| TC-3.4 | Serve WAL: `cache_state` on discover rows | TC-3.2 | Info extras documented |
| TC-3.5 | Delete request-path try budgets in `EngineResolver` (builder only) | TC-3.1 | REQ-NFR-1.3 |
| TC-3.6 | Integration: discover-java container **second** definition hit | TC-3.2 | `tier=types`, timing in trace |

**Phase exit:** TC-3.6 pass on Linux container harness.

---

## Phase TC-4 — Refactor pass (from audit)

**Exit:** P0 audit items resolved or explicitly deferred with ADR note.

| ID | Work package | Depends-on | Exit criteria |
|----|--------------|------------|---------------|
| TC-4.1 | Execute P0 refactors from pattern-audit-report | TC-1.3, TC-3 | No new ad-hoc layers in touched files |
| TC-4.2 | Thin `ServeHost` / `WorkspaceSession` boundaries per audit | TC-4.1 | discover path documented in design-patterns |
| TC-4.3 | Consolidate duplicated identifier/resolve helpers (if audit lists) | TC-4.1 | Single module or port |
| TC-4.4 | Deferred P1/P2 logged in audit report | TC-4.1 | Status column updated |

**Phase exit:** TC-4.1 complete or waived by human in ADR.

---

## Phase TC-5 — Relation graph + timing CI

**Exit:** References/implementations benefit from graph where specified; CI enforces liveness.

| ID | Work package | Depends-on | Exit criteria |
|----|--------------|------------|---------------|
| TC-5.1 | Graph edge write path in builder (from engine responses) | TC-3 | Unit: def/ref edges stored |
| TC-5.2 | T3′ graph read path (optional fast path) within 20 ms | TC-5.1 | Unit timing bound |
| TC-5.3 | Invalidation for graph slices | TC-5.1 | File dirty clears edges |
| TC-5.4 | Harness: `IT-discover-timing` + golden JSON | TC-3 | `resolve_ms` p99 ≤ 20 ms on fixture (mux path) |
| TC-5.5 | Harness: references golden + cache hit after warm | TC-5.1 | location_count > 0 when engine would |
| TC-5.6 | Unit tests: all T3′/T2/T1 chain steps assert ≤ 20 ms on small fixtures | TC-5.4 | Documented exceptions if any |

**Phase exit:** TC-5.4 required for sign-off; TC-5.5 for Java dogfood.

---

## Orchestrator quick reference

| Phase | Branch | Spawn |
|-------|--------|--------|
| TC-0 | `tcache-0` | 1 doc agent or single orchestrator |
| TC-1 | `tcache-1` | 1 spike agent (background OK) |
| TC-2 | `tcache-2` | WPs TC-2.1–TC-2.7 parallelizable after TC-2.1 |
| TC-3 | `tcache-3` | Sequential TC-3.1 → TC-3.2 → … |
| TC-4 | `tcache-4` | One agent per P0 file cluster |
| TC-5 | `tcache-5` | TC-5.4 last |

**Meta-orchestrator:** [agent-context.md](agent-context.md).
