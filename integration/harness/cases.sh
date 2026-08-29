#!/bin/sh
# IT-1.1–1.7 case bodies. Sourced by run-it1.sh.
# Required env: PLSP HARNESS BASE WORK_HOST RUNNER
# Docker also: SERVICE COMPOSE

set -eu

wh() { echo "$WORK_HOST/$1"; }
ws() { echo "$BASE/$1"; }

run_bin() {
    # run_bin HOME [PROGRESSIVE_LSP_HOME|-] args...
    home=$1
    shift
    envhome=$1
    shift
    if [ "$RUNNER" = docker ]; then
        if [ "$envhome" = "-" ]; then
            docker compose -f "$COMPOSE" exec -T -e HOME="$home" -e PROGRESSIVE_LSP_HOME= \
                "$SERVICE" "$@"
        else
            docker compose -f "$COMPOSE" exec -T -e HOME="$home" -e PROGRESSIVE_LSP_HOME="$envhome" \
                "$SERVICE" "$@"
        fi
    else
        if [ "$envhome" = "-" ]; then
            env -u PROGRESSIVE_LSP_HOME HOME="$home" "$@"
        else
            env HOME="$home" PROGRESSIVE_LSP_HOME="$envhome" "$@"
        fi
    fi
}

assert_layout() {
    prefix_host=$1
    for d in bin engines cache log run scripts workspaces; do
        [ -d "$prefix_host/$d" ] || { echo "missing dir $prefix_host/$d" >&2; return 1; }
    done
    [ -f "$prefix_host/config.toml" ] || { echo "missing config.toml" >&2; return 1; }
    [ -f "$prefix_host/installed-packs.toml" ] || { echo "missing installed-packs.toml" >&2; return 1; }
}

handshake() {
    home=$1
    envhome=$2
    root=$3
    extra=
    if [ -n "$root" ]; then
        extra="--root-uri file://$root"
    fi
    # shellcheck disable=SC2086
    if [ "$RUNNER" = docker ]; then
        if [ "$envhome" = "-" ]; then
            $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
                docker compose -f "$COMPOSE" exec -T -e HOME="$home" \
                "$SERVICE" "$PLSP" serve >/dev/null
        else
            $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
                docker compose -f "$COMPOSE" exec -T -e HOME="$home" \
                -e PROGRESSIVE_LSP_HOME="$envhome" \
                "$SERVICE" "$PLSP" serve ${4:-} >/dev/null
        fi
    else
        if [ "$envhome" = "-" ]; then
            env -u PROGRESSIVE_LSP_HOME HOME="$home" \
                $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
                "$PLSP" serve ${4:-} >/dev/null
        else
            env HOME="$home" PROGRESSIVE_LSP_HOME="$envhome" \
                $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
                "$PLSP" serve ${4:-} >/dev/null
        fi
    fi
}

# Optional 4th arg to handshake is extra serve args — handled poorly above.
# Dedicated helper for --prefix:
handshake_prefix() {
    home=$1
    envhome=$2
    root=$3
    prefix=$4
    extra=
    if [ -n "$root" ]; then
        extra="--root-uri file://$root"
    fi
    # shellcheck disable=SC2086
    if [ "$RUNNER" = docker ]; then
        $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
            docker compose -f "$COMPOSE" exec -T -e HOME="$home" \
            -e PROGRESSIVE_LSP_HOME="$envhome" \
            "$SERVICE" "$PLSP" serve --prefix "$prefix" >/dev/null
    else
        env HOME="$home" PROGRESSIVE_LSP_HOME="$envhome" \
            $HARNESS handshake --assert-stock --deadline-ms 15000 $extra -- \
            "$PLSP" serve --prefix "$prefix" >/dev/null
    fi
}

case_it11() {
    home_h=$(wh home)
    home_s=$(ws home)
    mkdir -p "$home_h/empty"
    run_bin "$home_s" "-" "$PLSP" install --prefix "$home_s/.progressivelsp" --packs python
    assert_layout "$home_h/.progressivelsp"
    if [ "$RUNNER" = docker ]; then
        run_bin "$home_s" "-" sh -c 'command -v node && exit 1; command -v java && exit 1; command -v python3 && exit 1; command -v php && exit 1; exit 0'
    fi
    handshake "$home_s" "$home_s/.progressivelsp" "$home_s/empty"
}

case_it12() {
    alice_h=$(wh alice)
    alice_s=$(ws alice)
    mkdir -p "$alice_h"
    handshake "$alice_s" "-" "$alice_s/ws"
    [ -d "$alice_h/.progressivelsp/bin" ] || { echo "IT-1.2 missing default home" >&2; return 1; }
    [ -f "$alice_h/.progressivelsp/config.toml" ] || { echo "IT-1.2 missing config" >&2; return 1; }
}

case_it13() {
    env_h=$(wh envhome)
    clip_h=$(wh cliprefix)
    env_s=$(ws envhome)
    clip_s=$(ws cliprefix)
    mkdir -p "$env_h" "$clip_h"
    handshake_prefix "$(ws home)" "$env_s" "" "$clip_s"
    [ -f "$clip_h/config.toml" ] || { echo "IT-1.3 CLI prefix did not win" >&2; return 1; }
    [ ! -f "$env_h/config.toml" ] || { echo "IT-1.3 wrote env home" >&2; return 1; }
}

case_it14() {
    home_h=$(wh overlay-home)
    ws_h=$(wh overlay-ws)
    home_s=$(ws overlay-home)
    ws_s=$(ws overlay-ws)
    mkdir -p "$home_h/.progressivelsp" "$ws_h/.progressivelsp"
    printf 'packs = ["rust"]\n' > "$home_h/.progressivelsp/config.toml"
    printf 'packs = ["python"]\nfuture = 1\n' > "$ws_h/.progressivelsp/config.toml"
    handshake_prefix "$(ws home)" "-" "$ws_s" "$home_s/.progressivelsp"
    grep -q 'python' "$ws_h/.progressivelsp/config.toml"
    grep -q 'rust' "$home_h/.progressivelsp/config.toml"
    [ ! -d "$ws_h/.progressivelsp/cache" ] || { echo "IT-1.4 cache under workspace" >&2; return 1; }
    [ -d "$home_h/.progressivelsp/cache" ] || { echo "IT-1.4 cache missing in prefix" >&2; return 1; }
}

case_it15() {
    ws_h=$(wh git-ws)
    home_h=$(wh git-home)
    ws_s=$(ws git-ws)
    home_s=$(ws git-home)
    mkdir -p "$ws_h" "$home_h"
    git init "$ws_h" >/dev/null
    printf '/target\n' > "$ws_h/.gitignore"
    if command -v git >/dev/null; then
        git -C "$ws_h" add .gitignore
        git -C "$ws_h" -c user.email=it1@example.com -c user.name=it1 commit -m init >/dev/null
    fi
    before=$(od -An -tx1 "$ws_h/.gitignore")
    handshake_prefix "$(ws home)" "-" "$ws_s" "$home_s/.progressivelsp"
    after=$(od -An -tx1 "$ws_h/.gitignore")
    [ "$before" = "$after" ] || { echo "IT-1.5 edited project .gitignore" >&2; return 1; }
    grep -q '.progressivelsp/cache/' "$ws_h/.git/info/exclude" || {
        echo "IT-1.5 missing git exclude" >&2
        return 1
    }
}

case_it16() {
    [ "$RUNNER" = docker ] || { echo "IT-1.6 is Docker/ELF only" >&2; return 1; }
    # ldd must fail or say not a dynamic executable. Never treat Mach-O as green.
    if run_bin /tmp "-" ldd /opt/plsp/progressive-lsp; then
        echo "IT-1.6 ldd succeeded (dynamic?)" >&2
        return 1
    fi
    handshake "$(ws home)" "-" ""
}

case_it17() {
    if run_bin "$(ws home)" "-" "$PLSP"; then
        echo "IT-1.7 bare binary must be non-zero" >&2
        return 1
    fi
    if run_bin "$(ws home)" "-" "$PLSP" help; then
        echo "IT-1.7 help must be non-zero" >&2
        return 1
    fi
    if run_bin "$(ws home)" "-" "$PLSP" serve --nope; then
        echo "IT-1.7 serve --nope must be non-zero" >&2
        return 1
    fi
}

case_host_handshake() {
    mkdir -p "$(wh smoke)/ws"
    handshake "$(ws smoke)" "-" "$(ws smoke)/ws"
}

run_case() {
    echo "== $1 ($RUNNER ${SERVICE:-host}) ==" >&2
    case "$1" in
        it11) case_it11 ;;
        it12) case_it12 ;;
        it13) case_it13 ;;
        it14) case_it14 ;;
        it15) case_it15 ;;
        it16) case_it16 ;;
        it17) case_it17 ;;
        host_handshake) case_host_handshake ;;
        *) echo "unknown case $1" >&2; return 1 ;;
    esac
}

# Allow WORK_HOST to default to BASE for host_smoke.
WORK_HOST=${WORK_HOST:-$BASE}
