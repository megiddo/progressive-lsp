# Agent context policy (productization program)

**Pointer-based context.** Meta-orchestrator and every phase orchestrator get the **same** repo path and locks. Pass payloads **unchanged** to children. Implementation uses the **Google Drive workspace checkout** unless the human opts into another worktree.

**Parent state:** `main` with POST-BUILD, POST-ART, POST-IDE merged. Human completed clangd **cache-fill + check-static** (POST-PROOF.1). PR #12 merged. Active program: [../productization-plan.md](../productization-plan.md).

**Do not** reopen POC-tier or post-POC track WPs. **Do not** run `--cache-fill` unless the human asked. **Do not** `git stash`. **Do not** `git config`. **Do not** `--no-verify`. **Do not** force-push `main`. git-branchless (`git sl`). No Graphite.

Hygiene: [../testing.md](../testing.md). Prior meta (historical): [../post-poc-tier/agent-context.md](../post-poc-tier/agent-context.md).

---

## Meta-orchestrator (orchestrator of orchestrators)

**Role:** Run **PROD phases in order** per parallel table in the plan. Spawn a **phase orchestrator** per phase (or implementers directly for PROD-1). Verify phase exit table before starting the next phase. **Do not** run PROD-2 and PROD-3 Docker-heavy work in parallel on one host.

**Loop:**

1. Confirm `main` (or signed-off parent branch) is clean. Never stash.
2. Read [../productization-plan.md](../productization-plan.md) for current phase exit criteria.
3. Spawn **phase orchestrator** with payload below (`PHASE`, `BRANCH`, `WP_IDS`).
4. Phase orchestrator may spawn **sub-orchestrators** only when the plan says so (PROD-5 tools vs matrix).
5. Meta-orchestrator verifies exit table for that phase before spawning the next.
6. Commits only when the human asked (unless human delegated “commit when phase complete”).
7. **Do not stop** for status summaries. Continue phase work until a step **mandates human action** (e.g. missing Docker daemon on host, `gh` not authenticated, destructive prod choice). Long builds run in background; poll logs and proceed.

### Meta payload (copy to meta-orchestrator chat)

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/productization/agent-context.md
  docs/productization-plan.md
  docs/productization/release-runbook.md
  docs/productization/validation-matrix.md
  docs/distribution-architecture.md
  docs/consumer.md
  docs/poc-tier/rest-live-proof.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
ROLE: meta-orchestrator
PHASE_ORDER: PROD-1 → PROD-2 → PROD-3 → (PROD-4 may overlap PROD-1 docs-only) → PROD-5
PARALLEL: PROD-1 + PROD-4 skeleton only if distinct files and human agrees
PRODUCT: four subsystems S1–S4; GitHub Releases for S4; container-default IDE unchanged
HUMAN_GATES: GitHub Release publish; long docker image build; optional re-run cache-fill
Spawn each phase orchestrator with PHASE payload below. Same REPO. No external worktrees.
```

---

## Phase orchestrator payload

Replace `<PHASE>`, `<BRANCH>`, `<PARENT>`, `<WP_IDS>`.

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/productization/agent-context.md
  docs/productization-plan.md
  docs/productization/release-runbook.md
  docs/productization/validation-matrix.md
  docs/distribution-architecture.md
  docs/consumer.md
  docs/poc-tier/rest-live-proof.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
PHASE: <PROD-1 | PROD-2 | PROD-3 | PROD-4 | PROD-5>
BRANCH: <prod-housekeep | prod-artifacts | prod-image | prod-contracts | prod-validation>
PARENT_BRANCH: <main | prod-housekeep | prod-artifacts | prod-image | prod-contracts>
WP_IDS: <from productization-plan table for this phase>
STEPS: 8-step phase loop in this file
Pass REPO + CONTEXT_POLICY + READ + locks to every implementer child unchanged.
```

### Phase → branch → WPs

| PHASE | BRANCH | PARENT | WP_IDS |
|---|---|---|---|
| PROD-1 HOUSEKEEP | `prod-housekeep` | `main` | PROD-1.1–PROD-1.5 |
| PROD-2 ARTIFACTS | `prod-artifacts` | `prod-housekeep` | PROD-2.1–PROD-2.5 |
| PROD-3 IMAGE | `prod-image` | `prod-artifacts` | PROD-3.1–PROD-3.3 |
| PROD-4 CONTRACTS | `prod-contracts` | `prod-housekeep` or `prod-image` | PROD-4.1–PROD-4.4 |
| PROD-5 VALIDATION | `prod-validation` | `prod-image` | PROD-5.1–PROD-5.4 |

Merge each branch via PR to `main` when the human wants; stack locally with git-branchless until then.

---

## Phase orchestrator loop (8 steps)

1. Last branch clean. Never stash.
2. `git checkout <PARENT>` && pull; `git checkout -b <BRANCH>` (or continue existing).
3. Implement **only** this phase’s WP_IDS from [../productization-plan.md](../productization-plan.md).
4. New types → row in [../design-patterns.md](../design-patterns.md).
5. `CARGO_TARGET_DIR=$PWD/target cargo test -p xtask -- --test-threads=1` (+ `-p poc-ide` if PROD-5 touches poc-ide). No Docker daemon in unit tests. No `sleep`.
6. Update [../milestones.md](../milestones.md) **Productization** section when phase signs off.
7. Do not commit musl ELFs, pack-cache blobs, or `target/local-artifacts/` archives.
8. Commit when human asked; message style: `Land … so PROD-<n> can sign off without starting <next-branch>.`

---

## Phase scope (do not cross)

| Phase | In | Out |
|---|---|---|
| PROD-1 | rest-live-proof, milestones, README, agent-context cross-links | xtask behavior changes |
| PROD-2 | cache push/pull docs, Release runbook, manifest URLs, optional xtask upload | IDE UX; runtime serve logic |
| PROD-3 | runtime-image proof, digests in docs | New pack pins |
| PROD-4 | distribution-architecture.md, consumer/versioning docs | Large refactors splitting crates |
| PROD-5 | smoke scripts, integration tier docs, conformance matrix | PR CI cache-fill |

---

## Sub-orchestrator (PROD-5 only, optional)

When PROD-5 splits **tooling** vs **matrix docs**, spawn two children with the same READ block:

```text
SUBTRACK: <VALIDATION-TOOLS | VALIDATION-MATRIX>
WP_IDS: <PROD-5.1–5.2 | PROD-5.3–5.4>
```

Both must finish before phase orchestrator signs off PROD-5.

---

## Locks (all phases)

- SHA-pegged pins in `xtask/pack-pins.toml`; no binaries in git.
- **S4 default path:** local store then **GitHub Releases**; see [../consumer.md](../consumer.md).
- PR CI: no `--cache-fill` on every PR.
- Sandbox: do not use `required_permissions: ["all"]` for `cargo` / `cargo xtask` / routine `docker` in this repo.
- One serve; identity URIs; hash-gated install unchanged.
