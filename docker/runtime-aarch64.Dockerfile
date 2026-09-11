# Runtime image (aarch64): COPY prebuilt HOST-2 core + HOST-3 slim ELFs only.
# glibc userspace for javacs (libc exception). x86_64 stays scratch.
# No rustc, cargo, clang, LLVM, zig, or go. Context is staging only.
#
#   docker build --platform linux/arm64 \
#     -f docker/runtime-aarch64.Dockerfile \
#     -t progressive-lsp-runtime:local \
#     target/runtime-image/aarch64-unknown-linux-musl
#
# Base: Rocky Linux 9 minimal (glibc). Pin by digest when CI publishes one.

FROM rockylinux:9-minimal

COPY prefix /opt/plsp
COPY tmp /tmp

ENTRYPOINT ["/opt/plsp/bin/progressive-lsp"]
CMD ["serve", "--prefix", "/opt/plsp"]
