# Hermetic slim engine pack (Zig): superhtml.
# Zig toolchain exists only inside this build container — not a shipped .so.
# Fetch at the pinned git SHA inside this image — not a Mac host product step.
# Parameterized by PACK + UPSTREAM_SHA + ZIG_TARGET / ZIG_ARCH.
#
#   docker build --platform linux/arm64 \
#     --build-arg PACK=superhtml --build-arg BINARY=superhtml \
#     --build-arg UPSTREAM_REPO=https://github.com/kristoff-it/superhtml.git \
#     --build-arg UPSTREAM_SHA=<40-hex> \
#     --build-arg ZIG_VERSION=0.15.1 --build-arg ZIG_ARCH=aarch64 \
#     --build-arg ZIG_TARGET=aarch64-linux-musl --build-arg ZIG_SHA256=<sha256> \
#     -f docker/engine-pack-zig.Dockerfile \
#     --output type=local,dest=target/musl/aarch64-unknown-linux-musl/engines/superhtml .

FROM alpine:3.20 AS build

ARG PACK
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG BINARY=superhtml
ARG ZIG_VERSION
ARG ZIG_ARCH
ARG ZIG_TARGET
ARG ZIG_SHA256

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${BINARY}" && test -n "${ZIG_VERSION}" && test -n "${ZIG_ARCH}" \
    && test -n "${ZIG_TARGET}" && test -n "${ZIG_SHA256}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown"

RUN apk add --no-cache git curl xz

RUN curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-${ZIG_ARCH}-linux-${ZIG_VERSION}.tar.xz" \
        -o /tmp/zig.tar.xz \
    && echo "${ZIG_SHA256}  /tmp/zig.tar.xz" | sha256sum -c \
    && mkdir -p /opt/zig \
    && tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1 \
    && rm /tmp/zig.tar.xz
ENV PATH="/opt/zig:${PATH}"

WORKDIR /fetch
RUN git clone "${UPSTREAM_REPO}" src \
    && cd src \
    && git checkout --detach "${UPSTREAM_SHA}" \
    && git submodule update --init --recursive

WORKDIR /fetch/src
# qemu/amd64: zig's parallel options cache hits error.Unexpected on access().
# Single-job + /tmp cache is the Darwin Desktop workaround; native linux/amd64
# does not need this. aarch64 (native here) already extracts statically.
ENV ZIG_LOCAL_CACHE_DIR=/tmp/zig-local
ENV ZIG_GLOBAL_CACHE_DIR=/tmp/zig-global
RUN mkdir -p "${ZIG_LOCAL_CACHE_DIR}" "${ZIG_GLOBAL_CACHE_DIR}" \
    && zig build -j1 -Doptimize=ReleaseSafe -Dtarget="${ZIG_TARGET}" \
    && mkdir -p /out \
    && cp zig-out/bin/superhtml "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
