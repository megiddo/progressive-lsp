# Hermetic Rust musl builder for both shipped triples.
#   docker build --platform linux/amd64 --build-arg RUST_TARGET=x86_64-unknown-linux-musl -f docker/rust-musl.Dockerfile .
#   docker build --platform linux/arm64 --build-arg RUST_TARGET=aarch64-unknown-linux-musl -f docker/rust-musl.Dockerfile .
#
# PR CI must not compile LLVM. This job builds only the core binary.

FROM rust:1.87-alpine

ARG RUST_TARGET=x86_64-unknown-linux-musl
ENV RUST_TARGET=${RUST_TARGET}

RUN apk add --no-cache musl-dev && \
    rustup target add "${RUST_TARGET}"

WORKDIR /src
COPY . .

RUN cargo build --release --locked --target "${RUST_TARGET}" --bin progressive-lsp
