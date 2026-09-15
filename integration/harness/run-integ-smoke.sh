#!/bin/sh
# Integration smoke entry — same as ./build integ (result tree printed by xtask).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
exec "$ROOT/build" integ "$@"
