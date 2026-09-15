#!/bin/sh
# IT-TAM runner: docker mux + plsp-it1 tam-run (mirrors run-discover-container.sh).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INTEGRATION="$ROOT/integration"
TAM_DIR="$INTEGRATION/tier-api-matrix"
HARNESS_MANIFEST="$INTEGRATION/harness/Cargo.toml"
WS_TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
OUT="${TAM_OUT:-$INTEGRATION/out}"

SUITE_ID="${1:-hermetic-discover-java}"
case "$SUITE_ID" in
  java-junit4)
    SUITE="$TAM_DIR/suites/java-junit4.tam.yaml"
    FIXTURE_ROOT="${PLSP_TAM_ROOT:-}"
    if [ -z "$FIXTURE_ROOT" ]; then
      echo "java-junit4 requires PLSP_TAM_ROOT (junit4 checkout) until TAM-3 fetch lands" >&2
      FIXTURE_ROOT="$INTEGRATION/fixtures/discover-java"
    fi
    ;;
  hermetic-discover-java | *)
    SUITE="$TAM_DIR/suites/hermetic-discover-java.tam.yaml"
    FIXTURE_ROOT="${PLSP_TAM_ROOT:-$INTEGRATION/fixtures/discover-java}"
    ;;
esac

mkdir -p "$OUT"

if [ ! -x "$HARNESS" ]; then
  CARGO_HOME="${CARGO_HOME:-$ROOT/.cargo-home}" CARGO_TARGET_DIR="$WS_TARGET" \
    cargo build --manifest-path "$HARNESS_MANIFEST" --bin plsp-it1
  HARNESS="$WS_TARGET/debug/plsp-it1"
  if [ ! -x "$HARNESS" ]; then
    HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
  fi
fi

ROOT_ABS=$(CDPATH= cd -- "$FIXTURE_ROOT" && pwd)
REPORT="$OUT/tam-${SUITE_ID}-report.json"
PLATFORM="${PLSP_DOCKER_PLATFORM:-linux/arm64}"
MUSL_BIN="$ROOT/target/musl/aarch64-unknown-linux-musl/progressive-lsp"

MOUNT_ARGS=""
if [ -f "$MUSL_BIN" ]; then
  MOUNT_ARGS="--mount-serve-bin $MUSL_BIN"
fi

set +e
# shellcheck disable=SC2086
"$HARNESS" tam-run \
  --suite "$SUITE" \
  --workspace-root "$ROOT_ABS" \
  --platform "$PLATFORM" \
  $MOUNT_ARGS \
  --init-deadline-ms "${PLSP_INIT_DEADLINE_MS:-600000}" \
  --request-deadline-ms "${PLSP_TAM_REQUEST_DEADLINE_MS:-15000}" \
  --quiet \
  >"$REPORT" 2>"$OUT/tam-${SUITE_ID}-trace.log"
STATUS=$?
set -e

echo "report: $REPORT"
echo "trace:  $OUT/tam-${SUITE_ID}-trace.log"

RESULT=$(grep -o '"result"[[:space:]]*:[[:space:]]*"[^"]*"' "$REPORT" | head -1 | sed 's/.*"\([^"]*\)"$/\1/' || true)
if [ "$RESULT" = "fail" ]; then
  exit 1
fi
if [ "$RESULT" = "skip" ]; then
  echo "tam-run skipped (docker/image): see report notes" >&2
  exit 0
fi
exit "$STATUS"
