# Hermetic slim engine pack (Graal native-image): java (javacs).
# Build-time JDK/Graal only. No JVM / JDT-LS / libjvm at runtime.
# Never ship a JAR, jlink image, or host `java` as the dest.
#
# Per-triple image + flags (xtask passes GRAAL_TAG / NATIVE_IMAGE_FLAGS):
#   x86_64:  GRAAL_TAG=25.0.0-muslib-ol9 + --static --libc=musl (fully static)
#   aarch64: GRAAL_TAG=25.0.0-ol9 + -H:+StaticExecutableWithDynamicLibC
#            (mostly static; host libc allowed). muslib is x64-only; 25.0.0-ol9
#            exists on arm64. aarch64 dest is required — not a Miss.
#
# Tag order is `$version[-muslib][-$platform]`. `25.0.0-ol9-muslib` does not exist.
#
#   docker build --platform linux/amd64 \
#     --build-arg PACK=java --build-arg BINARY=javacs \
#     --build-arg GRAAL_TAG=25.0.0-muslib-ol9 \
#     --build-arg NATIVE_IMAGE_FLAGS="--static --libc=musl" \
#     --build-arg UPSTREAM_REPO=https://github.com/georgewfraser/java-language-server.git \
#     --build-arg UPSTREAM_SHA=<40-hex> \
#     -f docker/engine-pack-graal.Dockerfile \
#     --output type=local,dest=target/musl/x86_64-unknown-linux-musl/engines/java .

ARG GRAAL_TAG=25.0.0-muslib-ol9
FROM ghcr.io/graalvm/native-image-community:${GRAAL_TAG} AS build

ARG PACK
ARG UPSTREAM_REPO
ARG UPSTREAM_SHA
ARG BINARY
ARG SOURCE_SUBDIR=
ARG NATIVE_IMAGE_FLAGS="--static --libc=musl"

RUN test -n "${PACK}" && test -n "${UPSTREAM_REPO}" && test -n "${UPSTREAM_SHA}" \
    && test -n "${BINARY}" \
    && test "${UPSTREAM_SHA}" != "latest" && test "${UPSTREAM_SHA}" != "unknown" \
    && test "${BINARY}" != "java"

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
# javac-based LS (org.javacs). Flags come from NATIVE_IMAGE_FLAGS:
# x86_64 `--static --libc=musl`; aarch64 `-H:+StaticExecutableWithDynamicLibC`.
# Fail closed if the result is a JAR, jlink image, host `java`, or libjvm.
RUN set -eux; \
    src="/fetch/src"; \
    if [ -n "${SOURCE_SUBDIR}" ]; then src="${src}/${SOURCE_SUBDIR}"; fi; \
    cd "${src}"; \
    mvn -q -DskipTests package; \
    test -f dist/classpath/java-language-server.jar; \
    CP="$(find dist/classpath -name '*.jar' | paste -sd: -)"; \
    native-image ${NATIVE_IMAGE_FLAGS} \
        --no-fallback \
        -H:+ReportExceptionStackTraces \
        -cp "${CP}" \
        -o "/tmp/${BINARY}" \
        org.javacs.Main; \
    test -f "/tmp/${BINARY}"; \
    test "${BINARY}" != "java"; \
    od -An -tx1 -N4 "/tmp/${BINARY}" | tr -d ' \n' | grep -qx '7f454c46' \
      || { echo "refusing non-ELF dest (JAR or host java)" >&2; exit 1; }; \
    if [ -x "${JAVA_HOME}/bin/java" ] && cmp -s "/tmp/${BINARY}" "${JAVA_HOME}/bin/java"; then \
      echo "refusing host java as the runtime binary" >&2; \
      exit 1; \
    fi; \
    if command -v readelf >/dev/null 2>&1 \
       && readelf -d "/tmp/${BINARY}" 2>/dev/null | grep -qi libjvm; then \
      echo "refusing libjvm DT_NEEDED" >&2; \
      exit 1; \
    fi; \
    mkdir -p /out; \
    cp "/tmp/${BINARY}" "/out/${BINARY}"

FROM scratch
ARG BINARY
COPY --from=build /out/${BINARY} /${BINARY}
