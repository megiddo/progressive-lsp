# Spike: tsgo and gopls (`CGO_ENABLED=0`)

- **tsgo:** TypeScript T3. Never Node tsserver.
- **gopls:** Go T3. Keep `CGO_ENABLED=0` (pure Go, **no libc**). Do not CGO-link glibc-static.

Both are skipped in the allocator matrix (their own heaps).

**Fail closed:** CGO or Node runtime → do not ship.

**M0 result:** notes only. Packs land in M4.
