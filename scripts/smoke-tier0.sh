#!/usr/bin/env bash
# PROD-5 tier 0: unit tests + docker + clangd pull verify.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

echo ">>> cargo test -p xtask -- --test-threads=1"
cargo test -p xtask -- --test-threads=1
echo ">>> cargo test -p poc-ide"
cargo test -p poc-ide
echo ">>> xtask smoke"
cargo run -q -p xtask -- smoke
echo "=== smoke-tier0: PASS ==="
