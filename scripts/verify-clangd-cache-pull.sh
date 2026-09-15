#!/usr/bin/env bash
# PROD-2.5: confirm cache pull repopulates pack-cache from local-artifacts (or remote manifest).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SHA="3623fe661ae35c6c80ac221f14d85be76aa870f1"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

for triple in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  cache_dir="$CARGO_TARGET_DIR/pack-cache/clangd/$SHA/$triple"
  rm -rf "$cache_dir"
  cargo run -q -p xtask -- pack --pack clangd --cache pull --target "$triple"
  test -f "$cache_dir/clangd" || {
    echo "ERROR: missing $cache_dir/clangd after pull"
    exit 1
  }
  echo "OK pull $triple -> $cache_dir/clangd"
done

echo "=== verify-clangd-cache-pull: PASS ==="
