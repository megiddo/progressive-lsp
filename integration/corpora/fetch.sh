#!/bin/sh
# Thin wrapper: fetch pinned URL+SHA corpora. Does not vendor git histories.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
HARNESS_MANIFEST="$ROOT/integration/harness/Cargo.toml"
CACHE="${PLSP_CORPORA_CACHE:-$ROOT/integration/corpora/.cache}"
ONLY=${1:-}

cargo build --manifest-path "$HARNESS_MANIFEST" --bin plsp-it1 >/dev/null
HARNESS="$ROOT/integration/harness/target/debug/plsp-it1"
if [ ! -x "$HARNESS" ]; then
    HARNESS="${CARGO_TARGET_DIR:-$ROOT/target}/debug/plsp-it1"
fi
if [ -n "$ONLY" ]; then
    exec "$HARNESS" fetch --pins "$ROOT/integration/corpora/pins.json" --cache "$CACHE" --id "$ONLY"
fi
exec "$HARNESS" fetch --pins "$ROOT/integration/corpora/pins.json" --cache "$CACHE"
