# Agent context policy (LOG stack)

**Pointer-based context.** Every orchestrator, implementer, hygiene agent, and sub-orchestrator on `log0`–`log11` receives the **same** payload. Do not paste this tree or the pattern table into prompts. Do not grow context “because the parent had it.” Stack complete at `log11`. There is no `log12`.

## Payload to pass (copy this block)

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  docs/logging.md
  docs/logging-plan.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/architecture.md
  docs/detailed-design.md
  docs/host-deps.md
  docs/README.md
  docs/poc-ide/third-party.md
  docs/poc-ide/agent-context.md
  docs/user/README.md
MILESTONE: <LOG-N>
BRANCH: <logN>
PARENT_BRANCH: <main or logN-1>
STEPS: the 8-step orchestrator loop in this file
Pass CONTEXT_POLICY + READ paths + MILESTONE + BRANCH to every child unchanged.
```

Children that only implement one WP also get that WP id from [../implementation-plan.md](../implementation-plan.md). They still get the same READ list (they will skip files they do not need).

## Orchestrator loop (every milestone)

1. **Last branch clean.** `git status` on `PARENT_BRANCH` (or current tip): working tree clean, last commit is the signed-off milestone. If dirty, stage and commit **only** if those files belong to the parent milestone; otherwise stop. `log0` / `log5` may keep untracked docs that **are** this milestone.
2. **Stack.** `git checkout PARENT_BRANCH && git checkout -b BRANCH`. Use git-branchless (`git sl`) to confirm the stack. Do not branch from `main` unless this is `log0`. Do **not** stack on `poc-no-console`. Parent of `log0` is **current `main`** (PR #4 / `poc-discover-log` already merged). Parent of `log5` is **`log4`**.
3. **Implement** the milestone WPs (spawn children if needed). Scope is that milestone only. `log0` and `log5` are documentation ingest: no crates, no rusqlite changes, no `eprintln!` changes. Do not start `LogPort` on `log0`. Do not start supervisor `with_log` on `log5`. LOG-0–LOG-5 stay signed off; do not reopen them.
4. **Pattern hygiene.** Every new type is a row in [../design-patterns.md](../design-patterns.md). No manager/helper/util layers. Invariant tests name the pattern. On `log0`/`log5`, draft types from [../logging.md](../logging.md) as table rows.
5. **Validate patterns.** If a type has no pattern, refactor or delete it (may spawn a child). Do not leave Ad-hoc. If you cannot name it, do not invent the type.
6. **Testing hygiene.** [../testing.md](../testing.md): 95% llvm-cov on crates that exist (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`); 80% mutants on listed crates that changed, including `progressive-lsp-log` (LOG-2+); no `thread::sleep`; `cargo test -- --test-threads=1`; `check-static` on musl ELFs after rusqlite is linked (LOG-2+). `log0` / `log5`: tests / llvm-cov / mutants / `sleep` / `check-static` are **N/A** (docs only). Darwin: do not fake musl greens.
7. **Docs.** Update [../milestones.md](../milestones.md) exit + sign-off, [../implementation-plan.md](../implementation-plan.md) WP rows, this tree if a locked decision moved, pattern table. `poc-ide` `RunLog` stays a separate schema — do not merge with the server WAL.
8. **Commit.** Stage only this milestone. Message style: `Land … so LOG-N can sign off without starting LOG-N+1.` Then `git sl`. Do not open `logN+1` from this agent.

## Sign-off

The orchestrator does not start `logN+1`. The parent parent (stack driver) starts the next orchestrator only when the sign-off checklist in milestones is checked.

## git-branchless

This repo uses **git-branchless** (`git sl`, `git next` / `git prev`). Stacked branches are ordinary `git checkout -b` children. Do not install Graphite. Do not `git config`. Do not `--no-verify`. Do not force-push `main`.
