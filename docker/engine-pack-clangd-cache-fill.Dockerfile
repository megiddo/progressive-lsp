# Dedicated clangd cache-fill. NOT the default `xtask pack` job.
# PR CI must not run this (must not compile LLVM from scratch on every run).
# Cache key = UPSTREAM_SHA + RUST_TARGET (the musl triple).
# Output dest after `xtask pack --cache-fill clangd`:
#   target/pack-cache/clangd/<sha>/<triple>/clangd
# Static archive graph is expected whack-a-mole (zlib, libxml2, ncurses, libffi,
# zstd, libedit, …). An unclosable .so is a documented miss — do not ship dynamic.
#
#   docker build --platform linux/arm64 \
#     --build-arg PACK=clangd --build-arg BINARY=clangd \
#     --build-arg UPSTREAM_REPO=https://github.com/llvm/llvm-project.git \
#     --build-arg UPSTREAM_SHA=<40-hex> \
#     --build-arg RUST_TARGET=aarch64-unknown-linux-musl \
#     --build-arg CACHE_KEY=<sha>:<triple> \
#     -f docker/engine-pack-clangd-cache-fill.Dockerfile \
#     --output type=local,dest=target/pack-cache/clangd/<sha>/<triple> .

FROM debian:bookworm AS build

ARG PACK=clangd
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG BINARY=clangd
ARG RUST_TARGET=x86_64-unknown-linux-musl
ARG CACHE_KEY

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${BINARY}" && test -n "${RUST_TARGET}" && test -n "${CACHE_KEY}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git cmake ninja-build python3 ca-certificates \
        musl-tools musl-dev g++ \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fetch
# Bounded retries: pack-container git clone can hit GitHub :443 timeout.
RUN set -eux; \
    n=0; \
    until git clone "${UPSTREAM_REPO}" src; do \
      n=$((n+1)); \
      echo "git clone retry ${n}/3 (${UPSTREAM_REPO})" >&2; \
      if [ "${n}" -ge 3 ]; then exit 1; fi; \
      rm -rf src; \
      sleep 5; \
    done; \
    git -C src checkout --detach "${UPSTREAM_SHA}"

# cmake LLVM/clangd — cache-fill path only. Fail closed; do not COPY a dynamic ELF.
WORKDIR /fetch/src
RUN mkdir -p build && cd build \
    && cmake -G Ninja ../llvm \
        -DCMAKE_BUILD_TYPE=MinSizeRel \
        -DCMAKE_C_COMPILER=musl-gcc \
        -DCMAKE_CXX_COMPILER=g++ \
        -DCMAKE_EXE_LINKER_FLAGS="-static" \
        -DLLVM_ENABLE_PROJECTS="clang;clang-tools-extra" \
        -DLLVM_TARGETS_TO_BUILD="X86;AArch64" \
        -DLLVM_ENABLE_ZLIB=OFF \
        -DLLVM_ENABLE_ZSTD=OFF \
        -DLLVM_ENABLE_LIBXML2=OFF \
        -DLLVM_ENABLE_TERMINFO=OFF \
        -DLLVM_ENABLE_LIBEDIT=OFF \
        -DCLANG_ENABLE_STATIC_ANALYZER=OFF \
        -DLLVM_BUILD_LLVM_DYLIB=OFF \
        -DLLVM_LINK_LLVM_DYLIB=OFF \
    && ninja clangd \
    && mkdir -p /out \
    && cp bin/clangd "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
