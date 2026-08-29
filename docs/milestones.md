# Milestones

Product exits. Work order and Depends-on: [implementation-plan.md](implementation-plan.md). Branches: [branching.md](branching.md). Durations are sequential engineering time, not calendar promises.

**Do not start M0 crates until Docs-0 is signed off.**

## Docs-0 — Repository documentation

**Status: SIGNED OFF** on branch `docs-0`. Do not start M0 crates until this section is signed off (it is).

**Scope:** this `docs/` tree. No application crates.

**Exit**

- [x] [docs/README.md](README.md) links every file in this set.
- [x] vision, requirements, architecture, detailed-design, design-patterns, testing, milestones, implementation-plan, branching exist and **agree** with locked decisions (static ELFs, no Node/JVM/CPython, Java T3 out, protobuf not `$/`, allocator matrix, `.progressivelsp`, Rhai catalog).
- [x] [initial-progressive-lsp-design.md](initial-progressive-lsp-design.md) marked archive.

**Sign-off checklist (D0)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — **N/A** (no crates)
- [x] 95% llvm-cov on crates that exist — **N/A** (none)
- [x] 80% mutants on listed crates that exist — **N/A** (none)
- [x] No `sleep` in tests — **N/A** (no tests)
- [x] `check-static` if ELF changed — **N/A** (no ELFs)
- [x] [design-patterns.md](design-patterns.md) names every type in [detailed-design.md](detailed-design.md)
- [x] Docs in this tree updated if a locked decision was refined

## M0 — Skeleton, `.progressivelsp`, control schema, build (~2 weeks)

- Workspace, `PluginRegistry` + empty `LanguageFactory` slots, vanilla LSP `initialize` / `shutdown`.
- Default prefix `$HOME/.progressivelsp`; git-exclude helpers; `config.toml` schema stub.
- `proto/progressive/v1`: Config, WatchBatch, FilesSince, IndexStatus (empty answers OK). Codec round-trip. `experimental.progressiveLsp` advertised (socket may be absent).
- `progressive-lsp-install`: `manifest.json`, `LocalFs`, hash helpers. No network.
- `xtask musl` both triples; `check-static`; coverage on existing libs.
- Hermetic Dockerfiles for Rust-musl and a stub engine-pack job.
- `language-matrix.md` pins as living table; control-protocol and plugin-sdk as implemented stubs matching these docs.
- Spike **notes** (not blockers for M0 exit unless marked): csharp-ls AOT+musl; glibc-static POC (`spike/glibc-static`); clangd size; ty/RA; tsgo/gopls `CGO_ENABLED=0`; superhtml/zls; PHPantom vs static phpactor; first `bench-alloc` rows or mimalloc placeholders.

**Exit:** static binaries both arches; placeholder or first `allocator-matrix.toml` rows; LSP initialize round-trip; control codec round-trip; `check-static` clean; install layout under `.progressivelsp`; worktree cache git-excluded in a **fixture** repo.

## M1 — Watch, FilesSince, incremental T1, Java baseline (~3 weeks)

- Watch coalescer + FakeWatcher/FakeClock (10k events → one `WatchBatch`).
- `FilesSince` after overflow or generation gap; `truncated` tested.
- Incremental Tree-sitter on `didChange`; dirty-set priority indexer.
- `WatchFilter` (identity enough).
- Java T1: definition, references, document/workspace symbol, hover signature (name + arity).
- Semantic tokens from Tree-sitter.
- Directory + Maven/Gradle/Eclipse adapters for a multi-package Java fixture.

**Exit:** F12 across packages on T1; open-buffer reparse in the ~10 ms budget; unopened file change reindexes via **server-side watch** without a progressive client; FilesSince + WatchBatch when control is on. **No host JDK.**

## M2 — Progressive ingest, T2, web + PHP, host scripts (~4 weeks)

- Package-stream ingest, `workDoneProgress`, `TierReady` when progressive connected, `data.tier` on locations.
- Java T2 heuristics; stack-graphs Java TSG evaluated behind the same Strategy; keep only if it wins.
- PHP T1 + Composer PSR-4; PHP T2 (`use` + hierarchy) as far as fixtures allow.
- HTML/CSS/JS T1: symbols, selector/`id` find-usages as AST+string, highlighting.
- Go T1 + `go.mod` adapter. Zig T1 + `build.zig` discovery.
- Rhai: `on_bootstrap`, `on_workspace_discover`, `on_pre_index` / `on_post_index`, `on_watch`. Sandbox tests.

**Exit:** Java ~95% of a **heuristic** fixture set (not JDT 99%). PHP F12 across namespaces via Composer. HTML/CSS/JS highlight + symbols. Go/Zig highlight + document symbols + intra-module F12 at T1/T2 without gopls/zls. A fixture script can deny a path and abort initialize. Ingest never blocks `didChange` highlighting.

## M3 — Engine supervisor + Python/Rust T3 (~4 weeks)

- `EngineAdapter` plugins: spawn, stdio proxy, crash/backoff, capability merge, forward changes.
- Pack discovery under `$PREFIX/engines/` (not a zeds-dead path).
- **ty** and **rust-analyzer** artifacts via `xtask dist --pack python,rust`.
- `on_engine_spawn` + `on_tier_ready`.
- Handoff: T3 when ready for that package; else T2 if that language has a T2 Strategy, else T1.

**Exit:** Python go-to-def / refs / hover types / implementation via ty; Rust the same via RA when sysroot exists; without packs, those languages still navigate at T1 (Python may use optional TSG T2; Rust has no dedicated T2). Core still static and usable alone. A script can skip ty; Python stays T1 (optional TSG T2).

## M4 — Remaining T3 packs (~5 weeks)

- clangd + `compile_commands.json`; C/C++ T3.
- csharp-ls AOT **or** documented C# T2 ceiling if the spike failed.
- oxc T2 + **tsgo** T3. Never Node tsserver.
- PHP T3 from M0 spike winner (PHPantom preferred; static phpactor only if fully static).
- HTML **superhtml**; CSS biome if musl-clean.
- **gopls**, **zls**. Degrade to T2 if project `go`/`zig` missing.
- Slim dist without C/C++/tsgo/gopls/zls default for Java-only workspaces.

**Exit:** C/C++ F12 + find-implementation on compile_commands fixture. TS go-to-type / generics via tsgo without Node. PHP T3 on Composer when pack installed; else T2. Go/Zig T3 on fixtures. C# T3 or explicit matrix note. HTML via superhtml or T1 fallback.

## M5 — Cache, multi-version, bursts (~3 weeks)

- Content-addressed index cache; cold start skips unchanged files.
- LATEST, LATEST-1, LATEST-2 fixtures per language; mixed-version workspace.
- Watch overflow → FilesSince catch-up; 10k-file external-edit burst within published budget.
- Grammar/engine lag: newer syntax must not panic.
- Performance gates: core RSS; T1/T2 p99; T3 not charged to core.

**Exit:** matrix CI green; cache hit test; burst test; [host-deps.md](host-deps.md) complete vs implementation.

## M6 — Deploy, contracts as standard (~3 weeks)

- `xtask dist` per-triple tarballs + `manifest.json` + SHA256; slim vs full.
- Install CLI: `install` / `serve` / `--control-socket` / `--mux`; `on_install_verify`; FakeTransport hash mismatch + atomic replace.
- Docs in this tree are the **published** standard (refresh if impl drifted).
- Conformance dashboard (pass % per language per tier).
- Versioning: core semver independent of engine SHAs; proto `progressive.v1`.

**Exit:** local `progressive-lsp install --packs python` produces a verified prefix; stock `initialize` with control off; progressive fixture uses FilesSince + WatchBatch over protobuf only. Fake ssh-like transport test for install.

## Post-v1 (not scheduled)

Java in-house types (still no JVM). Dual-run PHP T3 if the other spike wins. oxc_type_checker as TS T3. Native macOS/Windows hosts. WASM plugin ABI. HTTP/S3 transport in-tree. Buck2 if engine builds outgrow Docker cache. Watchman. `$/` JSON mirror of `progressive.v1` only if a real client cannot open a socket or mux.
