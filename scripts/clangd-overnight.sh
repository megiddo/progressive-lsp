#!/usr/bin/env bash
# Overnight REST.2: LLVM cache-fill + musl pack extract for clangd (both triples).
# Run from repo root (main or fix branch). Requires Docker Desktop with enough RAM.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
LOG_DIR="${LOG_DIR:-$ROOT/target/clangd-overnight-logs}"
mkdir -p "$LOG_DIR"
STAMP="$(date +%Y%m%d-%H%M%S)"
MAIN_LOG="$LOG_DIR/run-$STAMP.log"

exec > >(tee -a "$MAIN_LOG") 2>&1

echo "=== clangd overnight ==="
echo "root=$ROOT"
echo "log=$MAIN_LOG"
echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
date

if ! docker info >/dev/null 2>&1; then
  echo "ERROR: Docker daemon not reachable"
  exit 1
fi

run_step() {
  local name="$1"
  shift
  echo ""
  echo ">>> $name"
  date
  "$@"
  echo "<<< OK $name"
  date
}

SHA="3623fe661ae35c6c80ac221f14d85be76aa870f1"
cache_x86="$CARGO_TARGET_DIR/pack-cache/clangd/$SHA/x86_64-unknown-linux-musl/clangd"
cache_arm="$CARGO_TARGET_DIR/pack-cache/clangd/$SHA/aarch64-unknown-linux-musl/clangd"
dest_x86="$CARGO_TARGET_DIR/musl/x86_64-unknown-linux-musl/engines/clangd/clangd"
dest_arm="$CARGO_TARGET_DIR/musl/aarch64-unknown-linux-musl/engines/clangd/clangd"

# One triple at a time — do not run two cache-fills in parallel.
run_step "cache-fill x86_64" \
  cargo run -q -p xtask -- pack --pack clangd --cache-fill --target x86_64-unknown-linux-musl

run_step "cache-fill aarch64" \
  cargo run -q -p xtask -- pack --pack clangd --cache-fill --target aarch64-unknown-linux-musl

run_step "pack extract x86_64" \
  cargo run -q -p xtask -- pack --pack clangd --target x86_64-unknown-linux-musl

run_step "pack extract aarch64" \
  cargo run -p xtask -- pack --pack clangd --target aarch64-unknown-linux-musl

for f in "$cache_x86" "$cache_arm" "$dest_x86" "$dest_arm"; do
  if [[ ! -f "$f" ]]; then
    echo "ERROR: missing $f"
    exit 1
  fi
  ls -la "$f"
done

run_step "check-static x86_64 clangd" \
  cargo run -q -p xtask -- check-static "$dest_x86"

run_step "check-static aarch64 clangd" \
  cargo run -q -p xtask -- check-static "$dest_arm"

echo ""
echo "=== SUCCESS (cache + musl dests + check-static) ==="
echo "Optional next: cargo run -q -p xtask -- runtime-image --both"
echo "Update docs/poc-tier/rest-live-proof.md with PASS rows and image tag proof."
date
