# Agent context policy (TCACHE stack)

**Pointer-based context.** Meta-orchestrator → phase orchestrator → milestone agents share the **same** READ list and hygiene rules. Pass this file **unchanged** to children.

**Human gate:** Do not refactor `progressive-lsp-lang-*` / pack boundaries without sign-off ([productization/agent-context.md](../productization/agent-context.md)).

Hygiene: [testing.md](../testing.md), [branching.md](../branching.md).

---

## Meta-orchestrator (orchestrator of orchestrators)

**Role:** Run **TCACHE phases in order** (TC-0 → TC-5). Verify phase exit before spawning the next. Do not start implementation crates until **TC-0** and **TC-1** are signed off.

**Loop:**

1. Confirm parent branch clean (no stash; commit only when human asked or phase delegated).
2. Read [implementation-checklist.md](implementation-checklist.md) for current phase.
3. Spawn **phase orchestrator** with payload below.
4. Verify phase exit table before next phase.
5. Do not land runtime code on doc-only phases.

### Meta payload

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  docs/types-cache/agent-context.md
  docs/types-cache/requirements.md
  docs/types-cache/design.md
  docs/types-cache/design-patterns-addendum.md
  docs/types-cache/implementation-checklist.md
  docs/adr/001-types-cache-overlay.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/productization/agent-context.md
  docs/spikes/design-pattern-audit.md
ROLE: meta-orchestrator (TCACHE)
PHASE_ORDER: TC-0 → TC-1 → TC-2 → TC-3 → TC-4 → TC-5
HUMAN_GATES: lang plugin refactor; GitHub/release; long docker dogfood builds
Pass phase payload to each phase orchestrator unchanged except PHASE and WP_IDS.
```

---

## Phase orchestrator

**Role:** Execute one phase from [implementation-checklist.md](implementation-checklist.md). Spawn **milestone agents** per WP row (one WP per agent unless tightly coupled).

### Phase payload

```text
REPO: <same>
CONTEXT_POLICY: pointer
READ: (same list as meta)
ROLE: phase orchestrator
PHASE: TC-N
BRANCH: tcache-N  (or stacked child per implementation-plan.md)
PARENT_BRANCH: <tcache-N-1 or main per stack>
WP_IDS: <comma-separated from checklist, e.g. TC-2.1,TC-2.2>
STEPS: orchestrator loop below
Pass MILESTONE + WP_ID + READ to each milestone agent.
```

---

## Milestone agent (single WP)

```text
REPO: <same>
CONTEXT_POLICY: pointer
READ: (same list)
ROLE: implementer
MILESTONE: TC-N
WP_ID: TC-N.M
BRANCH: tcache-N
SCOPE: only this WP; no drive-by refactors outside spike-delivered list without TC-4
```

---

## Orchestrator loop (every phase)

1. **Clean parent.** `git status` on `PARENT_BRANCH`.
2. **Stack.** `git checkout PARENT_BRANCH && git checkout -b BRANCH`. git-branchless (`git sl`). No Graphite; no `git config`; no `--no-verify`; no force-push `main`.
3. **Implement** WP scope only.
4. **Pattern hygiene.** New types → [design-patterns.md](../design-patterns.md) + [design-patterns-addendum.md](design-patterns-addendum.md).
5. **Testing.** [testing.md](../testing.md); **timing assertions** where WP requires (TC-5 mandatory).
6. **Docs.** Update [milestones.md](../milestones.md), [implementation-plan.md](../implementation-plan.md), checklist checkboxes.
7. **Commit** only when human requested or phase delegation says so.

---

## Sign-off

Phase orchestrator does not start TC-(N+1). Meta-orchestrator verifies milestones sign-off checklist in [milestones.md](../milestones.md) (TCACHE section).
