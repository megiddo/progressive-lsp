# Engine artifact release runbook (PROD-2)

Publish **S4 engine artifacts** for `kind = cached` packs (today: **clangd**) to **GitHub Releases** on `megiddo/progressive-lsp`. Source stays in git; bytes never commit.

Layout convention (POST-ART.2): `{base}/engines/{pack}/{upstream_sha}/{triple}.tar.gz`.  
Manifest: [consumer.md](../consumer.md) `StoreManifest` — one row per blob with **content sha256** of the **archive** (what `cache push` prints).

Pin reference: [xtask/pack-pins.toml](../../xtask/pack-pins.toml) (`clangd` → `3623fe661ae35c6c80ac221f14d85be76aa870f1` until bumped).

---

## 1. Produce cache ELFs (once per pin bump)

After `--cache-fill` or [scripts/clangd-overnight.sh](../../scripts/clangd-overnight.sh):

```sh
export CARGO_TARGET_DIR="$PWD/target"
```

Confirm:

- `target/pack-cache/clangd/<upstream_sha>/<triple>/clangd` exists for both musl triples.

---

## 2. Push into local maintainer store (S4 staging)

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo xtask pack --pack clangd --cache push --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --cache push --target aarch64-unknown-linux-musl
```

Inspect:

- `target/local-artifacts/manifest.json` (`file://` URLs)
- `target/local-artifacts/engines/clangd/<upstream_sha>/*.tar.gz`

---

## 3. Release naming (GitHub)

| Field | Convention |
|---|---|
| **Tag** | `engines-{pack}-{upstream_sha}` e.g. `engines-clangd-3623fe661ae35c6c80ac221f14d85be76aa870f1` |
| **Title** | `Engine pack: clangd @ <short sha>` |
| **Asset path** | `engines/{pack}/{upstream_sha}/{triple}.tar.gz` (match POST-ART.2) |
| **Manifest asset** | `manifest.json` (optional second asset — same `StoreManifest`, `https://` urls) |

**Download URL shape:**

```text
https://github.com/megiddo/progressive-lsp/releases/download/<tag>/engines/clangd/<upstream_sha>/<triple>.tar.gz
```

Set pull base (no manifest file) to:

```text
PROGRESSIVE_LSP_ARTIFACT_BASE=https://github.com/megiddo/progressive-lsp/releases/download/<tag>
```

Then `artifact_url_from_base` matches the table above.

---

## 4. Upload (human `gh` auth)

Create draft release and upload archives from the local store (preserve path with `#`):

```sh
TAG="engines-clangd-3623fe661ae35c6c80ac221f14d85be76aa870f1"
SHA="3623fe661ae35c6c80ac221f14d85be76aa870f1"
ROOT="$PWD/target/local-artifacts/engines/clangd/$SHA"

gh release create "$TAG" --draft --title "Engine pack: clangd @ ${SHA:0:8}" --notes "Static musl clangd cache blobs. Pin in pack-pins.toml."

for triple in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  src="$ROOT/${triple}.tar.gz"
  dest="engines/clangd/${SHA}/${triple}.tar.gz"
  gh release upload "$TAG" "${src}#${dest}"
done
```

Publish manifest for consumers (after rewriting urls — step 5):

```sh
gh release upload "$TAG" target/local-artifacts/manifest.json#manifest.json
gh release edit "$TAG" --draft=false
```

**Note:** `PROGRESSIVE_LSP_ARTIFACT_PUSH=1` + `curl PUT` in xtask targets generic HTTPS hosts, **not** GitHub Releases. Use `gh release upload` for this repo.

---

## 5. Point manifest at HTTPS

Before uploading `manifest.json`, replace each row’s `url` with the release download URL (same path as upload `dest`). Keep `sha256` unchanged (archive hash from `cache push`).

Optional env for pulls on another machine:

```sh
export PROGRESSIVE_LSP_ARTIFACT_MANIFEST="https://github.com/megiddo/progressive-lsp/releases/download/${TAG}/manifest.json"
# or rely on target/local-artifacts/manifest.json after copy
```

---

## 6. Verify pull-only (PROD-2.5)

On a machine **with** local store **or** published manifest:

```sh
./scripts/verify-clangd-cache-pull.sh
```

Or manually:

```sh
export CARGO_TARGET_DIR="$PWD/target"
rm -rf "target/pack-cache/clangd/$SHA"
cargo xtask pack --pack clangd --cache pull --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --cache pull --target aarch64-unknown-linux-musl
```

Then `./build lsp $(uname -m | sed 's/arm64/aarch64/') --flavor dogfood` should not fail-closed on missing clangd cache (other full packs must still exist).

---

## 7. What this does not cover

- **S1** `progressive-lsp` binary releases (fat dist / core tarballs) — use `xtask dist` + separate release tag when ready.
- **Runtime OCI image** — [PROD-3](../productization-plan.md) (`runtime-image --both`); not uploaded as part of engine-only tag unless you add an image release process later.
