# Agent context policy (POC stack)

**Pointer-based context.** Every orchestrator, implementer, hygiene agent, and sub-orchestrator on `ide0`–`ide5` receives the **same** payload. Do not paste this tree or the pattern table into prompts. Do not grow context “because the parent had it.”

## Payload to pass (copy this block)

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  docs/poc-ide/README.md
  docs/poc-ide/architecture.md
  docs/poc-ide/third-party.md
  docs/poc-ide/agent-context.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/lsp-contract.md
  docs/user/progressive-v1-api.md
MILESTONE: <IDE-N>
BRANCH: <ideN>
PARENT_BRANCH: <main or ideN-1>
STEPS: the 8-step orchestrator loop in this file
Pass CONTEXT_POLICY + READ paths + MILESTONE + BRANCH to every child unchanged.
```

Children that only implement one WP also get that WP id from [../implementation-plan.md](../implementation-plan.md). They still get the same READ list (they will skip files they do not need).

## Orchestrator loop (every milestone)

1. **Last branch clean.** `git status` on `PARENT_BRANCH` (or current tip): working tree clean, last commit is the signed-off milestone. If dirty, stage and commit **only** if those files belong to the parent milestone; otherwise stop.
2. **Stack.** `git checkout PARENT_BRANCH && git checkout -b BRANCH`. Use git-branchless (`git sl`) to confirm the stack. Do not branch from `main` unless this is `ide0`.
3. **Implement** the milestone WPs (spawn children if needed). Scope is that milestone only.
4. **Pattern hygiene.** Every new type is a row in [../design-patterns.md](../design-patterns.md). No manager/helper/util layers. Invariant tests name the pattern.
5. **Validate patterns.** If a type has no pattern, refactor or delete it (may spawn a child). Do not leave Ad-hoc.
6. **Testing hygiene.** [../testing.md](../testing.md): 95% llvm-cov on crates that exist (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs` if that file exists); 80% mutants on listed crates that changed; no `thread::sleep`; `cargo test -- --test-threads=1`; `check-static` N/A for poc-ide. Darwin: do not fake musl greens.
7. **Docs.** Update [../milestones.md](../milestones.md) exit + sign-off, [../implementation-plan.md](../implementation-plan.md) WP rows, this tree if a locked decision moved, pattern table.
8. **Commit.** Stage only this milestone. Message style: `Land … so IDE-N can sign off without starting IDE-N+1.` Then `git sl`. Do not open `ideN+1` from this agent.

## Sign-off

The orchestrator does not start `ideN+1`. The parent parent (stack driver) starts the next orchestrator only when the sign-off checklist in milestones is checked.

## git-branchless

This repo uses **git-branchless** (`git sl`, `git next` / `git prev`). Stacked branches are ordinary `git checkout -b` children. Do not install Graphite. Do not `git config`. Do not `--no-verify`. Do not force-push `main`.
