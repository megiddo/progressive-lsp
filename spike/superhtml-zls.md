# Spike: superhtml and zls

- **superhtml:** HTML T3 (Zig). Fallback T1 if the pack is absent.
- **zls:** Zig T3. Tracks Zig tightly; matrix lag expected. No project `zig` on PATH → T2.

Allocator matrix skips both (their heaps).

**Fail closed:** dynamic Zig std / interpreter / `DT_NEEDED` → do not ship.

**M0 result:** notes only.
