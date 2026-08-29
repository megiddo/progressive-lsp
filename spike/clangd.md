# Spike: clangd static archive graph

Closing the static graph (`zlib`, `libxml2`, `ncurses`/`tinfo`, `libffi`, `zstd`, `libedit`, …) is expected whack-a-mole.

**Fail closed:** first unclosable `.so`-only dependency is a documented miss. Do **not** ship a dynamic clangd.

Size: record stripped ELF size on CI when a candidate exists. A huge but static binary is acceptable; a small dynamic one is not.

**M0 result:** notes only. Engine-pack Dockerfile is a stub (no LLVM compile on PR CI).

**M4.1:** clangd `PackAdapter` + `compile_commands.json` WorkspaceSource. C/C++ T1 without pack; T3 via Fake clangd (F12 + find-implementation). Slim dist excludes clangd. Fail closed: stub bytes never exec; do not ship `DT_NEEDED`. One-shot cmake argv only if `CMakeLists.txt` already exists.
