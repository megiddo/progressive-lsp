#!/bin/sh
# IT-TAM runner: docker mux + plsp-it1 tam-run.
set -eu

HARNESS_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=integration/harness/bootstrap.sh
. "$HARNESS_DIR/bootstrap.sh"

plsp_export_cargo_env
ROOT=$(plsp_repo_root)
INTEGRATION="$ROOT/integration"
TAM_DIR="$INTEGRATION/tier-api-matrix"
OUT="${TAM_OUT:-$INTEGRATION/out}"

SUITE_ID="${1:-hermetic-discover-java}"
case "$SUITE_ID" in
  java-junit4)
    SUITE="$TAM_DIR/suites/java-junit4.tam.yaml"
    FIXTURE_ROOT="${PLSP_TAM_ROOT:-$INTEGRATION/fixtures/discover-java}"
    ;;
  hermetic-discover-java | *)
    SUITE="$TAM_DIR/suites/hermetic-discover-java.tam.yaml"
    FIXTURE_ROOT="${PLSP_TAM_ROOT:-$INTEGRATION/fixtures/discover-java}"
    ;;
esac

mkdir -p "$OUT"
HARNESS=$(plsp_ensure_harness_bin)

if plsp_docker_ok; then
  plsp_ensure_runtime_image || true
  plsp_ensure_musl_controller || true
fi

ROOT_ABS=$(CDPATH= cd -- "$FIXTURE_ROOT" && pwd)
REPORT="$OUT/tam-${SUITE_ID}-report.json"
PLATFORM="${PLSP_DOCKER_PLATFORM:-$(plsp_docker_platform)}"
MUSL_BIN=$(plsp_musl_bin)

set +e
if [ -f "$MUSL_BIN" ]; then
  "$HARNESS" tam-run \
    --suite "$SUITE" \
    --workspace-root "$ROOT_ABS" \
    --platform "$PLATFORM" \
    --mount-serve-bin "$MUSL_BIN" \
    --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
    --request-deadline-ms "${PLSP_TAM_REQUEST_DEADLINE_MS:-15000}" \
    --quiet \
    >"$REPORT" 2>"$OUT/tam-${SUITE_ID}-trace.log"
else
  "$HARNESS" tam-run \
    --suite "$SUITE" \
    --workspace-root "$ROOT_ABS" \
    --platform "$PLATFORM" \
    --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
    --request-deadline-ms "${PLSP_TAM_REQUEST_DEADLINE_MS:-15000}" \
    --quiet \
    >"$REPORT" 2>"$OUT/tam-${SUITE_ID}-trace.log"
fi
STATUS=$?
set -e

echo "report: $REPORT"
echo "trace:  $OUT/tam-${SUITE_ID}-trace.log"

RESULT=$(grep -o '"result"[[:space:]]*:[[:space:]]*"[^"]*"' "$REPORT" | head -1 | sed 's/.*"\([^"]*\)"$/\1/' || true)
if [ "$RESULT" = "fail" ]; then
  exit 1
fi
if [ "$RESULT" = "skip" ]; then
  echo "tam-run skipped: see report notes" >&2
  exit 0
fi
exit "$STATUS"
