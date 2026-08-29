# Spike: glibc-static POC

Optional libc flavor: embed glibc via `*-linux-gnu` + `crt-static`. **Same bar** as musl: no interpreter, no `DT_NEEDED`.

This is **not** “use the host’s libc.” LGPL relink obligations apply; dist notes must not pretend this is MIT-like musl.

**Do not “fix” `dlopen`:** if something needs gconv/NSS plugins, stop calling it or keep that engine musl-only / T2. Do not ship `.so`.

**Suggested sequence (Linux CI, not this Darwin host)**

1. Tiny `hello` bin, built on a **different** distro than CI, `check-static` clean.
2. Same with Tree-sitter linked in.
3. clangd archive graph (see [../clangd.md](../clangd.md)).
4. csharp-ls glibc-static: pass or fail closed.

**M0 result:** directory reserved. Not folded into `xtask dist` (no clean ELF yet). `xtask musl` remains the default path.
