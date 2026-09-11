# Post–POC-tier development plan

Work after **`t3-rest` merges to `main`**. POC-tier stack (POC-URI → POC-REST) lands in one PR; this file is the **formal follow-up** for artifacts, local builds, deployment, IDE posture, and live clangd proof.

Hygiene: [testing.md](testing.md). Exits copy to [milestones.md](milestones.md) when each WP signs off. Patterns: [design-patterns.md](design-patterns.md). Do not open `host8`. Do not commit musl ELFs or pack-cache blobs to git.

Related: [poc-tier-plan.md](poc-tier-plan.md), [poc-tier/rest-live-proof.md](poc-tier/rest-live-proof.md), [consumer.md](consumer.md), [architecture.md](architecture.md).

## Locks (unchanged)

| Lock | Meaning |
|---|---|
| Pin engines by **upstream git SHA** in `xtask/pack-pins.toml` | URLs and manifests reference SHA + triple + content hash; no `:latest`. |
| No binaries in git | Cache and musl dests stay under `target/` (gitignored). |
| PR CI | No cmake LLVM / `--cache-fill` on every PR. |
| Hash-gated install | `progressive-lsp install` and `InstallPacks` verify bytes; mismatch → no exec. |
| One serve | Native **or** container per workspace; never two intelligence processes. |
| Identity URIs | Same `file:` paths client and server; no rewriter in core or poc-ide. |

## Target end state

```text
Local dev     ./build lsp {arch} [--flavor dogfood]  →  musl core + all image-required packs + runtime image
              Optional: pull SHA-pegged cache blobs (clangd) instead of local --cache-fill

Deploy        Default manifest base URL + per-artifact URLs (tar.gz primary; zip/tar.bz2 optional)
              Thin: core + per-pack tarballs. Fat: full dist tarball or runtime OCI image.

POC / laptop  Default: one Linux serve in container (progressive T1→T2→T3)
              Optional native T1/T2 on non-Linux (legacy/dev); must not drive product design
              Long-term: single client interface to progressive-lsp (container = local remote)
              Linux prod: same musl binaries as container ISA
```

---

## Branch stack (after POC-tier PR merges)

```text
main
  └── post-build     # POST-BUILD.*
        └── post-artifacts   # POST-ART.*
              └── post-ide     # POST-IDE.*
```

Do not start a child branch until the parent WP’s exit criteria are met. Small fixes on `main` are OK; new behavior lands on the stack.

---

## POST-PROOF — Live REST.2 clangd (orchestrator, not CI)

**Depends on:** POC-tier PR merged; Docker Desktop resourced (see [rest-live-proof.md](poc-tier/rest-live-proof.md)).

| ID | Task | Exit |
|---|---|---|
| POST-PROOF.1 | Run `scripts/clangd-overnight.sh` (or manual cache-fill + pack both triples) | Cache + musl dests exist; `check-static` green on both clangd ELFs |
| POST-PROOF.2 | `cargo xtask runtime-image --both` | `progressive-lsp-runtime:local` builds; record digests in rest-live-proof |
| POST-PROOF.3 | Update milestones REST.2 live rows | Proof table filled; no cmake in PR CI |

---

## POST-BUILD — Single local “build everything” entrypoint

**Why.** `./build lsp` today rebuilds **slim** packs in step 2; dogfood `RuntimeImagePlan` **requires full** packs (clangd, tsgo, gopls, zls). “Doesn’t work on my machine” should have one documented path.

**Branch:** `post-build` off merged `main`.

| ID | Task | Exit |
|---|---|---|
| POST-BUILD.1 | Add `--flavor dogfood` (or make dogfood the default for `./build lsp all`) | Step 2 builds slim **and** full pack dests for selected triple(s); fail-closed messages name missing clangd cache |
| POST-BUILD.2 | Image freshness stamp includes **full** pack dest bytes (not slim-only) | Changing only a full pack stales runtime image |
| POST-BUILD.3 | Document in `./build help lsp` and [testing.md](testing.md) | One command list: core → packs → image; clangd cache-fill vs pull called out |
| POST-BUILD.4 | Unit tests lock argv / pack lists without Docker | `cargo test -p xtask` green |

---

## POST-ART — SHA-pegged artifact store (no git blobs)

**Why.** Users must build locally **or** reuse SHA-pegged bytes from a **local maintainer store** (`target/local-artifacts/`, gitignored). Remote HTTPS/CDN is **optional later** (upload when ready); deployment still accepts any URL in the manifest.

**Branch:** `post-artifacts` off signed-off `post-build` (POST-BUILD.1 may be parallel if only pull is needed for clangd).

| ID | Task | Exit |
|---|---|---|
| POST-ART.1 | **Manifest schema** (JSON): `pack`, `upstream_sha`, `triple`, `sha256`, `url`, `format` (`tar.gz` \| `tar` \| `zip` \| `tar.bz2`) | Documented in [consumer.md](consumer.md); example manifest in repo (URLs only, no blobs) |
| POST-ART.2 | **Layout convention** e.g. `{base}/engines/{pack}/{upstream_sha}/{triple}.{format}` | Matches `pack-cache` and musl dest layout |
| POST-ART.3 | `xtask pack --pack clangd --cache pull` (and `push` → local store) | Pull reads `target/local-artifacts/` first; push writes archive + `manifest.json`; optional remote upload later |
| POST-ART.4 | Wire `./build lsp --flavor dogfood` to **try pull** for `kind = cached` before reporting miss | Dogfood succeeds when local store (or pack-cache) has clangd; no artifact host required |
| POST-ART.5 | **Fat bundle**: document `xtask dist --pack full` and runtime image as equivalent “everything under prefix” | Release process publishes one full tarball + optional OCI tag per ISA |
| POST-ART.6 | **Install path**: keep in-tree install verify-only; document consumer `ArtifactTransport` fetching manifest URLs | No silent network inside `progressive-lsp install` unless an explicit future flag says otherwise |

**Default store:** `target/local-artifacts/` on disk (not in git). Remote default base URL is unset until maintainers upload; no placeholder HTTPS host required for day-to-day dev.

---

## POST-IDE — Container-default, progressive tiers (laptop)

**Why.** The long-term product is **one client interface** to `progressive-lsp` (stdio LSP + optional `progressive.v1`), backed by a **Linux intelligence host**. On macOS/Windows that host is the **container** (identity mount, same `file:` URIs). The POC must not let optional **native T1/T2** (Darwin serve) warp design: keep it as an explicit opt-in for fast local index without Docker, not the default path.

**Progressive container (canonical).** One `serve --mux` in the container. **T1/T2 always run there** when the container starts. **T3 is per language / per pack**: missing overnight **clangd** gates **C/C++ T3 only**, not the session. POST-PROOF fills packs; POST-IDE must not require POST-PROOF to land code/tests.

**Branch:** `post-ide` off merged `main` (POST-ART merged).

| ID | Task | Exit |
|---|---|---|
| POST-IDE.1 | **Default open = container** on non-Linux (`./build run ide`, File menu primary). **Optional** native Open Folder for T1/T2 only (flag/menu); T3 stays `NeedsContainer` on native. Docs: [poc-ide/README.md](../poc-ide/README.md), [t3-linux-hosts.md](../t3-linux-hosts.md), [consumer.md](../consumer.md) — single-interface goal stated | One serve per workspace; native path documented as non-product |
| POST-IDE.2 | **Runtime gates:** (a) Docker + image → can start serve; (b) **preflight** reports missing **dogfood** engines without blocking T1/T2. Journal + tier strip honest per language | `DockerRuntime::preflight_t3` (or renamed preflight) not `Ok(())`; `FakeRuntime` tests |
| POST-IDE.3 | `./build run ide --folder DIR` defaults `--container` on non-Linux; help cites `./build lsp <arch> --flavor dogfood` for full T3 when packs exist | `./build help run ide` updated |
| POST-IDE.4 | Tests: launch flags, journal steps, tier labels for slim vs dogfood image scenarios (fakes only) | `cargo test -p poc-ide` green |

**Linux dev laptops:** default **container** for parity with Mac POC, **or** document native musl serve as Linux-only dev shortcut; production Linux server stays native serve. Neither path may fork URIs or tier protocol.

---

## POST-DOC — Plan integration

| ID | Task | Exit |
|---|---|---|
| POST-DOC.1 | Link this file from [docs/README.md](README.md) and [implementation-plan.md](implementation-plan.md) | Agents find post-POC work without reopening closed POC-tier WPs |
| POST-DOC.2 | When POST-* WPs sign off, add milestones section **Post–POC-tier** | Checklists mirror this file |

---

## Explicitly out of scope (still)

- `host8`, attach/mux redesign, C# T3, in-tree HTTP/S3 **transport inside serve** (consumer fetches).
- Committing ELFs, pack-cache, or Docker layers to git.
- PR CI `--cache-fill` / full LLVM builds.
