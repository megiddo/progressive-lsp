#!/bin/sh
# PD3 IT-3 orchestrator. Envelope control socket on P-java / P-py / P-ts.
# Darwin T3 stubs are skip_pack_missing — not typed-hover greens.
# --mux is pending_mux; do not silently retest the socket.
# Native cargo test is the unit gate. No Graphite. No push.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INTEGRATION="$ROOT/integration"
OUT="${IT3_OUT:-$INTEGRATION/out}"
HARNESS_MANIFEST="$INTEGRATION/harness/Cargo.toml"
CACHE="${PLSP_CORPORA_CACHE:-$INTEGRATION/corpora/.cache}"
EXPECTED="$INTEGRATION/expected"

mkdir -p "$OUT"
MODE=${1:-auto}

WS_TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
NATIVE_PLSP="$WS_TARGET/debug/progressive-lsp"
HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
if [ ! -x "$HARNESS" ]; then
    HARNESS="$WS_TARGET/debug/plsp-it1"
fi

pack_is_stub() {
    prefix=$1
    pack=$2
    [ -n "$pack" ] || return 1
    for f in "$prefix/engines/$pack"/*; do
        [ -f "$f" ] || continue
        if grep -q 'progressive-lsp-pack-stub:' "$f" 2>/dev/null; then
            return 0
        fi
    done
    return 1
}

build_native() {
    cargo build --manifest-path "$ROOT/Cargo.toml" --bin progressive-lsp
    cargo build --manifest-path "$HARNESS_MANIFEST" --bin plsp-it1
    HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
    if [ ! -x "$HARNESS" ]; then
        HARNESS="$WS_TARGET/debug/plsp-it1"
    fi
}

json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

append_row() {
    if [ "$FIRST" -eq 1 ]; then
        FIRST=0
    else
        ROWS="$ROWS,"
    fi
    ROWS="$ROWS$1"
}

run_progressive_row() {
    backend=$1
    root=$2
    expected=$3
    t3_pack=$4

    prefix=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it3-prefix.XXXXXX")
    work=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it3-ws.XXXXXX")
    cp -R "$root" "$work/tree"
    rm -rf "$work/tree/.progressivelsp" "$work/tree/.git"
    root="$work/tree"
    mkdir -p "$prefix/run" "$prefix/inbox" "$prefix/scripts"
    sock="$prefix/run/control.sock"

    if out=$("$HARNESS" progressive --backend "$backend" --root "$root" --expected "$expected" \
        --prefix "$prefix" --control-socket "$sock" --deadline-ms 25000 -- \
        "$PLSP" serve --prefix "$prefix" --control-socket "$sock" 2>"$OUT/it3-$backend.err"); then
        append_row "$out"
    else
        note=$(json_escape "$(tr '\n' ' ' < "$OUT/it3-$backend.err" | tail -c 240)")
        append_row "{\"backend\":\"$backend\",\"rpc\":\"IT-3\",\"result\":\"fail\",\"notes\":\"$note\"}"
    fi

    if [ -n "$t3_pack" ]; then
        if pack_is_stub "$prefix" "$t3_pack" || [ ! -d "$prefix/engines/$t3_pack" ]; then
            append_row "{\"backend\":\"$backend\",\"rpc\":\"IT-3.5-types\",\"result\":\"skip_pack_missing\",\"notes\":\"Darwin/CI stub or missing pack; not a T3 typed-hover green\"}"
        fi
    fi
}

write_report() {
    host=$(uname -s)
    arch=$(uname -m)
    gap_json="null"
    if [ -n "${2:-}" ]; then
        gap_json="\"$(json_escape "$2")\""
    fi
    cat > "$OUT/it3-report.json" <<EOF
{
  "host": "$host",
  "arch": "$arch",
  "mode": "$1",
  "gap": $gap_json,
  "rows": [$ROWS]
}
EOF
}

ROWS=""
FIRST=1

build_native
export PLSP="${PLSP:-$NATIVE_PLSP}"
export HARNESS

append_row "{\"backend\":\"P-java\",\"rpc\":\"IT-3.mux\",\"result\":\"pending_mux\",\"notes\":\"--mux is not implemented as an IT-3 gate; do not silently retest the socket\"}"

run_progressive_row P-java "$ROOT/fixtures/java-multi" "$EXPECTED/java-multi.json" ""

py_root="$ROOT/fixtures/matrix/python/3.14"
py_expected="$EXPECTED/matrix-python.json"
if [ -d "$CACHE/flask" ]; then
    py_root="$CACHE/flask"
    py_expected="$EXPECTED/flask.json"
fi
run_progressive_row P-py "$py_root" "$py_expected" "python"

ts_root="$ROOT/fixtures/matrix/typescript/5.9"
ts_expected="$EXPECTED/matrix-ts.json"
if [ -d "$CACHE/zod" ]; then
    ts_root="$CACHE/zod"
    ts_expected="$EXPECTED/zod.json"
fi
run_progressive_row P-ts "$ts_root" "$ts_expected" "tsgo"

gap=""
if [ "$(uname -s)" != "Linux" ]; then
    gap="Darwin host: T3 types rows skip_pack_missing on stub packs. Control socket tests run against native Mach-O. Linux CI + musl packs is the T3 gate. IT-3.mux is pending_mux."
    echo "$gap" > "$OUT/IT3_DARWIN_GAP.txt"
fi

write_report "$MODE" "$gap"
echo "IT-3 report: $OUT/it3-report.json"
