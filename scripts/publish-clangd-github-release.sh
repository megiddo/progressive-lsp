#!/usr/bin/env bash
# PROD-2: upload local-artifacts clangd archives to GitHub Releases (megiddo/progressive-lsp).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SHA="3623fe661ae35c6c80ac221f14d85be76aa870f1"
TAG="engines-clangd-${SHA}"
REPO="${GITHUB_REPOSITORY:-megiddo/progressive-lsp}"
STORE="${CARGO_TARGET_DIR:-$ROOT/target}/local-artifacts"
ARCHIVE_DIR="$STORE/engines/clangd/$SHA"

for triple in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  test -f "$ARCHIVE_DIR/${triple}.tar.gz" || {
    echo "ERROR: missing $ARCHIVE_DIR/${triple}.tar.gz — run cache push first" >&2
    exit 1
  }
done

BASE="https://github.com/${REPO}/releases/download/${TAG}"
MANIFEST_RELEASE="$STORE/manifest-release.json"

python3 - <<PY
import json, pathlib
src = pathlib.Path("$STORE/manifest.json")
data = json.loads(src.read_text())
base = "$BASE"
for a in data["artifacts"]:
    p = a["pack"]
    s = a["upstream_sha"]
    t = a["triple"]
    a["url"] = f"{base}/engines/{p}/{s}/{t}.tar.gz"
pathlib.Path("$MANIFEST_RELEASE").write_text(json.dumps(data, indent=2) + "\n")
print("wrote", "$MANIFEST_RELEASE")
PY

if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "Release $TAG already exists; uploading assets only"
else
  gh release create "$TAG" --repo "$REPO" --draft \
    --title "Engine pack: clangd @ ${SHA:0:8}" \
    --notes "Static musl clangd cache blobs. Pin: xtask/pack-pins.toml"
fi

for triple in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  src="$ARCHIVE_DIR/${triple}.tar.gz"
  dest="engines/clangd/${SHA}/${triple}.tar.gz"
  gh release upload "$TAG" --repo "$REPO" "${src}#${dest}" --clobber
done

gh release upload "$TAG" --repo "$REPO" "$MANIFEST_RELEASE#manifest.json" --clobber
gh release edit "$TAG" --repo "$REPO" --draft=false

echo "Published: $BASE/manifest.json"
