# Hermetic slim engine pack (Rust): ty, rust-analyzer, phpantom, biome.
# Parameterized by PACK + UPSTREAM_SHA + RUST_TARGET.
# Fetch at the pinned git SHA inside this image — not a Mac host product step.
# PR CI must not compile LLVM. Heavy packs (clangd/tsgo/gopls/zls) are HOST-7.
#
#   docker build --platform linux/arm64 \
#     --build-arg PACK=python --build-arg BINARY=ty --build-arg CARGO_BIN=ty \
#     --build-arg UPSTREAM_REPO=https://github.com/astral-sh/ty.git \
#     --build-arg UPSTREAM_SHA=<40-hex> --build-arg SOURCE_SUBDIR=ruff \
#     --build-arg RUST_TARGET=aarch64-unknown-linux-musl \
#     -f docker/engine-pack.Dockerfile \
#     --output type=local,dest=target/musl/aarch64-unknown-linux-musl/engines/python .

# Debian host rustc + musl *target* (alpine musl-host rustup 1.98+ is unpublished).
FROM rust:1.98.0-bookworm AS build

ARG PACK
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG RUST_TARGET=x86_64-unknown-linux-musl
ARG RUST_CHANNEL=1.98.0
ARG CARGO_BIN
ARG BINARY
ARG SOURCE_SUBDIR=

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${CARGO_BIN}" && test -n "${BINARY}" && test -n "${RUST_CHANNEL}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown"

RUN apt-get update \
    && apt-get install -y --no-install-recommends git musl-tools musl-dev make gcc pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && rustup toolchain install "${RUST_CHANNEL}" --profile minimal \
    && rustup default "${RUST_CHANNEL}" \
    && rustup target add "${RUST_TARGET}"

ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc

WORKDIR /fetch
RUN git clone "${UPSTREAM_REPO}" src \
    && cd src \
    && git checkout --detach "${UPSTREAM_SHA}" \
    && git submodule update --init --recursive

# SOURCE_SUBDIR is `ruff` for ty (the Rust workspace is the submodule). Empty otherwise.
WORKDIR /fetch/src
ENV CARGO_TERM_COLOR=never
# x86_64-unknown-linux-musl defaults to dynamic ld-musl (PT_INTERP). aarch64
# still came out static; qemu amd64 did not. Target-specific env + rustc
# link flags beat upstream .cargo/config rustflags that drop RUSTFLAGS.
ENV RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=yes -C link-arg=-static"
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=yes -C link-arg=-static"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=yes -C link-arg=-static"
# LTO is optional; Darwin Docker Desktop OOM-killed biome_cli with default LTO.
ENV CARGO_PROFILE_RELEASE_LTO=off
ENV CARGO_BUILD_JOBS=2
# Honor upstream rust-toolchain.toml when present (biome/ty); else RUST_CHANNEL.
# `cargo rustc` is not used: ty/ruff is a virtual manifest.
RUN set -eux; \
    src="/fetch/src"; \
    if [ -n "${SOURCE_SUBDIR}" ]; then src="${src}/${SOURCE_SUBDIR}"; fi; \
    cd "${src}"; \
    rustup show; \
    rustup target add "${RUST_TARGET}"; \
    cargo build --release --target "${RUST_TARGET}" --bin "${CARGO_BIN}"; \
    mkdir -p /out; \
    cp "target/${RUST_TARGET}/release/${CARGO_BIN}" "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
