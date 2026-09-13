# Distribution architecture (S1–S4)

Normative map of **four subsystems** that share one git repository today. Each may later become its own repo or GitHub org project; contracts below must stay stable across that split.

Related: [productization-plan.md](productization-plan.md), [consumer.md](consumer.md), [architecture.md](architecture.md), [release-runbook.md](productization/release-runbook.md).

---

## Subsystems

### S1 — Runtime (`progressive-lsp` product)

**Delivers:** Static musl server binary, LSP stdio, optional `progressive.v1` control, language plugins, hash-gated install layout under a prefix.

**Source of truth:** Rust crates in this repo; version **0.1.0** (Cargo) is the server product version, independent of engine SHAs.

**Contracts:**

| Consumer | Contract doc | Transport |
|---|---|---|
| Stock LSP client | [lsp-contract.md](lsp-contract.md) | stdio JSON-RPC only |
| Progressive client | [control-protocol.md](control-protocol.md) | socket or `--mux` |
| Installer / prefix layout | [consumer.md](consumer.md) | verify-only in-tree; optional fetch via `ArtifactTransport` |

**Distribution (future):** GitHub Release tarball `progressive-lsp-{triple}.tar.gz` or fat dist from `xtask dist` — not the same tag family as engine-only releases.

---

### S2 — Orchestrator build

**Delivers:** Maintainer commands that build S1, stage packs, stamp freshness, produce dist tarballs and OCI runtime images.

**Source of truth:** `./build`, `cargo xtask`, [testing.md](testing.md).

**Entrypoints:**

| Command | Output |
|---|---|
| `./build lsp {arch} [--flavor slim\|dogfood]` | musl core + pack dests under `target/musl/` |
| `cargo xtask dist --pack …` | Prefix-shaped tarballs + manifest |
| `cargo xtask runtime-image [--both]` | `progressive-lsp-runtime:local` (OCI) |

**Contracts to S3/S4:**

- Reads [xtask/pack-pins.toml](../xtask/pack-pins.toml) for engine SHAs and pack `kind` (`slim`, `full`, `cached`, …).
- For `kind = cached` (clangd): tries **S4 pull** before fail-closed ([POST-ART](../post-poc-tier-development-plan.md)).
- Image plan (`RuntimeImagePlan`) lists required binaries per triple; missing dest → hard error at image build.

---

### S3 — Engine build

**Delivers:** Musl static ELFs per pack and triple under `target/musl/…` and/or `target/pack-cache/…`.

**Source of truth:** Pin rows in `pack-pins.toml`; Dockerfiles under `docker/`; cache-fill only for `cached` packs (clangd LLVM).

**Contracts:**

| Input | Output |
|---|---|
| `upstream_sha` + triple | `target/musl/{triple}/engines/{pack}/{binary}` |
| `cached` pack | `target/pack-cache/{pack}/{sha}/{triple}/{binary}` via `--cache-fill` or S4 pull |

**CI lock:** PR CI must not run `--cache-fill` / full LLVM builds.

**Handoff to S4:** `cargo xtask pack --pack {p} --cache push` archives cache ELF → `target/local-artifacts/…`.

---

### S4 — Engine artifacts (vending)

**Delivers:** Immutable archives + `StoreManifest` for SHA-verified pull.

**Layout (POST-ART.2):**

```text
{base}/engines/{pack}/{upstream_sha}/{triple}.tar.gz
```

**Default maintainer store (local):** `target/local-artifacts/` (gitignored).

**Target remote:** GitHub Releases on `megiddo/progressive-lsp`, tag `engines-{pack}-{upstream_sha}` — see [release-runbook.md](productization/release-runbook.md).

**Manifest row fields:** `pack`, `upstream_sha`, `triple`, `sha256` (of **archive**), `url`, `format`.

**Pull resolution order** (xtask): local `manifest.json` → `PROGRESSIVE_LSP_ARTIFACT_MANIFEST` → local archive file → `PROGRESSIVE_LSP_ARTIFACT_BASE` URL template.

**Contracts to consumers:** External hosts use `progressive-lsp-install` + `ArtifactTransport`; no silent network inside in-tree `progressive-lsp install`.

---

## Edge summary

```text
  S3 engine build ──cache push──► S4 artifacts ◄──cache pull── S2 orchestrator
        ▲                              │
        │ cache-fill (maintainer)      │ GitHub Releases (https)
        └──────────────────────────────┘

  S2 orchestrator ──build──► S1 runtime binary + staged packs ──► OCI runtime image
                                      ▲
                                      └── poc-ide / container serve (laptop POC)
```

---

## Versioning

| Artifact | Version key | Notes |
|---|---|---|
| Server crates / binary | Cargo **0.1.0** | Product semver; not engine SHA |
| Engine blob | `upstream_sha` in pins | 40-hex git SHA; bump = new release tag |
| Release tag (engines) | `engines-{pack}-{upstream_sha}` | One tag per pin generation per pack |
| Runtime OCI | `progressive-lsp-runtime:local` (dev) / future semver tag | Includes S1 + staged pack bytes |

No `:latest` for engines. Content hash (`sha256` in manifest) gates install.

---

## GitHub hosting (v1 assumption)

| Subsystem | GitHub surface |
|---|---|
| S1–S3 source | This repository |
| S4 engine bytes | **Release assets** (same repo) |
| S1 binary / fat dist | Release assets (separate tags when published) |
| OCI runtime | Registry (local `docker load` today; ghcr optional later) |

Splitting repos later: S4 manifest `url` fields change; layout path stays the same.

---

## Out of scope

- In-tree HTTP/S3 transport inside `serve`
- Committing ELFs, pack-cache, or local-artifacts to git
- C# T3; `host8`
- PR CI `--cache-fill`
