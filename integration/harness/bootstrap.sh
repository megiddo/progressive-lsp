#!/bin/sh
# Shared env + auto-recovery for plsp-it1 integration scripts. Source, do not execute.
set -eu

plsp_repo_root() {
  if [ -n "${PLSP_REPO_ROOT:-}" ] && [ -d "$PLSP_REPO_ROOT" ]; then
    printf '%s\n' "$PLSP_REPO_ROOT"
    return 0
  fi
  # harness/bootstrap.sh -> harness -> integration -> repo
  ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
  PLSP_REPO_ROOT=$ROOT
  export PLSP_REPO_ROOT
  printf '%s\n' "$ROOT"
}

plsp_export_cargo_env() {
  ROOT=$(plsp_repo_root)
  export CARGO_TARGET_DIR="$ROOT/target"
  export CARGO_HOME="${CARGO_HOME:-$ROOT/.cargo-home}"
  mkdir -p "$CARGO_HOME"
}

plsp_host_lsp_arch() {
  case "$(uname -m)" in
    arm64 | aarch64) printf '%s\n' aarch64 ;;
    x86_64 | amd64) printf '%s\n' x86_64 ;;
    *) printf '%s\n' aarch64 ;;
  esac
}

plsp_docker_platform() {
  case "$(plsp_host_lsp_arch)" in
    aarch64) printf '%s\n' linux/arm64 ;;
    *) printf '%s\n' linux/amd64 ;;
  esac
}

plsp_musl_bin() {
  ROOT=$(plsp_repo_root)
  ARCH=$(plsp_host_lsp_arch)
  printf '%s\n' "$ROOT/target/musl/${ARCH}-unknown-linux-musl/progressive-lsp"
}

plsp_docker_ok() {
  command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1
}

plsp_runtime_image_ok() {
  docker image inspect progressive-lsp-runtime:local >/dev/null 2>&1
}

# Builds plsp-it1 if missing. Prints absolute path to binary.
plsp_ensure_harness_bin() {
  plsp_export_cargo_env
  ROOT=$(plsp_repo_root)
  BIN="$CARGO_TARGET_DIR/debug/plsp-it1"
  if [ -x "$BIN" ]; then
    printf '%s\n' "$BIN"
    return 0
  fi
  echo "bootstrap: building plsp-it1 (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)..." >&2
  cargo build --manifest-path "$ROOT/integration/harness/Cargo.toml" --bin plsp-it1
  if [ ! -x "$BIN" ]; then
    echo "bootstrap: plsp-it1 missing after build: $BIN" >&2
    exit 1
  fi
  printf '%s\n' "$BIN"
}

# Ensures musl controller exists for --mount-serve-bin (best effort before docker integ).
plsp_ensure_musl_controller() {
  plsp_export_cargo_env
  ROOT=$(plsp_repo_root)
  MUSL=$(plsp_musl_bin)
  if [ -f "$MUSL" ]; then
    return 0
  fi
  if ! plsp_docker_ok; then
    echo "bootstrap: musl progressive-lsp missing and docker unavailable; container tests may use image-only serve" >&2
    return 0
  fi
  ARCH=$(plsp_host_lsp_arch)
  echo "bootstrap: building musl progressive-lsp (./build lsp $ARCH)..." >&2
  "$ROOT/build" lsp "$ARCH"
}

# Ensures progressive-lsp-runtime:local exists; runs dogfood lsp build when docker works.
plsp_ensure_runtime_image() {
  if plsp_runtime_image_ok; then
    return 0
  fi
  if ! plsp_docker_ok; then
    echo "bootstrap: docker unavailable; skipping runtime image build" >&2
    return 1
  fi
  ROOT=$(plsp_repo_root)
  ARCH=$(plsp_host_lsp_arch)
  echo "bootstrap: progressive-lsp-runtime:local missing — running ./build lsp $ARCH --flavor dogfood ..." >&2
  "$ROOT/build" lsp "$ARCH" --flavor dogfood
  plsp_runtime_image_ok
}

plsp_fixture_discover_java() {
  ROOT=$(plsp_repo_root)
  CDPATH= cd -- "$ROOT/integration/fixtures/discover-java" && pwd
}
