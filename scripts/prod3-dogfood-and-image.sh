#!/usr/bin/env bash
# PROD-3: full dogfood packs + runtime image (long; needs Docker Desktop).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
LOG_DIR="${LOG_DIR:-$CARGO_TARGET_DIR/clangd-overnight-logs}"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/prod3-dogfood-and-image-$(date +%Y%m%d-%H%M%S).log"

exec > >(tee -a "$LOG") 2>&1

echo "=== PROD-3 dogfood + runtime image ==="
echo "log=$LOG"
date

./build lsp all --flavor dogfood
cargo xtask runtime-image --both
docker images progressive-lsp-runtime:local

echo "=== SUCCESS — paste docker images line into docs/poc-tier/rest-live-proof.md ==="
date
