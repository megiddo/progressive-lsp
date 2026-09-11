# Runtime image (x86_64): COPY prebuilt HOST-2 core + HOST-3 slim ELFs only.
# aarch64 uses docker/runtime-aarch64.Dockerfile (glibc base for javacs).
# No rustc, cargo, clang, LLVM, zig, or go. Context is a staging dir
# of already-extracted files — not the git tree.
#
#   docker build --platform linux/arm64 \
#     -f docker/runtime.Dockerfile \
#     -t progressive-lsp-runtime:local \
#     target/runtime-image/aarch64-unknown-linux-musl
#
# Fail-closed for a missing core ELF happens in `xtask runtime-image`
# before this file is invoked. superhtml x86_64 may be omitted (HOST-3 miss).

FROM scratch

COPY prefix /opt/plsp
COPY tmp /tmp

ENTRYPOINT ["/opt/plsp/bin/progressive-lsp"]
CMD ["serve", "--prefix", "/opt/plsp"]
