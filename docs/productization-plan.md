# Productization plan (post–POST-IDE)

Work after **POST-BUILD**, **POST-ART**, and **POST-IDE** are merged to `main`. POC-tier code and post-POC tracks are **signed off**; the human completed **live clangd cache-fill** (POST-PROOF.1). This plan closes proof gaps, publishes engine bytes, rebuilds the dogfood image, documents **four distribution subsystems**, and adds validation tooling.

**Orchestration:** [productization/agent-context.md](productization/agent-context.md) — meta-orchestrator spawns **phase orchestrators** (or implementers directly when a phase is small). Run phases **sequentially** unless the parallel table allows overlap.

**Prior plans (closed; do not reopen WPs):** [poc-tier-plan.md](poc-tier-plan.md), [post-poc-tier-development-plan.md](post-poc-tier-development-plan.md).

Hygiene: [testing.md](testing.md). Consumer / manifest contracts: [consumer.md](consumer.md). Live clangd notes: [poc-tier/rest-live-proof.md](poc-tier/rest-live-proof.md).

## Locks (unchanged)

| Lock | Meaning |
|---|---|
| No ELFs / pack-cache / image layers in git | Bytes under `target/` or GitHub Release assets only |
| SHA pins in `xtask/pack-pins.toml` | Manifest rows keyed by `upstream_sha` + triple + content sha256 |
| PR CI | No `--cache-fill`, no full LLVM cmake on every PR |
| Hash-gated install | Verify-only in-tree; network fetch only via explicit consumer transport |
| One serve; identity URIs | Container-default POC posture unchanged |
| Workspace checkout | Google Drive path in agent-context; no `git stash`; no `git config`; no `--no-verify`; no force-push `main` |

## Four subsystems (organizational model)

All may live in **one GitHub repo** today; each has its own **contract surface** for a future split.

| ID | Subsystem | In-repo home (today) | Delivers |
|---|---|---|---|
| **S1 Runtime** | Language intelligence product | `progressive-lsp`, crates, protocols | Static musl `progressive-lsp`, LSP + optional `progressive.v1` |
| **S2 Orchestrator build** | How maintainers produce S1 + image | `./build`, `cargo xtask` | Core build, pack wiring, dist, runtime image plan |
| **S3 Engine build** | How maintainers produce engine ELFs | `pack-pins.toml`, `docker/*`, pack recipes | Slim/full/`cached` dests; Docker cache-fill for clangd |
| **S4 Engine artifacts** | Published bytes consumers install | `target/local-artifacts/`, **GitHub Releases** (target) | `StoreManifest`, per-pack archives, fat dist / OCI tags |

**Contract edges (implement in PROD-4, use in PROD-2–3):**

- **S2 → S3:** flavor (`slim` \| `dogfood`), pull vs `--cache-fill`, fail-closed messages ([testing.md](testing.md)).
- **S3 → S4:** pin row → cache path → `{base}/engines/{pack}/{upstream_sha}/{triple}.tar.gz` + sha256 ([consumer.md](consumer.md)).
- **S4 → S1 consumers:** `ArtifactTransport` + verify-only `Installer` ([consumer.md](consumer.md)).
- **S2 → S4:** `./build lsp --flavor dogfood` tries `cache pull` before miss; image stamp includes full dests.

GitHub hosts **source (S1–S3)** and **release assets (S4)** until a dedicated artifact org/repo exists.

---

## Phase order and parallelism

```text
PROD-1 HOUSEKEEP ──► PROD-2 ARTIFACTS ──► PROD-3 IMAGE ──► PROD-5 VALIDATION
        │                    │
        └──────────► PROD-4 CONTRACTS (docs-heavy; may start after PROD-1)
```

| Parallel OK? | Phases | Condition |
|---|---|---|
| Yes | PROD-1 + PROD-4 (initial pass) | Different files; no branch fight; PROD-4 does not change xtask behavior yet |
| No | PROD-2 + PROD-3 | Both use Docker / long builds; run **ARTIFACTS then IMAGE** on one machine |
| No | PROD-3 + PROD-5 impl | VALIDATION assumes dogfood image recipe exists |
| Human gate | PROD-2 GitHub Release publish | Agent prepares manifest + runbook; human may run `gh release create` / upload |

Default branch stack (git-branchless friendly):

```text
main
  └── prod-housekeep     # PROD-1
        └── prod-artifacts   # PROD-2
              └── prod-image       # PROD-3
                    └── prod-contracts   # PROD-4 (if not merged from parallel doc-only work)
                          └── prod-validation   # PROD-5
```

Small doc-only PROD-4 work may land on `prod-housekeep` and merge early; **normative contract doc** should be signed off before PROD-2 marks Release URLs final.

---

## PROD-1 — HOUSEKEEP (documentation & POST-PROOF closure)

**Why.** Repo docs still say REST.2 live clangd is **pending**; post-POC agent-context still **skips POST-PROOF**. Human proof exists; agents must not re-run overnight without ask.

**Branch:** `prod-housekeep` off `main`.

| ID | Task | Exit |
|---|---|---|
| PROD-1.1 | Update [poc-tier/rest-live-proof.md](poc-tier/rest-live-proof.md): PASS for cache-fill + `check-static` both triples; note PR #12 + build-host clang fix; remove “pending” where human succeeded | Table reflects human run; paths under `target/` documented |
| PROD-1.2 | Update [milestones.md](milestones.md) POC-REST / Post–POC-tier: REST.2 **live** ELF proof not pending; add **Productization** section stub pointing here | Checklists match reality |
| PROD-1.3 | Revise [post-poc-tier/agent-context.md](post-poc-tier/agent-context.md): POST-PROOF complete; active work → [productization/agent-context.md](productization/agent-context.md) | No `SKIP: POST-PROOF` for new meta runs |
| PROD-1.4 | [implementation-plan.md](implementation-plan.md) + [docs/README.md](README.md): link productization plan; agent rule row for productization meta | Discoverability |
| PROD-1.5 | Archive stale orchestrator instructions (branch names `fix-clangd-musl-link`, “checkout t3-rest”) in rest-live-proof overnight section — replace with `main` + pointer to [scripts/clangd-overnight.sh](../scripts/clangd-overnight.sh) | One maintainer path |

**Orchestrator spawn:** phase orchestrator only (no sub-orchestrators). Implementers may split by file (rest-live-proof vs milestones vs agent-context).

**Tests:** `cargo test -p xtask -- --test-threads=1` if any xtask/doc test touched; otherwise docs-only.

---

## PROD-2 — ARTIFACTS (finalize S4; GitHub Releases)

**Why.** Clangd musl ELFs exist locally after cache-fill; POST-ART built **local store** + pull/push. Target store: **GitHub Releases** on `megiddo/progressive-lsp` (same org/repo as source for now).

**Branch:** `prod-artifacts` off signed-off `prod-housekeep` (or `main` if PROD-1 merged).

| ID | Task | Exit |
|---|---|---|
| PROD-2.1 | Maintainer runbook: `cache push` both triples → inspect `target/local-artifacts/manifest.json` | [productization/release-runbook.md](productization/release-runbook.md) + [consumer.md](consumer.md) |
| PROD-2.2 | Define **Release naming**: tag pattern, asset names per `{pack}/{upstream_sha}/{triple}.tar.gz`, manifest asset URL | Table in runbook; matches `StoreManifest` |
| PROD-2.3 | Implement or document **upload path**: `gh release upload` + manifest `url` → `https://github.com/.../releases/download/...` **or** extend xtask when `PROGRESSIVE_LSP_ARTIFACT_BASE` + `PROGRESSIVE_LSP_ARTIFACT_PUSH=1` | Pull from HTTPS works on clean machine without `--cache-fill` |
| PROD-2.4 | Pin **example manifest** in repo ([xtask/artifact-manifest.example.json](../xtask/artifact-manifest.example.json)) with real layout but placeholder tag until first release | URLs are https template, not `file://` only |
| PROD-2.5 | `./build lsp <arch> --flavor dogfood` succeeds with **pull only** after wipe of `pack-cache/clangd` (manual or scripted check documented) | Fail-closed message unchanged when manifest missing |

**Human gates:** First Release create/upload may require human `gh` auth. Agent prepares assets locally via `cache push`; does not commit blobs.

**Orchestrator spawn:** phase orchestrator → implementers for xtask/docs; optional child for `gh`-only runbook.

**Tests:** `cargo test -p xtask -- --test-threads=1`; artifact store unit tests green. No `--cache-fill` in CI.

---

## PROD-3 — IMAGE (dogfood runtime with clangd)

**Why.** `RuntimeImagePlan` requires static **clangd** on both musl triples. POST-PROOF.2: rebuild image with full dests.

**Branch:** `prod-image` off signed-off `prod-artifacts` (or `main` with PROD-2 merged).

| ID | Task | Exit |
|---|---|---|
| PROD-3.1 | Record maintainer command: `cargo xtask runtime-image --both` (or `./build lsp … --flavor dogfood` path) after clangd dests present | [rest-live-proof.md](poc-tier/rest-live-proof.md) + runbook |
| PROD-3.2 | Human or agent run: image builds; record **digests / tags** (`progressive-lsp-runtime:local`) in rest-live-proof | POST-PROOF.2 exit |
| PROD-3.3 | Confirm `PackImageCopy` includes clangd both ISAs (unit plan already; note live tag in docs) | Milestones POST-PROOF row checked |

**Orchestrator spawn:** single implementer for docs; **human or long-running agent** for docker build (do not run parallel with PROD-2 cache-fill).

**Tests:** xtask unit tests; no requirement to commit image layers.

---

## PROD-4 — CONTRACTS (four subsystems & distribution)

**Why.** Separate **runtime**, **orchestrator build**, **engine build**, and **artifact vending** in documentation so future repo splits and Release layout stay coherent.

**Branch:** `prod-contracts` (may start doc skeleton on `prod-housekeep` in parallel with PROD-1).

| ID | Task | Exit |
|---|---|---|
| PROD-4.1 | Add [distribution-architecture.md](distribution-architecture.md): S1–S4 definitions, contract edges, GitHub-as-host for source + releases | Linked from README + consumer.md |
| PROD-4.2 | Versioning story: Cargo crate version vs engine `upstream_sha` vs Release tag | One table; no `:latest` for engines |
| PROD-4.3 | Explicit **out of scope**: in-tree SSH transport, PR CI cache-fill, committing ELFs | Matches locks above |
| PROD-4.4 | Update [design-patterns.md](design-patterns.md) if new types for release/upload helpers | Pattern row or “uses existing StoreManifest” note |

**Orchestrator spawn:** phase orchestrator → doc implementers; no Docker.

---

## PROD-5 — VALIDATION (smoke, matrix, tools)

**Why.** Product has not been end-to-end exercised recently; need tiered smoke + manual matrix for language flavors without LLVM in every PR.

**Branch:** `prod-validation` off signed-off `prod-image` (contracts doc merged).

| ID | Task | Exit |
|---|---|---|
| PROD-5.1 | **Smoke tier 0:** script or `xtask` subcommand — docker reachable, image present, `progressive-lsp serve --mux` starts (fake or short-lived) | Documented in [testing.md](testing.md); runs without cache-fill |
| PROD-5.2 | **Smoke tier 1:** poc-ide or `./build run ide --folder <fixture>` — T1 attach + one slim T3 language | Manual checklist + optional automation behind `integration/` |
| PROD-5.3 | **Dogfood matrix:** table of languages × tier × expected gate (clangd for C/C++, Java aarch64 libc exception, Rust sysroot honesty) | [conformance.md](conformance.md) or productization appendix |
| PROD-5.4 | Release/nightly bar: which smokes run on PR vs on Release publish | [integration/README.md](../integration/README.md) updated |

**Orchestrator spawn:** phase orchestrator → sub-orchestrator **VALIDATION-TOOLS** if both xtask smoke and integration fixtures land in one phase.

**Tests:** new tests must obey testing.md; no `sleep`; no daemon in unit tests unless existing integration pattern.

---

## Meta-orchestrator exit (program complete)

| Gate | Meaning |
|---|---|
| PROD-1 signed off | Docs truthful; POST-PROOF not pending |
| PROD-2 signed off | GitHub Release path documented; pull works for clangd |
| PROD-3 signed off | Dogfood runtime image built with clangd; digests recorded |
| PROD-4 signed off | distribution-architecture.md is source of truth for S1–S4 |
| PROD-5 signed off | Smoke tiers documented; at least tier 0 automated |

Then update [milestones.md](milestones.md) **Productization** section to **SIGNED OFF** and freeze this plan except human-driven pin bumps and new engine releases.

---

## Explicitly out of scope (this program)

- `host8`, C# T3, in-tree HTTP/S3 inside serve
- Reopening POC-URI … POC-REST or POST-BUILD/ART/IDE WPs
- Native macOS/Windows **server** as product path
- Buck2 / alternate engine build orchestration
- Mandatory remote CDN other than GitHub Releases for v1 of this plan
