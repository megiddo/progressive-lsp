# Hermetic full engine pack (Go): gopls, tsgo.
# CGO_ENABLED=0. Pure Go, no libc. Do not CGO-link glibc.
# Fetch at the pinned git SHA inside this image — not a Mac host product step.
# Parameterized by PACK + UPSTREAM_SHA + GOOS / GOARCH / GO_PACKAGE.
#
#   docker build --platform linux/arm64 \
#     --build-arg PACK=gopls --build-arg BINARY=gopls \
#     --build-arg UPSTREAM_REPO=https://github.com/golang/tools.git \
#     --build-arg UPSTREAM_SHA=<40-hex> --build-arg SOURCE_SUBDIR=gopls \
#     --build-arg GO_PACKAGE=. --build-arg GO_VERSION=1.24 \
#     --build-arg GOOS=linux --build-arg GOARCH=arm64 \
#     -f docker/engine-pack-go.Dockerfile \
#     --output type=local,dest=target/musl/aarch64-unknown-linux-musl/engines/gopls .

ARG GO_VERSION=1.26
FROM golang:${GO_VERSION}-bookworm AS build

ARG PACK
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG BINARY
ARG SOURCE_SUBDIR=
ARG GO_PACKAGE=.
ARG GOOS=linux
ARG GOARCH=amd64

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${BINARY}" && test -n "${GOOS}" && test -n "${GOARCH}" \
    && test -n "${GO_PACKAGE}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown"

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fetch
# Go binary does not need the tsgo TypeScript submodule (conformance fixtures).
# qemu/amd64 clone of that tree flakes; do not fail the pack job on it.
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

ENV CGO_ENABLED=0
# Pipe fallback: Docker Desktop DNS to proxy.golang.org can time out.
ENV GOPROXY=https://proxy.golang.org|direct

WORKDIR /fetch/src
RUN set -eux; \
    src="/fetch/src"; \
    if [ -n "${SOURCE_SUBDIR}" ]; then src="${src}/${SOURCE_SUBDIR}"; fi; \
    cd "${src}"; \
    if [ -f go.mod ] && grep -q 'module golang.org/x/tools/gopls' go.mod; then \
        go mod edit -replace golang.org/x/tools=../; \
    fi; \
    CGO_ENABLED=0 GOOS="${GOOS}" GOARCH="${GOARCH}" \
        go build -trimpath -ldflags="-s -w" -o "/tmp/${BINARY}" ${GO_PACKAGE}; \
    mkdir -p /out; \
    cp "/tmp/${BINARY}" "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
