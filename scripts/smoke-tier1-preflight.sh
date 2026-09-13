#!/usr/bin/env bash
# PROD-5 tier 1 preflight (no GUI): image + fixture path hint.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$ROOT/integration/fixtures/smoke-python"

docker info >/dev/null
docker image inspect progressive-lsp-runtime:local >/dev/null

echo "tier-1 preflight OK: Docker + runtime image present"
echo "Manual: ./build run ide --folder \"$FIXTURE\" --container"
echo "Expect: T1 tree + Python T3 when ty is in the dogfood image."
