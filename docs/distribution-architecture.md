# Distribution architecture (S1–S4)

**Status:** stub — **PROD-4** fills normative content. Meta-orchestrator: implement per [productization-plan.md](productization-plan.md) PROD-4.1–PROD-4.3.

This document will be the **contract map** between four organizationally distinct subsystems that today share one GitHub repository.

| ID | Subsystem | Delivers | Primary contracts |
|---|---|---|---|
| **S1 Runtime** | `progressive-lsp` product bytes + protocols | [lsp-contract.md](lsp-contract.md), [control-protocol.md](control-protocol.md) |
| **S2 Orchestrator build** | `./build`, `xtask` — produce S1, stage packs, dist, image | [testing.md](testing.md), `./build help` |
| **S3 Engine build** | Pinned upstream SHAs → musl ELFs (Docker, native recipes) | [xtask/pack-pins.toml](../xtask/pack-pins.toml), `docker/*`, `check-static` |
| **S4 Engine artifacts** | Published archives + manifest for install/pull | [consumer.md](consumer.md) `StoreManifest`, GitHub Releases (target) |

**Edges (to expand in PROD-4):**

- S2 invokes S3 (build) or S4 (pull) depending on pack `kind`.
- S3 hands off tarballs to S4 via `xtask pack --cache push`.
- S4 URLs are consumed by S2 pull and by external consumers via `ArtifactTransport`.

Until PROD-4 signs off, [consumer.md](consumer.md) remains authoritative for manifest fields and layout.
