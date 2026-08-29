# Stub engine-pack job. Content-addressed by upstream git SHA (later).
# PR CI does not compile LLVM/clangd/tsgo from scratch.
# Real pack builds land with M3/M4; this file reserves the hermetic layout.

FROM alpine:3.20

ARG PACK=stub
ARG UPSTREAM_SHA=unknown

RUN echo "engine-pack stub pack=${PACK} sha=${UPSTREAM_SHA}" > /pack-id.txt

# Placeholder: later stages fetch a cached artifact keyed by UPSTREAM_SHA.
# Do not compile LLVM here.
CMD ["cat", "/pack-id.txt"]
