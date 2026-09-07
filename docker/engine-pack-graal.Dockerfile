# Hermetic slim engine pack (Graal native-image): java (javacs).
# Build-time JDK/Graal only. No JVM / JDT-LS / libjvm at runtime.
# Output must be a static musl ELF (`xtask check-static`). Never ship a JAR.
# Do not native-image Eclipse JDT.LS. Do not copy a jlink runtime.
#
# Tag order is `$version[-muslib][-$platform]` (e.g. `25.0.0-muslib-ol9`).
# `25.0.0-ol9-muslib` does not exist. muslib images are **x64 only**.
# aarch64: `xtask pack` returns JAVA-T3.2 Miss (no docker). Do not pretend
# muslib-ol9 exists for linux/arm64. See spike/java-t3.md.
#
#   docker build --platform linux/amd64 \
#     --build-arg PACK=java --build-arg BINARY=javacs \
#     --build-arg UPSTREAM_REPO=https://github.com/georgewfraser/java-language-server.git \
#     --build-arg UPSTREAM_SHA=<40-hex> \
#     -f docker/engine-pack-graal.Dockerfile \
#     --output type=local,dest=target/musl/x86_64-unknown-linux-musl/engines/java .

# Pinned. Exists on linux/amd64; muslib is not published for arm64.
ARG GRAAL_TAG=25.0.0-muslib-ol9
FROM ghcr.io/graalvm/native-image-community:${GRAAL_TAG} AS build

ARG PACK
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG BINARY
ARG SOURCE_SUBDIR=
ARG TARGETARCH

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${BINARY}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown"

# Fail closed on arm64 if docker is invoked anyway (pack.rs should Miss first).
RUN if [ "${TARGETARCH}" = "arm64" ]; then \
      echo "JAVA-T3.2 miss: Graal muslib / native-image --static --libc=musl is Linux x64 only. \
See spike/java-t3.md. Refusing a non-static extract." >&2; \
      exit 1; \
    fi

RUN microdnf install -y git maven \
    && microdnf clean all

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

WORKDIR /fetch/src
# javac-based LS (org.javacs). native-image --static --libc=musl; fail closed if
# the result is a JAR, jlink image, or DT_NEEDED / libjvm.
RUN set -eux; \
    src="/fetch/src"; \
    if [ -n "${SOURCE_SUBDIR}" ]; then src="${src}/${SOURCE_SUBDIR}"; fi; \
    cd "${src}"; \
    mvn -q -DskipTests package; \
    test -f dist/classpath/java-language-server.jar; \
    CP="$(find dist/classpath -name '*.jar' | paste -sd: -)"; \
    native-image --static --libc=musl \
        --no-fallback \
        -H:+ReportExceptionStackTraces \
        -cp "${CP}" \
        -o "/tmp/${BINARY}" \
        org.javacs.Main; \
    test -f "/tmp/${BINARY}"; \
    mkdir -p /out; \
    cp "/tmp/${BINARY}" "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
