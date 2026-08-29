# Spikes (M0 notes)

Fail closed: a spike that cannot produce a fully static ELF (`check-static`: no interpreter, no `DT_NEEDED`) does **not** ship. Document the ceiling in [docs/language-matrix.md](../docs/language-matrix.md) instead.

These are notes, not blockers for M0 crate sign-off. Hygiene applies if a spike is folded into xtask.

| Spike | Status | Ship rule |
|---|---|---|
| [csharp-ls](csharp-ls.md) | Notes only | AOT + musl or C# T2 ceiling |
| [glibc-static](glibc-static/README.md) | Notes only | Fold into xtask only if `check-static` clean |
| [clangd](clangd.md) | Notes only | Close archive graph or document first unclosable `.so` |
| [ty / rust-analyzer](ty-ra.md) | Notes only | musl static; sysroot is a project artifact |
| [tsgo / gopls](tsgo-gopls.md) | Notes only | `CGO_ENABLED=0`; no Node |
| [superhtml / zls](superhtml-zls.md) | Notes only | Zig static; degrade without project `zig` |
| [PHP T3](php-t3.md) | Notes only | PHPantom preferred; static phpactor only if fully static |

Do not vendor huge engines in this tree.
