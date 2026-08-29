# Spike: superhtml and zls

- **superhtml:** HTML T3 (Zig). Fallback T1 if the pack is absent.
- **zls:** Zig T3. Tracks Zig tightly; matrix lag expected. No project `zig` on PATH → T2.

Allocator matrix skips both (their heaps).

**Fail closed:** dynamic Zig std / interpreter / `DT_NEEDED` → do not ship.

**M0 result:** notes only.

**M4:** superhtml and zls adapters land. HTML T3 when pack ready, else T1. Zig T3 when pack + `build.zig`, else T2/T1. Darwin stubs only; biome musl-clean unknown on this host (CSS adapter + T1 fallback). Slim dist excludes zls; superhtml is in slim.
