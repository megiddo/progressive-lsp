#!/usr/bin/env bash
# PROD-2.5 remote: pull clangd from published GitHub Release manifest.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SHA="3623fe661ae35c6c80ac221f14d85be76aa870f1"
TAG="engines-clangd-${SHA}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export PROGRESSIVE_LSP_ARTIFACT_MANIFEST="https://github.com/megiddo/progressive-lsp/releases/download/${TAG}/manifest.json"

rm -rf "$CARGO_TARGET_DIR/pack-cache/clangd/$SHA"
cargo run -q -p xtask -- pack --pack clangd --cache pull --target x86_64-unknown-linux-musl
test -f "$CARGO_TARGET_DIR/pack-cache/clangd/$SHA/x86_64-unknown-linux-musl/clangd"
echo "=== verify-clangd-remote-pull: PASS ==="
