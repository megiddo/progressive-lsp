#!/bin/sh
# PD1 IT-1 orchestrator. Linux CI + prebuilt musl ELF + Docker is the real gate.
# Darwin without Docker or without a Linux ELF: host_smoke only.
# Never claim IT-1.1 green on a Mach-O.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INTEGRATION="$ROOT/integration"
COMPOSE="$INTEGRATION/compose.yaml"
OUT="${IT1_OUT:-$INTEGRATION/out}"
HARNESS_MANIFEST="$INTEGRATION/harness/Cargo.toml"
DISTROS="arch rhel debian ubuntu"

mkdir -p "$OUT"
MODE=${1:-auto}

is_linux_elf() {
    [ -f "$1" ] || return 1
    # ELF magic 0x7f E L F — do not treat Mach-O as a distro artifact.
    od -An -tx1 -N 4 "$1" 2>/dev/null | tr -s ' ' | grep -q '7f 45 4c 46'
}

docker_ok() {
    docker info >/dev/null 2>&1
}

host_ident() {
    uname -s
}

build_native() {
    cargo build --manifest-path "$ROOT/Cargo.toml" --bin progressive-lsp
    cargo build --manifest-path "$HARNESS_MANIFEST" --bin plsp-it1
}

WS_TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
NATIVE_PLSP="$WS_TARGET/debug/progressive-lsp"
HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
if [ ! -x "$HARNESS" ]; then
    HARNESS="$WS_TARGET/debug/plsp-it1"
fi

write_report() {
    # $1 = json rows array (already formatted)
    # $2 = mode
    # $3 = gap (may be empty)
    host=$(host_ident)
    arch=$(uname -m)
    gap_json="null"
    if [ -n "${3:-}" ]; then
        gap_json=$(printf '%s' "$3" | sed 's/"/\\"/g')
        gap_json="\"$gap_json\""
    fi
    cat > "$OUT/it1-report.json" <<EOF
{
  "host": "$host",
  "arch": "$arch",
  "mode": "$2",
  "gap": $gap_json,
  "rows": $1
}
EOF
}

run_host_smoke() {
    build_native
    HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
    if [ ! -x "$HARNESS" ]; then
        HARNESS="$WS_TARGET/debug/plsp-it1"
    fi
    BASE=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it1-host.XXXXXX")
    export PLSP="$NATIVE_PLSP"
    export HARNESS
    export BASE
    export WORK_HOST=$BASE
    export RUNNER=host
    # shellcheck source=cases.sh
    . "$INTEGRATION/harness/cases.sh"
    rows="["
    first=1
    for case in it12 it13 it14 it15 it17 host_handshake; do
        result=pass
        if ! run_case "$case"; then
            result=fail
        fi
        if [ "$first" -eq 1 ]; then
            first=0
        else
            rows="$rows,"
        fi
        rows="$rows{\"id\":\"host_smoke.$case\",\"distro\":\"darwin-host\",\"result\":\"$result\"}"
    done
    rows="$rows]"
    echo "$rows"
}

skip_matrix_rows() {
    reason=$1
    rows="["
    first=1
    for d in $DISTROS; do
        for id in IT-1.1 IT-1.2 IT-1.3 IT-1.4 IT-1.5 IT-1.6 IT-1.7; do
            if [ "$first" -eq 1 ]; then
                first=0
            else
                rows="$rows,"
            fi
            rows="$rows{\"id\":\"$id\",\"distro\":\"$d\",\"result\":\"skip_darwin_gap\",\"reason\":\"$reason\"}"
        done
    done
    rows="$rows]"
    echo "$rows"
}

run_docker_matrix() {
    elf=$1
    work=$(mktemp -d "${TMPDIR:-/tmp}/plsp-it1-work.XXXXXX")
    export PLSP_ELF=$elf
    export IT1_WORK=$work
    export HARNESS
    export COMPOSE
    export INTEGRATION
    build_native
    HARNESS="$INTEGRATION/harness/target/debug/plsp-it1"
    if [ ! -x "$HARNESS" ]; then
        HARNESS="$WS_TARGET/debug/plsp-it1"
    fi
    docker compose -f "$COMPOSE" pull
    docker compose -f "$COMPOSE" up -d --wait || docker compose -f "$COMPOSE" up -d
    rows="["
    first=1
    trap 'docker compose -f "$COMPOSE" down --remove-orphans >/dev/null 2>&1 || true' EXIT
    for d in $DISTROS; do
        export SERVICE=$d
        export RUNNER=docker
        export BASE=/it1
        export WORK_HOST=$work
        export PLSP=/opt/plsp/progressive-lsp
        # shellcheck source=cases.sh
        . "$INTEGRATION/harness/cases.sh"
        for case in it11 it12 it13 it14 it15 it16 it17; do
            result=pass
            if ! run_case "$case"; then
                result=fail
            fi
            id=$(echo "$case" | sed 's/it1/IT-1./')
            if [ "$first" -eq 1 ]; then
                first=0
            else
                rows="$rows,"
            fi
            rows="$rows{\"id\":\"$id\",\"distro\":\"$d\",\"result\":\"$result\"}"
        done
    done
    rows="$rows]"
    docker compose -f "$COMPOSE" down --remove-orphans >/dev/null 2>&1 || true
    trap - EXIT
    echo "$rows"
}

ELF=${PLSP_ELF:-$INTEGRATION/artifacts/progressive-lsp}

case "$MODE" in
    host_smoke)
        rows=$(run_host_smoke)
        write_report "$rows" "host_smoke" "host_smoke only (explicit). IT-1.1–1.6 remain Linux CI."
        echo "IT-1 host_smoke report: $OUT/it1-report.json"
        ;;
    matrix)
        if ! docker_ok; then
            echo "docker is required for matrix mode" >&2
            exit 2
        fi
        if ! is_linux_elf "$ELF"; then
            echo "PLSP_ELF is not a Linux ELF: $ELF" >&2
            exit 2
        fi
        rows=$(run_docker_matrix "$ELF")
        write_report "$rows" "matrix" ""
        echo "IT-1 matrix report: $OUT/it1-report.json"
        ;;
    auto|*)
        if docker_ok && is_linux_elf "$ELF"; then
            rows=$(run_docker_matrix "$ELF")
            write_report "$rows" "matrix" ""
        else
            gap="Docker daemon unavailable and/or no prebuilt musl ELF at $ELF. Native cargo test is the unit gate. IT-1.1–1.6 run in Linux CI/Docker — not on a Darwin Mach-O."
            smoke=$(run_host_smoke)
            skips=$(skip_matrix_rows "darwin_ci_gap")
            # merge smoke + skips
            smoke_trim=$(printf '%s' "$smoke" | sed 's/^\[//;s/\]$//')
            skips_trim=$(printf '%s' "$skips" | sed 's/^\[//;s/\]$//')
            rows="[$smoke_trim,$skips_trim]"
            write_report "$rows" "host_smoke" "$gap"
            echo "$gap" > "$OUT/DARWIN_CI_GAP.txt"
            echo "IT-1 Darwin gap documented: $OUT/DARWIN_CI_GAP.txt"
        fi
        echo "IT-1 report: $OUT/it1-report.json"
        ;;
esac
