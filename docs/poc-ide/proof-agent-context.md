# Agent context policy (POC-proof stack)

**Pointer-based context.** Every orchestrator, implementer, hygiene agent, and sub-orchestrator on `poc-proof-log` → `poc-lsp-async` → `poc-tier-status` → `poc-no-stall` receives the **same** payload. Do not paste this tree or the pattern table into prompts. Do not grow context “because the parent had it.”

There is no `log12`. These are POC-proof slices on current `main` (after the log11 merge). Do not reopen LOG-0–LOG-11. Do not merge `RunLog` with the serve WAL.

## Payload to pass (copy this block)

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  /Users/inoahsmi/.cursor/plans/close_poc_proof_gaps_9beafd00.plan.md
  docs/poc-ide/README.md
  docs/poc-ide/architecture.md
  docs/poc-ide/third-party.md
  docs/poc-ide/agent-context.md
  docs/poc-ide/proof-agent-context.md
  docs/logging.md
  docs/logging-plan.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/lsp-contract.md
  docs/user/progressive-v1-api.md
  docs/user/README.md
  docs/language-matrix.md
MILESTONE: <poc-proof-log | poc-lsp-async | poc-tier-status | poc-no-stall>
BRANCH: <same as MILESTONE>
PARENT_BRANCH: <main | poc-proof-log | poc-lsp-async | poc-tier-status>
STEPS: the 8-step orchestrator loop in this file
Pass CONTEXT_POLICY + READ paths + MILESTONE + BRANCH to every child unchanged.
```

Children that only implement one WP also get that WP id from [../implementation-plan.md](../implementation-plan.md). They still get the same READ list (they will skip files they do not need).

## Orchestrator loop (every milestone)

1. **Last branch clean.** `git checkout` the parent. Working tree must be clean. Last commit is the signed-off parent (for `poc-proof-log`, parent is current `main` after log11 merge, commit `a0f10a2`). If dirty, stop and report.
2. **Stack.** `git checkout PARENT_BRANCH && git checkout -b BRANCH`. Confirm with `git sl`. This repo uses **git-branchless**. Do not stack `poc-proof-log` on `log11` or `poc-discover-log` history — only on current `main`. Do not install Graphite.
3. **Implement** that milestone’s WPs only (spawn children if needed). `poc-no-stall` is the last POC-proof slice — do not open a follow-on branch.
4. **Pattern hygiene.** Every new type is a row in [../design-patterns.md](../design-patterns.md). No manager/helper/util layers. Invariant tests name the pattern.
5. **Validate patterns.** If a type has no pattern, refactor or delete it. Do not leave Ad-hoc.
6. **Testing hygiene.** [../testing.md](../testing.md): 95% llvm-cov on crates that exist (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`); 80% mutants on listed crates that changed; no `thread::sleep`; `cargo test -- --test-threads=1` (or crate-scoped plus composition-root if the workspace is too heavy — say what you ran). Darwin: do not fake musl greens. `check-static` N/A unless the musl ELF story changed.
7. **Docs.** Sign off in [../milestones.md](../milestones.md). Add WP rows in [../implementation-plan.md](../implementation-plan.md). Keep the stack diagram in [../branching.md](../branching.md). `RunLog` stays a **separate schema** from the server WAL.
8. **Commit.** Stage only this milestone. Message style: `Land … so <BRANCH> can sign off without starting <next>.` Then `git sl`. Do not open the next branch. Do not create a PR.

## Slice scope (do not cross)

| Branch | In | Out |
|---|---|---|
| `poc-proof-log` | Empty F12 **info** on serve; `PROGRESSIVE_LSP_LOG_LEVEL`; poc-ide debug spawn, stderr→RunLog, `xtask poc`, footer paths | LSP reader thread; IndexStatus fields; DiscoverOffer; highlight cache |
| `poc-lsp-async` | LSP/control IO threads; Discover/didChange never block `fn ui`; keep `$/progress` | IndexStatus ingest fields; DiscoverOffer menus |
| `poc-tier-status` | Additive ingest/tier status; T1/T2/T3 strip; context menu from LanguageCatalog × current tier | Highlight cache; tree-expand worker |
| `poc-no-stall` | Highlight cache; tree expand worker; disable Navigate/F12 until Ready | PackAdapter `Command` spawn; merging RunLog with WAL |

## Sign-off

The orchestrator does not start the next branch. The parent (stack driver) starts the next orchestrator only when the sign-off checklist in milestones is checked.

## git-branchless

This repo uses **git-branchless** (`git sl`, `git next` / `git prev`). Stacked branches are ordinary `git checkout -b` children. The user may say “stackless”; they mean git-branchless. Do not install Graphite. Do not `git config`. Do not `--no-verify`. Do not force-push `main`.
