# Agent context policy (post–POC-tier stack)

**Pointer-based context.** Meta-orchestrator and every track orchestrator get the **same** repo path and locks. Pass payloads **unchanged** to children. All implementation happens in the **Google Drive workspace checkout** (not sibling `~/progressive-lsp-*` worktrees unless the human explicitly opts in).

Parent stack: **current `main`** after PR #8 merge. Do not reopen POC-URI … POC-REST WPs. Do not open `host8`. Do not run POST-PROOF overnight / `--cache-fill` unless the human asked. Do not `git stash`. Do not `git config`. Do not `--no-verify`. Do not force-push `main`. git-branchless (`git sl`). No Graphite.

Plan: [../post-poc-tier-development-plan.md](../post-poc-tier-development-plan.md). Hygiene: [../testing.md](../testing.md).

## Meta-orchestrator (orchestrator of orchestrators)

**Role:** Run tracks **in order**. POST-BUILD and POST-ART are merged. **POST-IDE** may start without POST-PROOF. **Skip POST-PROOF** until the human runs overnight clangd. Do not let optional native Darwin T1/T2 drive design — default container, progressive T1/T2/T3 inside one Linux serve.

**Loop:**

1. Confirm `main` is merged and clean. Never stash.
2. Spawn **track orchestrator** with payload below (`TRACK` + `BRANCH` + `WP_IDS`).
3. Track orchestrator stacks branch from signed-off parent, implements WPs, tests, docs, milestones.
4. Meta-orchestrator verifies exit table for that track before spawning the next track.
5. Commits only when the human asked (or track orchestrator policy says “commit when WP complete” if human delegated).

### Meta payload (copy to meta-orchestrator chat)

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/post-poc-tier/agent-context.md
  docs/post-poc-tier-development-plan.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
  docs/consumer.md
ROLE: meta-orchestrator
TRACK_ORDER: POST-IDE (active) → POST-PROOF when human ready
SKIP: POST-PROOF (human overnight)
PRODUCT: single client interface to progressive-lsp; container-default on laptop; optional native T1/T2 non-product
Spawn each track orchestrator with TRACK payload below. Same REPO. No external worktrees.
```

---

## Track orchestrator payloads

Replace `<TRACK>`, `<BRANCH>`, `<PARENT>`, `<WP_IDS>`.

```text
REPO: /Users/inoahsmi/Library/CloudStorage/GoogleDrive-en.gannim@gmail.com/My Drive/Personal/Development/progressive-lsp
CONTEXT_POLICY: pointer
READ:
  docs/post-poc-tier/agent-context.md
  docs/post-poc-tier-development-plan.md
  docs/design-patterns.md
  docs/testing.md
  docs/milestones.md
  docs/implementation-plan.md
  docs/branching.md
TRACK: <POST-BUILD | POST-ART | POST-IDE>
BRANCH: <post-build | post-artifacts | post-ide>
PARENT_BRANCH: <main | post-build | post-artifacts>
WP_IDS: <WP_IDS from plan table for this track>
STEPS: 8-step track loop in this file
Pass REPO + CONTEXT_POLICY + READ + locks to every implementer child unchanged.
```

### Track → branch → WPs

| TRACK | BRANCH | PARENT | WP_IDS |
|---|---|---|---|
| POST-BUILD | `post-build` | `main` | POST-BUILD.1–POST-BUILD.4 |
| POST-ART | `post-artifacts` | `post-build` | POST-ART.1–POST-ART.6 |
| POST-IDE | `post-ide` | `post-artifacts` | POST-IDE.1–POST-IDE.4 |

POST-DOC runs inside each track (milestones + implementation-plan rows when signing off).

---

## Track orchestrator loop (8 steps)

1. Last branch clean. Never stash.
2. `git checkout <PARENT>` && pull; `git checkout -b <BRANCH>` (or continue existing).
3. Implement **only** this track’s WP_IDS from [../post-poc-tier-development-plan.md](../post-poc-tier-development-plan.md).
4. New types → row in [../design-patterns.md](../design-patterns.md).
5. `CARGO_TARGET_DIR=$PWD/target cargo test -p xtask -- --test-threads=1` (and `-p poc-ide` for POST-IDE). No Docker daemon in unit tests. No `sleep`.
6. Update [../milestones.md](../milestones.md) **Post–POC-tier** section on sign-off.
7. Do not commit musl ELFs or pack-cache blobs.
8. Commit when human asked; message style: `Land … so POST-<track> can sign off without starting <next-branch>.`

---

## Track scope (do not cross)

| Branch | In | Out |
|---|---|---|
| `post-build` | `./build lsp --flavor dogfood`; full pack rebuild; image stamp includes full dests | HTTP pull; poc-ide UX |
| `post-artifacts` | Manifest schema, **local** `target/local-artifacts/` store, cache pull/push, wire dogfood pull for `cached`; remote CDN optional later | IDE default container |
| `post-ide` | Container-default open; progressive T1/T2/T3 in one serve; per-pack T3 gating; optional native T1/T2 only | Warping design for Darwin; cmake CI; POST-PROOF |

---

## Locks (all tracks)

- SHA-pegged pins in `xtask/pack-pins.toml`; no binaries in git. **Local artifact store first** (`target/local-artifacts/`); no remote artifact host required until upload.
- PR CI: no `--cache-fill` / cmake LLVM on every PR.
- Sandbox: do not use `required_permissions: ["all"]` for `cargo` / `cargo xtask` / routine `docker` in this repo.
- One serve; identity URIs; hash-gated install unchanged.
