#!/bin/sh
# Container mux discover: same API path as POC IDE **Find Definition** (context menu).
# Requires: docker, progressive-lsp-runtime:local, plsp-it1 built.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INTEGRATION="$ROOT/integration"
HARNESS_MANIFEST="$INTEGRATION/harness/Cargo.toml"
WS_TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
OUT="${DISCOVER_CONTAINER_OUT:-$INTEGRATION/out}"

mkdir -p "$OUT"

if [ ! -x "$HARNESS" ]; then
  cargo build --manifest-path "$HARNESS_MANIFEST" --bin plsp-it1
  HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
  if [ ! -x "$HARNESS" ]; then
    HARNESS="$WS_TARGET/debug/plsp-it1"
  fi
fi

if ! command -v docker >/dev/null 2>&1; then
  echo '{"rpc":"discover-container","result":"skip","notes":"docker missing"}' >"$OUT/discover-container-report.json"
  exit 0
fi

if ! docker image inspect progressive-lsp-runtime:local >/dev/null 2>&1; then
  echo '{"rpc":"discover-container","result":"skip","notes":"progressive-lsp-runtime:local missing — run ./build lsp aarch64 --flavor dogfood"}' \
    >"$OUT/discover-container-report.json"
  exit 0
fi

PLATFORM="${PLSP_DOCKER_PLATFORM:-linux/arm64}"
LOG_DIR="${PLSP_DISCOVER_LOG_DIR:-$HOME/.progressivelsp/log}"
mkdir -p "$LOG_DIR"
WAL="$LOG_DIR/discover-container-$(date +%s)-$$.sqlite"

# In-tree fixture (CI). Override for dogfood PDF client:
#   PLSP_DISCOVER_ROOT=/path/to/supplytech-pdf-client/src
#   PLSP_DISCOVER_EXPECTED=integration/expected/discover-container-pdf.json  (optional custom golden)
FIXTURE_ROOT="${PLSP_DISCOVER_ROOT:-$INTEGRATION/fixtures/discover-java}"
EXPECTED="${PLSP_DISCOVER_EXPECTED:-$INTEGRATION/expected/discover-container-java.json}"

ROOT_ABS=$(CDPATH= cd -- "$FIXTURE_ROOT" && pwd)

MUSL_BIN="$ROOT/target/musl/aarch64-unknown-linux-musl/progressive-lsp"

set +e
if [ -f "$MUSL_BIN" ]; then
  "$HARNESS" discover-container \
    --root "$ROOT_ABS" \
    --expected "$EXPECTED" \
    --platform "$PLATFORM" \
    --wal "$WAL" \
    --mount-serve-bin "$MUSL_BIN" \
    --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
    --discover-deadline-ms "${PLSP_DISCOVER_DEADLINE_MS:-15000}" \
    >"$OUT/discover-container-report.json" 2>"$OUT/discover-container-trace.log"
else
  "$HARNESS" discover-container \
    --root "$ROOT_ABS" \
    --expected "$EXPECTED" \
    --platform "$PLATFORM" \
    --wal "$WAL" \
    --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
    --discover-deadline-ms "${PLSP_DISCOVER_DEADLINE_MS:-15000}" \
    >"$OUT/discover-container-report.json" 2>"$OUT/discover-container-trace.log"
fi
STATUS=$?
set -e

echo "report: $OUT/discover-container-report.json"
echo "trace:  $OUT/discover-container-trace.log"
echo "serve wal: $WAL"

RESULT=$(grep -o '"result"[[:space:]]*:[[:space:]]*"[^"]*"' "$OUT/discover-container-report.json" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')
if [ "$RESULT" = "fail" ]; then
  exit 1
fi
exit "$STATUS"
