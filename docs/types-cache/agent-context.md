# Agent context policy (TCACHE stack)

**Pointer-based context.** Meta-orchestrator → phase orchestrator → milestone agents share the **same** READ list and hygiene rules. Pass this file **unchanged** to children.

**Human gate:** Do not refactor `progressive-lsp-lang-*` / pack boundaries without sign-off ([productization/agent-context.md](../productization/agent-context.md)).

Hygiene: [testing.md](../testing.md), [branching.md](../branching.md).

---

## Meta-orchestrator (orchestrator of orchestrators)

**Role:** Run **TCACHE phases in order** (TC-0 → TC-5). **TCACHE-0 is signed off** on `pre-tcache-base`. Verify phase exit before spawning the next. Do not start **TCACHE-2** implementation crates until **TCACHE-1** is signed off.

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

## Milestone agent (single WP or whole phase)

```text
REPO: <same>
CONTEXT_POLICY: pointer
READ: (same list)
ROLE: milestone agent
MILESTONE: TCACHE-N  (maps to phase TC-N)
WP_IDS: <from implementation-checklist.md>
PARENT_BRANCH: <tip of prior stacked branch — see stack below>
BRANCH: tcache-N
BASE_WORKING_BRANCH: pre-tcache-base
SCOPE: only this milestone; no drive-by refactors outside TC-4 audit list
STEPS: milestone workflow below (mandatory, in order)
```

**Branch stack (git-branchless):** stacked on **`pre-tcache-base`** (dogfood discover + TCACHE docs). Do not stack TCACHE on `main` until human merges the base branch.

```text
pre-tcache-base     # base working branch (committed local work before TCACHE)
  └── tcache-0      # TCACHE-0 — signed off (docs only, already on base)
        └── tcache-1
              └── tcache-2
                    └── tcache-3
                          └── tcache-4
                                └── tcache-5
```

Use **`git sl`** / **`git branchless`** for navigation; no Graphite. No `git config`; no `--no-verify`; no force-push `main`; no stash.

---

## Milestone workflow (every TCACHE-N agent — mandatory order)

Each milestone agent **must** complete all seven steps before reporting sign-off to the phase orchestrator.

1. **Commit prior work on the parent branch.** On `PARENT_BRANCH`, run `git status`. If anything belongs to an **earlier** milestone, commit it on that branch first (or finish the prior milestone agent). The parent tip must be **clean** before stacking.
2. **Create stacked branch.** `git checkout PARENT_BRANCH && git checkout -b tcache-N` (name from [implementation-plan.md](../implementation-plan.md)). Confirm with `git sl`.
3. **Do the milestone work.** Implement only WPs for this milestone ([implementation-checklist.md](implementation-checklist.md)). Pattern hygiene: new types → [design-patterns.md](../design-patterns.md) + [design-patterns-addendum.md](design-patterns-addendum.md).
4. **Coverage and mutation.** Per [testing.md](../testing.md): **≥ 95% line coverage** (llvm-cov) on crates touched; **≥ 80% mutation kill** on listed crates that changed. No `thread::sleep` in tests. Timing assertions where the WP requires (TCACHE-5 mandatory). `check-static` on musl ELFs if this milestone changes shipped binaries.
5. **Update checklist docs.** Mark WPs done in [implementation-checklist.md](implementation-checklist.md); update [milestones.md](../milestones.md) (TCACHE section) and [implementation-plan.md](../implementation-plan.md) WP rows as needed.
6. **Commit on the branch.** Stage **only** this milestone’s files. One or more commits with message style: `Land … so TCACHE-N can sign off without starting TCACHE-N+1.` Run `git sl`.
7. **Sign off.** Complete the TCACHE-N sign-off checklist in [milestones.md](../milestones.md). Do **not** create `tcache-N+1` in the same agent turn unless you are the meta-orchestrator spawning the next milestone agent.

Doc-only milestones (TCACHE-0, TCACHE-1): step 4 is **N/A** (note N/A in sign-off). Step 6 still applies.

---

## Phase orchestrator loop

1. Verify **parent branch clean** and prior milestone signed off.
2. Spawn **milestone agent(s)** with payload above (`PARENT_BRANCH`, `BRANCH`, `WP_IDS`, full READ list).
3. Verify milestone agent completed **steps 1–7** before declaring phase exit.
4. Do not start TC-(N+1) until TCACHE-N sign-off checklist is checked.

---

## Meta-orchestrator sign-off

Meta-orchestrator verifies TCACHE-N sign-off in [milestones.md](../milestones.md) before spawning the next phase. First implementation milestone after base: **TCACHE-1** (`tcache-1` stacked on `pre-tcache-base`; TCACHE-0 treated as signed off on base).
