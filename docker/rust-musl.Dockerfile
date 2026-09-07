# Hermetic Rust musl builder for both shipped triples (core ELF only).
#   docker build --platform linux/amd64 --build-arg RUST_TARGET=x86_64-unknown-linux-musl \
#     -f docker/rust-musl.Dockerfile --output type=local,dest=target/musl/x86_64-unknown-linux-musl .
#   docker build --platform linux/arm64 --build-arg RUST_TARGET=aarch64-unknown-linux-musl \
#     -f docker/rust-musl.Dockerfile --output type=local,dest=target/musl/aarch64-unknown-linux-musl .
#
# Extract dest is target/musl/<triple>/progressive-lsp (HOST-2).
# PR CI must not compile LLVM. This job builds only the core binary.
# Slim packs are docker/engine-pack.Dockerfile (HOST-3), not this file.

FROM rust:1.87-alpine AS build

ARG RUST_TARGET=x86_64-unknown-linux-musl
ENV RUST_TARGET=${RUST_TARGET}

RUN apk add --no-cache musl-dev && \
    rustup target add "${RUST_TARGET}"

WORKDIR /src
COPY . .

RUN cargo build --release --locked --target "${RUST_TARGET}" --bin progressive-lsp && \
    mkdir -p /out && \
    cp "/src/target/${RUST_TARGET}/release/progressive-lsp" /out/progressive-lsp

FROM scratch
COPY --from=build /out/progressive-lsp /progressive-lsp
