# Agent context policy (SERVE-ABS stack)

**Pointer-based context.** Meta-orchestrator → phase orchestrator → milestone agents share the **same** READ list and hygiene rules. Pass this file **unchanged** to children.

**Prerequisite:** TCACHE **TCACHE-5** signed off on `main`.

**Human gates:** `progressive-lsp-lang-*` crate splits (ABS-4); proto breaking changes (none planned — additive only); long docker dogfood builds.

Hygiene: [testing.md](../testing.md), [branching.md](../branching.md).

---

## Meta-orchestrator

**Role:** Run **ABS phases in order** (ABS-0 → ABS-4). Verify phase exit in [implementation-checklist.md](implementation-checklist.md) before spawning the next. **ABS-0** is docs-only; **ABS-1+** stack branches per [README.md](README.md).

**Loop:**

1. Confirm parent branch clean (no stash; commit only when human asked or phase delegated).
2. Read [implementation-checklist.md](implementation-checklist.md) for current phase.
3. Spawn **phase orchestrator** with payload below.
4. Verify phase exit table + [milestones.md](../milestones.md) sign-off before next phase.
5. Do not land runtime refactors on ABS-0 without human ack.

### Meta payload

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  docs/serve-abs/agent-context.md
  docs/serve-abs/requirements.md
  docs/serve-abs/design.md
  docs/serve-abs/implementation-checklist.md
  docs/serve-abs/README.md
  docs/architecture/abstraction-targets-plan.md
  docs/architecture/n-tier-framework.md
  docs/adr/002-serve-readiness-fsm.md
  docs/adr/003-file-event-hub.md
  docs/types-cache/requirements.md
  docs/progressive-lsp-apis.md
  docs/spikes/runtime-timeout-inventory.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/productization/agent-context.md
ROLE: meta-orchestrator (SERVE-ABS)
PHASE_ORDER: ABS-0 → ABS-1 → ABS-2 → ABS-3 → ABS-4
HUMAN_GATES: lang plugin refactor; GitHub/release; docker dogfood
Pass phase payload to each phase orchestrator unchanged except PHASE and WP_IDS.
```

---

## Phase orchestrator

**Role:** Complete all WPs in one phase on the correct stack branch; update checklist `[x]` rows; request milestone sign-off.

**Mandatory order (every ABS-N agent):**

1. Branch from stack parent ([branching.md](../branching.md)).
2. Implement only WPs in scope.
3. `cargo test` on touched crates; integration rows from checklist IT-ABS-* when applicable.
4. Coverage / mutants on touched crates (≥95% / ≥80% where tools exist).
5. No `sleep` for product semantics in tests — use control FSM / fakes.
6. Update [implementation-checklist.md](implementation-checklist.md) Done column.
7. Commit when human asked; do not amend failed hooks.

### Phase payload template

```text
REPO: <absolute path>
CONTEXT_POLICY: pointer
READ: (same as meta READ)
ROLE: phase-orchestrator (SERVE-ABS)
PHASE: ABS-<N>
BRANCH: serve-abs-<N>   # ABS-0 uses main, docs only
WP_IDS: ABS-<N>.*
EXIT: implementation-checklist.md Phase ABS-<N> exit criteria
```

---

## Milestone agent (single WP or whole phase)

```text
REPO: <same>
CONTEXT_POLICY: pointer
READ: (same list)
ROLE: milestone agent
MILESTONE: ABS-N
WP_IDS: <from implementation-checklist.md>
PARENT_BRANCH: <tip of prior stacked branch>
BRANCH: serve-abs-N   # ABS-0: main, docs only
SCOPE: only this milestone; no lang-* crate splits without human gate (ABS-4)
STEPS: milestone workflow below (mandatory, in order)
```

**Branch stack (git-branchless):** stacked on **`main`** after TCACHE merge (`e68900c` or later).

```text
main
  └── serve-abs-1
        └── serve-abs-2
              └── serve-abs-3
                    └── serve-abs-4
```

Use **`git sl`**; no Graphite. No `git config`; no `--no-verify`; no force-push `main`; no stash.

---

## Milestone workflow (every ABS-N agent — mandatory order)

1. **Commit prior work on the parent branch.** Parent tip clean before stacking.
2. **Create stacked branch.** `git checkout PARENT && git checkout -b serve-abs-N`. Confirm with `git sl`.
3. **Implement** WPs in [implementation-checklist.md](implementation-checklist.md) only. New types → [design-patterns.md](../design-patterns.md).
4. **Tests / cov / mutants** per [testing.md](../testing.md) on touched crates. **No `sleep`** for serve tier semantics. `CARGO_TARGET_DIR=$PWD/target` for cargo in this repo.
5. **Update** checklist + [milestones.md](../milestones.md) SERVE-ABS section.
6. **Commit** milestone files. Message style: `Land … so ABS-N can sign off.`
7. **Sign off** ABS-N checklist in milestones.md. Do not open `serve-abs-N+1` in the same agent turn unless you are the meta-orchestrator.

**Shell:** NEVER `required_permissions: ["all"]` for `cargo` / `xtask` / routine docker ([yolo-workspace.mdc](../../.cursor/rules/yolo-workspace.mdc)). Use default sandbox + `network` / `git_write` as needed.

Doc-only **ABS-0**: step 4 N/A; step 6 still applies.

---

## Phase orchestrator loop

1. Verify parent clean and prior ABS milestone signed off.
2. Spawn milestone agent(s) with payload above.
3. Verify steps 1–7 before phase exit.
4. Do not start ABS-(N+1) until sign-off checklist checked.

---

## Meta-orchestrator sign-off

Verify ABS-N sign-off in [milestones.md](../milestones.md) before spawning the next phase. **ABS-0** may complete on `main` (docs). First implementation branch: **`serve-abs-1`**.

---

## Milestone agent (single WP)

Use when splitting a large phase. Inherit READ list; scope **one** WP row; do not start dependent WPs until dependency signed off in checklist.
