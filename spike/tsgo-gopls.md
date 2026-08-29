# Spike: tsgo and gopls (`CGO_ENABLED=0`)

- **tsgo:** TypeScript T3. Never Node tsserver.
- **gopls:** Go T3. Keep `CGO_ENABLED=0` (pure Go, **no libc**). Do not CGO-link glibc-static.

Both are skipped in the allocator matrix (their own heaps).

**Fail closed:** CGO or Node runtime → do not ship.

**M0 result:** notes only. Packs land in M4.

**M4:** tsgo and gopls adapters land. Darwin `xtask dist --pack full` writes stubs only. FakeEngineAdapter covers TS go-to-type / generics and Go T3. Real `CGO_ENABLED=0` musl ELFs are Linux CI / Docker. Slim dist excludes both.
