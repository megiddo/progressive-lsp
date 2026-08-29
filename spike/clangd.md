# Spike: clangd static archive graph

Closing the static graph (`zlib`, `libxml2`, `ncurses`/`tinfo`, `libffi`, `zstd`, `libedit`, …) is expected whack-a-mole.

**Fail closed:** first unclosable `.so`-only dependency is a documented miss. Do **not** ship a dynamic clangd.

Size: record stripped ELF size on CI when a candidate exists. A huge but static binary is acceptable; a small dynamic one is not.

**M0 result:** notes only. Engine-pack Dockerfile is a stub (no LLVM compile on PR CI).
