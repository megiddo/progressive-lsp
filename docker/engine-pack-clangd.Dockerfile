# Content-addressed clangd. Cache hit COPY only. Never cmake.
# Cache key = UPSTREAM_SHA + RUST_TARGET (the musl triple).
# `xtask pack --pack clangd` copies
#   target/pack-cache/clangd/<sha>/<triple>/clangd
# to dest when that file exists. Missing cache is a documented miss — not cmake.
# This Dockerfile COPYs a staged cache/${BINARY} from the build context.
# Default pack does not invoke this file on a cache miss.
# PR CI must not compile LLVM from scratch.

FROM scratch
ARG BINARY=clangd
COPY cache/${BINARY} /${BINARY}
