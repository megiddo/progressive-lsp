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

POC / laptop  Non-Linux: intelligence only via Linux host (container = local remote)
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

## POST-IDE — Container as primary intelligence host (laptop)

**Why.** On macOS/Windows, the editor should not treat a **Darwin** `progressive-lsp serve` as the product path for language intelligence. Docker (or a real remote Linux host) is **remote semantics** with an identity mount—not a optional T3 side door.

**Branch:** `post-ide` off `post-artifacts` (or `post-build` if artifact pull is not required for preflight).

| ID | Task | Exit |
|---|---|---|
| POST-IDE.1 | **Non-Linux:** default folder open = container; deprecate or hide native Open Folder for intelligence (T1/T2 in container on same serve) | [poc-ide/README.md](poc-ide/README.md) + [t3-linux-hosts.md](t3-linux-hosts.md) updated; one serve only |
| POST-IDE.2 | **`DockerRuntime::preflight_t3`**: inspect image or prefix layout for required engines (not `Ok(())`) | Honest T3 strip / journal when image built without dogfood packs |
| POST-IDE.3 | `./build run ide --folder DIR` defaults to `--container` on non-Linux | `./build help run ide` documents requirement: `./build lsp <arch> --flavor dogfood` first |
| POST-IDE.4 | Tests: `FakeRuntime` + argv/journal; no daemon | `cargo test -p poc-ide` green |

**Linux dev laptops:** choose container-only for parity with Mac **or** native musl serve on the host (still Linux intelligence). Document the chosen default in poc-ide; production Linux remains native serve on the server.

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
