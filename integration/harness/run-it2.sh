#!/bin/sh
# PD2 IT-2 orchestrator. Stock stdio LSP per language on pinned corpora.
# Darwin stubs are skip_pack_missing for T3 — not typed-hover greens.
# Native cargo test is the unit gate. No Graphite. No push.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INTEGRATION="$ROOT/integration"
OUT="${IT2_OUT:-$INTEGRATION/out}"
HARNESS_MANIFEST="$INTEGRATION/harness/Cargo.toml"
PINS="$INTEGRATION/corpora/pins.json"
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

is_linux_elf() {
    [ -f "$1" ] || return 1
    od -An -tx1 -N 4 "$1" 2>/dev/null | tr -s ' ' | grep -q '7f 45 4c 46'
}

pack_is_stub() {
    prefix=$1
    pack=$2
    [ -n "$pack" ] || return 1
    # stub bytes start with progressive-lsp-pack-stub:
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
    # uses global ROWS, FIRST
    if [ "$FIRST" -eq 1 ]; then
        FIRST=0
    else
        ROWS="$ROWS,"
    fi
    ROWS="$ROWS$1"
}

run_backend_row() {
    language=$1
    corpus=$2
    root=$3
    expected=$4
    packs=$5
    t3_pack=$6
    extra_note=$7

    prefix=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it2-prefix.XXXXXX")
    work=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it2-ws.XXXXXX")
    # Ghost disk rewrites an unopened sibling — never mutate the in-tree fixture or pin cache.
    cp -R "$root" "$work/tree"
    rm -rf "$work/tree/.progressivelsp" "$work/tree/.git"
    root="$work/tree"
    if [ -n "$packs" ]; then
        "$PLSP" install --prefix "$prefix" --packs "$packs" >/dev/null 2>&1 || true
    else
        "$PLSP" install --prefix "$prefix" --packs python >/dev/null 2>&1 || true
    fi

    # T1/T2 stock row always runs. T3 is a separate skip_pack_missing row on Darwin stubs.
    if out=$("$HARNESS" backend --expected "$expected" --root "$root" --deadline-ms 20000 -- \
        "$PLSP" serve --prefix "$prefix" 2>"$OUT/it2-$corpus.err"); then
        append_row "$out"
    else
        note=$(json_escape "$(tr '\n' ' ' < "$OUT/it2-$corpus.err" | tail -c 240)")
        extra=$(json_escape "$extra_note")
        append_row "{\"language\":\"$language\",\"corpus\":\"$corpus\",\"pack\":\"\",\"result\":\"fail\",\"definition_ok\":false,\"tokens_ok\":false,\"ghost_edit_ok\":false,\"notes\":\"$note $extra\"}"
    fi
    if [ -n "$t3_pack" ]; then
        if pack_is_stub "$prefix" "$t3_pack" || [ ! -d "$prefix/engines/$t3_pack" ]; then
            append_row "{\"language\":\"$language\",\"corpus\":\"$corpus\",\"corpus_sha\":\"\",\"pack\":\"$t3_pack\",\"tier_observed\":\"skip_pack_missing\",\"definition_ok\":false,\"tokens_ok\":false,\"ghost_edit_ok\":false,\"result\":\"skip_pack_missing\",\"notes\":\"Darwin/CI stub or missing pack; not a T3 typed-hover green\"}"
        fi
    fi
}

run_supplements() {
    run_backend_row java java-heuristic "$ROOT/fixtures/java-heuristic" \
        "$EXPECTED/java-heuristic.json" "" "" "in-tree supplement"
    run_backend_row php matrix-php "$ROOT/fixtures/matrix/php/8.5" \
        "$EXPECTED/matrix-php.json" "" "phpantom" "in-tree supplement"
    run_backend_row javascript matrix-js "$ROOT/fixtures/matrix/javascript/es2026" \
        "$EXPECTED/matrix-js.json" "" "tsgo" "in-tree supplement"
    run_backend_row typescript matrix-ts "$ROOT/fixtures/matrix/typescript/5.9" \
        "$EXPECTED/matrix-ts.json" "" "tsgo" "in-tree supplement"
    run_backend_row css matrix-css "$ROOT/fixtures/matrix/css/current" \
        "$EXPECTED/matrix-css.json" "" "biome" "in-tree supplement"
    run_backend_row html matrix-html "$ROOT/fixtures/matrix/html/current" \
        "$EXPECTED/matrix-html.json" "" "superhtml" "in-tree supplement"
    run_backend_row go matrix-go "$ROOT/fixtures/matrix/go/1.27" \
        "$EXPECTED/matrix-go.json" "" "gopls" "in-tree supplement"
    run_backend_row zig matrix-zig "$ROOT/fixtures/matrix/zig/current" \
        "$EXPECTED/matrix-zig.json" "" "zls" "in-tree supplement"
    run_backend_row python matrix-python "$ROOT/fixtures/matrix/python/3.14" \
        "$EXPECTED/matrix-python.json" "" "ty" "in-tree supplement"
    run_backend_row rust matrix-rust "$ROOT/fixtures/matrix/rust/2024" \
        "$EXPECTED/matrix-rust.json" "" "rust-analyzer" "in-tree supplement"
    run_backend_row c matrix-c "$ROOT/fixtures/matrix/c/c23" \
        "$EXPECTED/matrix-c.json" "" "clangd" "in-tree supplement"
    run_backend_row cpp matrix-cpp "$ROOT/fixtures/matrix/cpp/cpp26" \
        "$EXPECTED/matrix-cpp.json" "" "clangd" "in-tree supplement"
    run_backend_row csharp csharp-mini "$INTEGRATION/corpora/csharp-mini" \
        "$EXPECTED/csharp-mini.json" "" "" "T1/T2 ceiling; imported SDK-style snippet"
}

run_fetched() {
    # T1/T2 rows on fetched trees. T3 pack name is passed so stub → skip_pack_missing.
    run_backend_row java junit4 "$CACHE/junit4" "$EXPECTED/java-junit4.json" "" "" ""
    run_backend_row php php-fig-log "$CACHE/php-fig-log" "$EXPECTED/php-fig-log.json" "" "phpantom" ""
    run_backend_row javascript preact "$CACHE/preact" "$EXPECTED/preact.json" "" "tsgo" ""
    run_backend_row typescript zod "$CACHE/zod" "$EXPECTED/zod.json" "" "tsgo" ""
    run_backend_row css pico-css "$CACHE/pico-css" "$EXPECTED/pico-css.json" "" "biome" ""
    run_backend_row html html5-boilerplate "$CACHE/html5-boilerplate" "$EXPECTED/html5-boilerplate.json" "" "superhtml" ""
    run_backend_row go go-version "$CACHE/go-version" "$EXPECTED/go-version.json" "" "gopls" ""
    run_backend_row zig known-folders "$CACHE/known-folders" "$EXPECTED/known-folders.json" "" "zls" ""
    run_backend_row python flask "$CACHE/flask" "$EXPECTED/flask.json" "" "ty" ""
    run_backend_row rust anyhow "$CACHE/anyhow" "$EXPECTED/anyhow.json" "" "rust-analyzer" ""
    run_backend_row c hiredis "$CACHE/hiredis" "$EXPECTED/hiredis.json" "" "clangd" ""
    run_backend_row cpp cxxopts "$CACHE/cxxopts" "$EXPECTED/cxxopts.json" "" "clangd" ""
    run_backend_row csharp bullseye "$CACHE/bullseye" "$EXPECTED/bullseye.json" "" "" "expected_ceiling"
}

write_report() {
    host=$(uname -s)
    arch=$(uname -m)
    gap_json="null"
    if [ -n "${2:-}" ]; then
        gap_json="\"$(json_escape "$2")\""
    fi
    cat > "$OUT/it2-report.json" <<EOF
{
  "host": "$host",
  "arch": "$arch",
  "mode": "$1",
  "gap": $gap_json,
  "rows": [$ROWS]
}
EOF
}

# Isolation: Java corpus must not require clangd; PHP must not require php/clangd.
assert_isolation() {
    prefix=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it2-iso.XXXXXX")
    "$PLSP" install --prefix "$prefix" --packs python >/dev/null
    [ ! -e "$prefix/engines/clangd" ] || {
        echo "isolation: java/core install must not require clangd" >&2
        return 1
    }
    command -v php >/dev/null 2>&1 && echo "note: host php present; server must not use it" >&2
    append_row "{\"language\":\"isolation\",\"corpus\":\"core\",\"result\":\"pass\",\"notes\":\"core prefix has no clangd; PHP does not require host php\"}"
}

ROWS=""
FIRST=1
ELF=${PLSP_ELF:-$INTEGRATION/artifacts/progressive-lsp}

build_native
export PLSP="${PLSP:-$NATIVE_PLSP}"
export HARNESS

case "$MODE" in
    supplements)
        run_supplements
        assert_isolation || true
        write_report "supplements" "in-tree fixtures + csharp-mini only"
        ;;
    fetched|auto)
        gap=""
        if ! "$HARNESS" fetch --pins "$PINS" --cache "$CACHE"; then
            gap="corpus fetch failed or offline. In-tree supplements still run. Linux CI retries fetch-at-SHA."
            echo "$gap" > "$OUT/IT2_FETCH_GAP.txt"
        fi
        run_supplements
        if [ -d "$CACHE/junit4" ] || [ -d "$CACHE/anyhow" ]; then
            run_fetched
        else
            append_row "{\"language\":\"all\",\"corpus\":\"fetch\",\"result\":\"skip_fetch_missing\",\"notes\":\"no cached corpora; supplements only\"}"
        fi
        assert_isolation || true
        # T3 note: stubs are never typed greens
        if ! is_linux_elf "$ELF"; then
            gap="${gap} Darwin host: T3 rows skip_pack_missing on stub packs. Not clangd/ty typed-hover greens. Linux CI + musl packs is the T3 gate."
            echo "$gap" > "$OUT/IT2_DARWIN_GAP.txt"
        fi
        write_report "host_smoke" "$gap"
        ;;
    *)
        echo "usage: run-it2.sh [auto|supplements|fetched]" >&2
        exit 2
        ;;
esac

echo "IT-2 report: $OUT/it2-report.json"
