# Agent context policy (SEAMS + IT-TAM)

**Pointer-based context.** Meta-orchestrator → phase agents share this READ list.

**Base branch:** `serve-abs-4` @ SERVE-ABS complete (or `main` after merge).

**Hygiene:** [testing.md](../../docs/testing.md), [branching.md](../../docs/branching.md), `.cursor/rules/yolo-workspace.mdc` — **never** `required_permissions: ["all"]` for cargo/xtask; **`CARGO_TARGET_DIR=$PWD/target`**.

**Human gates:** Proto breaking changes (additive only); no `progressive-lsp-lang-*` splits.

---

## Meta payload

```text
REPO: <absolute path to progressive-lsp>
CONTEXT_POLICY: pointer
READ:
  integration/tier-api-matrix/agent-context.md
  integration/tier-api-matrix/integ-seams-plan.md
  integration/tier-api-matrix/README.md
  docs/adr/004-progressive-result-meta-and-trace.md
  docs/progressive-lsp-apis.md
  docs/lsp-contract.md
  docs/control-protocol.md
  docs/testing.md
  docs/branching.md
ROLE: meta-orchestrator (SEAMS + IT-TAM)
PHASE_ORDER: SEAMS-0 → SEAMS-1 → SEAMS-2 → SEAMS-3 → SEAMS-4 → SEAMS-5 → SEAMS-6 → TAM-1 → TAM-2 (stop after TAM-2 or hard blocker)
```

## Branch stack (git-branchless)

```text
serve-abs-4
  └── integ-seams-1    # SEAMS-1 chain policy + unit tests
        └── integ-seams-2   # SEAMS-2 LSP extended results
              └── integ-seams-3   # SEAMS-3 FetchTrace + ProgressiveMeta control
                    └── integ-tam-1   # TAM-1 loader
                          └── integ-tam-2   # TAM-2 container runner (minimal)
```

## Milestone workflow (mandatory)

1. Clean parent; commit doc WIP on parent if needed.
2. `git checkout -b <branch>` from stack parent; `git sl`.
3. Implement phase scope only; unit tests green; meta **off** preserves existing behavior.
4. Update integ-seams-plan checkboxes / docs if added.
5. Commit; sign off phase in commit message.
6. No PR/push unless human asked.
