#!/bin/sh
# Default integration smoke: hermetic IT-TAM + discover-container (meta + trace on fail).
set -eu

HARNESS_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=integration/harness/bootstrap.sh
. "$HARNESS_DIR/bootstrap.sh"

plsp_export_cargo_env
ROOT=$(plsp_repo_root)
OUT="${PLSP_INTEG_OUT:-$ROOT/integration/out}"
mkdir -p "$OUT"

SUITE_ID="${1:-hermetic-discover-java}"

echo "=== integration smoke (suite=$SUITE_ID) ===" >&2

if plsp_docker_ok; then
  plsp_ensure_runtime_image || true
  plsp_ensure_musl_controller || true
else
  echo "bootstrap: no docker — container steps may report skip" >&2
fi

FAIL=0

echo "--- tam-run ---" >&2
if ! "$HARNESS_DIR/run-tam.sh" "$SUITE_ID"; then
  FAIL=1
fi

echo "--- discover-container ---" >&2
if ! "$HARNESS_DIR/run-discover-container.sh"; then
  FAIL=1
fi

echo "=== done (fail=$FAIL) reports under $OUT ===" >&2
exit "$FAIL"
