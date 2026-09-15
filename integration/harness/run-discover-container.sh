#!/bin/sh
# Container mux discover: same API path as POC IDE **Find Definition**.
set -eu

HARNESS_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=integration/harness/bootstrap.sh
. "$HARNESS_DIR/bootstrap.sh"

plsp_export_cargo_env
ROOT=$(plsp_repo_root)
INTEGRATION="$ROOT/integration"
OUT="${DISCOVER_CONTAINER_OUT:-$INTEGRATION/out}"
mkdir -p "$OUT"

HARNESS=$(plsp_ensure_harness_bin)

if ! plsp_docker_ok; then
  echo '{"rpc":"discover-container","result":"skip","notes":"docker missing"}' >"$OUT/discover-container-report.json"
  exit 0
fi

if ! plsp_ensure_runtime_image; then
  echo '{"rpc":"discover-container","result":"skip","notes":"runtime image build failed or docker unavailable"}' \
    >"$OUT/discover-container-report.json"
  exit 0
fi

plsp_ensure_musl_controller || true

PLATFORM="${PLSP_DOCKER_PLATFORM:-$(plsp_docker_platform)}"
LOG_DIR="${PLSP_DISCOVER_LOG_DIR:-$HOME/.progressivelsp/log}"
mkdir -p "$LOG_DIR"
WAL="$LOG_DIR/discover-container-$(date +%s)-$$.sqlite"

FIXTURE_ROOT="${PLSP_DISCOVER_ROOT:-$INTEGRATION/fixtures/discover-java}"
EXPECTED="${PLSP_DISCOVER_EXPECTED:-$INTEGRATION/expected/discover-container-java.json}"
ROOT_ABS=$(CDPATH= cd -- "$FIXTURE_ROOT" && pwd)
MUSL_BIN=$(plsp_musl_bin)

USE_PLAIN="${PLSP_DISCOVER_PLAIN:-0}"
set +e
run_discover() {
  if [ "$USE_PLAIN" = "1" ]; then
    "$HARNESS" discover-container \
      --root "$ROOT_ABS" \
      --expected "$EXPECTED" \
      --platform "$PLATFORM" \
      --wal "$WAL" \
      "$@" \
      --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
      --discover-deadline-ms "${PLSP_DISCOVER_DEADLINE_MS:-15000}" \
      >"$OUT/discover-container-report.json" 2>"$OUT/discover-container-trace.log"
  else
    "$HARNESS" discover-container \
      --root "$ROOT_ABS" \
      --expected "$EXPECTED" \
      --platform "$PLATFORM" \
      --wal "$WAL" \
      "$@" \
      --emit-result-meta \
      --emit-timing \
      --fetch-trace-on-fail \
      --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
      --discover-deadline-ms "${PLSP_DISCOVER_DEADLINE_MS:-15000}" \
      >"$OUT/discover-container-report.json" 2>"$OUT/discover-container-trace.log"
  fi
}

if [ -f "$MUSL_BIN" ]; then
  run_discover --mount-serve-bin "$MUSL_BIN"
else
  run_discover
fi
STATUS=$?
set -e

echo "report: $OUT/discover-container-report.json"
echo "trace:  $OUT/discover-container-trace.log"
echo "serve wal: $WAL"

RESULT=$(grep -o '"result"[[:space:]]*:[[:space:]]*"[^"]*"' "$OUT/discover-container-report.json" | head -1 | sed 's/.*"\([^"]*\)"$/\1/' || true)
if [ "$RESULT" = "fail" ]; then
  exit 1
fi
exit "$STATUS"
