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

**Status: SIGNED OFF** on branch `m0`. Do not start M1 until this section stays signed off.

- Workspace, `PluginRegistry` + empty `LanguageFactory` slots, vanilla LSP `initialize` / `shutdown`.
- Default prefix `$HOME/.progressivelsp`; git-exclude helpers; `config.toml` schema stub.
- `proto/progressive/v1`: Config, WatchBatch, FilesSince, IndexStatus (empty answers OK). Codec round-trip. `experimental.progressiveLsp` advertised (socket may be absent).
- `progressive-lsp-install`: `manifest.json`, `LocalFs`, hash helpers. No network.
- `xtask musl` both triples; `check-static`; coverage on existing libs.
- Hermetic Dockerfiles for Rust-musl and a stub engine-pack job.
- `language-matrix.md` pins as living table; control-protocol and plugin-sdk as implemented stubs matching these docs.
- Spike **notes** (not blockers for M0 exit unless marked): csharp-ls AOT+musl; glibc-static POC (`spike/glibc-static`); clangd size; ty/RA; tsgo/gopls `CGO_ENABLED=0`; superhtml/zls; PHPantom vs static phpactor; first `bench-alloc` rows or mimalloc placeholders.

**Exit:** static binaries both arches (toolchain + documented CI gap on Darwin — see notes); placeholder `allocator-matrix.toml` rows; LSP initialize round-trip; control codec round-trip; `check-static` clean on fixture ELFs; install layout under `.progressivelsp`; worktree cache git-excluded in a **fixture** repo.

**Sign-off checklist (M0)**

- [x] Exit criteria for this WP met (CI Linux must still produce both musl ELFs)
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`)
- [x] 80% mutants on listed crates that exist
- [x] No `sleep` in tests
- [x] `check-static` on fixture ELFs (static pass / DT_NEEDED fail / Mach-O refuse). Not run against a host Mach-O as a fake green.
- [x] [design-patterns.md](design-patterns.md) table updated for M0 types
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test` is the M0 gate on macOS.
- `xtask musl` is Docker-based for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. Producing both-arch ELFs requires Linux CI or a working multi-arch Docker; this host does not substitute a Mach-O.
- `xtask check-static` is unit-tested against in-tree fixture ELFs. A green result on those fixtures is not a claim that release musl binaries were built here.

## M1 — Watch, FilesSince, incremental T1, Java baseline (~3 weeks)

**Status: SIGNED OFF** on branch `m1`. Do not start M2 until this section stays signed off.

- Watch coalescer + FakeWatcher/FakeClock (10k events → one `WatchBatch`).
- `FilesSince` after overflow or generation gap; `truncated` tested.
- Incremental Tree-sitter on `didChange`; dirty-set priority indexer.
- `WatchFilter` (identity enough).
- Java T1: definition, references, document/workspace symbol, hover signature (name + arity).
- Semantic tokens from Tree-sitter.
- Directory + Maven/Gradle/Eclipse adapters for a multi-package Java fixture.

**Exit:** F12 across packages on T1; open-buffer reparse in the ~10 ms budget; unopened file change reindexes via **server-side watch** without a progressive client; FilesSince + WatchBatch when control is on. **No host JDK.**

**Sign-off checklist (M1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C) — **97.16% lines**
- [x] 80% mutants on listed crates that exist — watch 87.1%, index 97.9%, resolve 85.4%, workspace 87.6%, lang-java 86.6%, core+control+install 93.4%
- [x] No `sleep` in tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change)
- [x] [design-patterns.md](design-patterns.md) table updated for M1 types
- [x] Docs in this tree updated if a locked decision was refined

## M2 — Progressive ingest, T2, web + PHP, host scripts (~4 weeks)

**Status: SIGNED OFF** on branch `m2`. Do not start M3 until this section stays signed off. No `EngineSupervisor`, no ty/RA packs.

- Package-stream ingest, `workDoneProgress`, `TierReady` when progressive connected, `data.tier` on locations.
- Java T2 heuristics; stack-graphs Java TSG evaluated behind the same Strategy; **dropped** (archived upstream; heuristics win on the fixture). Strategy slot `StackGraphResolver` remains `NotReady`.
- PHP T1 + Composer PSR-4; PHP T2 (`use` + hierarchy) as far as fixtures allow.
- HTML/CSS/JS T1: symbols, selector/`id` find-usages as AST+string, highlighting. Split crates (`lang-html` / `lang-css` / `lang-javascript`); TypeScript T1 uses the JS grammar.
- Go T1 + `go.mod` adapter. Zig T1 + `build.zig` discovery.
- Rhai: `on_bootstrap`, `on_workspace_discover`, `on_pre_index` / `on_post_index`, `on_watch`. Sandbox tests.

**Exit:** Java ~95% of a **heuristic** fixture set (not JDT 99%). PHP F12 across namespaces via Composer. HTML/CSS/JS highlight + symbols. Go/Zig highlight + document symbols + intra-module F12 at T1/T2 without gopls/zls. A fixture script can deny a path and abort initialize. Ingest never blocks `didChange` highlighting.

**Sign-off checklist (M2)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C) — **96.49% lines**
- [x] 80% mutants on listed crates that exist — script 90.2%, css 100%, javascript 95.8%, html 96.8%, go 82.6%, zig 85.7%, php 98.1%, java 82.6%, watch 96.8%, index 81.5%, resolve 81.6%, workspace 96.9%, control 88.7%, core 94.7%, install 95.8%
- [x] No `sleep` in tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change)
- [x] [design-patterns.md](design-patterns.md) table updated for M2 types
- [x] Docs in this tree updated if a locked decision was refined

## M3 — Engine supervisor + Python/Rust T3 (~4 weeks)

**Status: SIGNED OFF** on branch `m3`. Do not start M4 until this section stays signed off. No clangd/tsgo/gopls/zls/csharp-ls/PHPantom packs.

- `EngineAdapter` plugins: spawn, stdio proxy, crash/backoff, capability merge, forward changes.
- Pack discovery under `$PREFIX/engines/` (not a zeds-dead path).
- **ty** and **rust-analyzer** artifacts via `xtask dist --pack python,rust`.
- `on_engine_spawn` + `on_tier_ready`.
- Handoff: T3 when ready for that package; else T2 if that language has a T2 Strategy, else T1.

**Exit:** Python go-to-def / refs / hover types / implementation via ty; Rust the same via RA when sysroot exists; without packs, those languages still navigate at T1 (Python may use optional TSG T2; Rust has no dedicated T2). Core still static and usable alone. A script can skip ty; Python stays T1 (optional TSG T2).

**Sign-off checklist (M3)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own) — **96.14% lines**
- [x] 80% mutants on listed crates that exist — engine 81.5%, script 80.6%, python 92.0%, rust 86.0%, workspace 98.7%, resolve 80.7%, core 94.2%; remaining listed libs unchanged from M2
- [x] No `sleep` in tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change; Darwin dist writes stubs only)
- [x] [design-patterns.md](design-patterns.md) table updated for M3 types
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test` is the M3 gate on macOS. Tests use `FakeEngineAdapter` / fixture stub bytes.
- `xtask dist --pack python,rust` writes `$PREFIX/engines/{python,rust}/` + `manifest.json` + SHA256. The files are **stubs**, not musl ELFs. Real ty / rust-analyzer static packs are Linux CI / Docker (same class as the M0 musl gap). Do not treat stub hashes as `check-static` greens.
- Engine wrappers: mutation-test supervisor crash/backoff/hash/discovery only — not clangd/ty/RA upstream source.

## M4 — Remaining T3 packs (~5 weeks)

**Status: SIGNED OFF** on branch `m4`. Do not start M5 until this section stays signed off. No disk IndexCache productization, no LATEST+2 matrix CI, no burst RSS gates.

- clangd + `compile_commands.json`; C/C++ T3 (Fake clangd on Darwin).
- C# **T1/T2 ceiling** — csharp-ls AOT produced no musl ELF (`spike/csharp-ls.md`).
- Heuristic JS/TS T2 + **tsgo** T3. Never Node tsserver. oxc_resolver/oxc_semantic not wired (heuristic import Strategy).
- PHP T3 winner: **PHPantom** (Rust). T2 when pack absent.
- HTML **superhtml** T3 or T1 fallback; CSS **biome** adapter + T1 fallback (musl-clean unknown on Darwin).
- **gopls**, **zls**. T3 when pack + project `go.mod`/`build.zig`; else T2/T1. No bundled SDKs.
- Slim dist default excludes clangd/tsgo/gopls/zls. Full includes them as Darwin stubs / CI packs.

**Exit:** C/C++ F12 + find-implementation on compile_commands fixture (Fake T3). TS go-to-type / generics via tsgo without Node. PHP T3 on Composer when pack installed; else T2. Go/Zig T3 on fixtures when pack+project; else degrade. C# T1/T2 ceiling documented. HTML via superhtml or T1 fallback.

**Sign-off checklist (M4)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own) — **96.44% lines**
- [x] 80% mutants on listed crates that exist (engine: supervisor/adapters discovery/backoff/hash only) — engine 85.7%, workspace 97.8%, install 92.0%, javascript 84.9%, php 100%, html 100%, css 100%, go 90.0%, zig 88.9%, c 89.5%, cpp 100%, csharp 88.9%; remaining listed libs unchanged from M3
- [x] No `sleep` in tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change; Darwin dist writes stubs only)
- [x] [design-patterns.md](design-patterns.md) table updated for M4 types
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test` is the M4 gate on macOS. Tests use `FakeEngineAdapter` / fixture stub bytes.
- `xtask dist` default is **slim** (python, rust, phpantom, superhtml, biome). `--pack full` adds clangd, tsgo, gopls, zls stubs. Stubs are not musl ELFs. Do not treat stub hashes as `check-static` greens.
- Engine wrappers: mutation-test supervisor crash/backoff/hash/discovery only — not clangd/tsgo/gopls/zls/PHPantom/superhtml/biome upstream source.

## M5 — Cache, multi-version, bursts (~3 weeks)

**Status: SIGNED OFF** on branch `m5`. Do not start M6 until this section stays signed off. No dist tarball productization, no conformance dashboard, no `on_install_verify` as an M6-only exit (the install crate already exists from M0).

- Content-addressed `IndexCache` under `$PREFIX/cache/` keyed `(grammar_ver, language_id, file_hash)`. Cold start skips unchanged files. Never written into the git worktree.
- LATEST, LATEST-1, LATEST-2 fixtures per v1 language; one mixed-version workspace. C# T1/T2 only. Java no T3.
- Watch overflow → FilesSince catch-up with `truncated`; 10k-file external-edit burst via FakeWatcher/FakeClock within the published budget.
- Grammar lag: newer-than-window syntax → ERROR nodes / unparsed note; server stays up (Java/PHP/JS/Python/Rust/C).
- Performance gates recorded: open-buffer reparse ~10 ms class; T1/T2 definition p99 < 50 ms after index; core RSS without engines; T3 not charged to core.

**Exit:** matrix fixture tests green (`cargo test` is the Darwin stand-in; Linux CI must run the same fixtures); cache hit test (disk, cold start); burst test; [host-deps.md](host-deps.md) complete vs implementation.

**Sign-off checklist (M5)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own) — **96.50% lines**
- [x] 80% mutants on listed crates that exist (especially index, watch, core) — index 82.7%, watch 98.0%, core 88.0%; remaining listed libs unchanged from M4
- [x] No `sleep` in tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change)
- [x] [design-patterns.md](design-patterns.md) table updated for M5 types
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test` is the M5 gate on macOS. Matrix fixtures live under `fixtures/matrix/` and `fixtures/lag/`. Linux CI must run the same fixtures on the matching arch.
- RSS / p99 / burst numbers in [testing.md](testing.md) from this host are **Darwin samples**. Allocator-matrix winners are recorded only from a matching CI arch job. Do not treat laptop numbers as musl ELF greens.
- `xtask bench-perf` reprints the host samples. `xtask bench-alloc` still only reads `xtask/allocator-matrix.toml`.

## M6 — Deploy, contracts as standard (~3 weeks)

**Status: SIGNED OFF** on branch `m6` (merged to `main`). v1 product exits are complete. Post-dev work is **PD0–PD4**, stacked on `main` — not M7.

- `xtask dist` per-triple tarballs + `manifest.json` + SHA256; slim vs full.
- Install CLI: `install` / `serve` / `--control-socket` / `--control-fd` / `--mux`; `on_install_verify`; FakeRemoteTransport hash mismatch + atomic replace.
- Docs in this tree are the **published** standard (refreshed vs impl).
- Conformance dashboard (pass % per language per tier): [conformance.md](conformance.md).
- Versioning: workspace/core **0.1.0**; engine SHAs live in pack `manifest.json`; proto stays `progressive.v1`.

**Exit:** local `progressive-lsp install --packs python` produces a verified prefix; stock `initialize` with control off; progressive fixture uses FilesSince + WatchBatch over protobuf only. Fake ssh-like transport test for install.

**Darwin vs CI:** this host’s `xtask dist` tarballs contain **stubs**, not musl ELFs. Do not claim `check-static` green on them. The real dist is Linux CI per-triple musl (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`).

## PD0 — Ingest post-dev docs

**Status: SIGNED OFF** on branch `pd0`. Design + user docs in-tree. Do not start PD1 crates until this section is signed off (it is).

**Scope:** copy user README/API, integration designs, T2 spike notes. No integration harness yet.

**Exit**

- [x] [docs/user/README.md](user/README.md) and [docs/user/progressive-v1-api.md](user/progressive-v1-api.md) in-tree
- [x] [integration/](../integration/README.md) IT-1/IT-2/IT-3 designs
- [x] [docs/spikes/t2-strategy-bakeoff.md](spikes/t2-strategy-bakeoff.md)
- [x] branching / implementation-plan list PD1–PD4

**Sign-off checklist (PD0)**

- [x] Exit criteria met
- [x] Tests / llvm-cov / mutants / `sleep` / `check-static` — **N/A** (docs only)
- [x] Docs in this tree updated

## PD1 — IT-1 deploy and config

**Status: SIGNED OFF** on branch `pd1`. Do not start PD2 until this section stays signed off.

Headless install/`serve`/`initialize` on Arch, Rocky/UBI, Debian, Ubuntu containers using a **prebuilt** static core (Linux CI). No Node/JVM/CPython in the image. Prefix, overlay config, git exclude. Spec: [integration/01-deploy-config.md](../integration/01-deploy-config.md).

**Exit:** IT-1.1–1.7 pass on CI Linux for musl core (no engine packs required). Darwin: do not fake musl greens; skip or document Docker-unavailable as a CI gap.

**Sign-off checklist (PD1)**

- [x] Exit criteria for this WP met (harness + compose + IT-1.1–1.7 cases; Linux CI is the distro gate)
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own, `integration/`) — **96.43% lines**
- [x] 80% mutants on listed crates that exist — **N/A** (no listed crate source change; composition root + integration harness only)
- [x] No `sleep` in crate unit tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change). Do not run it on a Darwin Mach-O.
- [x] [design-patterns.md](design-patterns.md) table updated for `ServeHost` / `LspStdioDriver`
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the PD1 unit gate on macOS.
- `integration/harness/run-it1.sh auto` on this host runs **host_smoke** only (Mach-O `serve` handshake, prefix, overlay, git exclude, help). That is **not** IT-1.1.
- Real IT-1.1–1.6 need a prebuilt musl ELF bind-mounted into Arch / Rocky / Debian / Ubuntu (`integration/compose.yaml`). This Darwin host has no Docker daemon and no musl ELF — same class as the M0 musl gap. Linux CI is the distro gate.
- `xtask check-static` on a Mach-O or dist stub is a refuse, not a green.

## PD2 — IT-2 vanilla LSP backends

**Status: SIGNED OFF** on branch `pd2`. Do not start PD3 until this section stays signed off.

Each language on a **pinned SHA** real corpus. Stock stdio LSP only. Spec: [integration/02-lsp-backends.md](../integration/02-lsp-backends.md).

**Exit:** report rows per language; T3 rows `skip_pack_missing` when packs are stubs. No `$/` FilesSince.

**Sign-off checklist (PD2)**

- [x] Exit criteria for this WP met (corpora pins + goldens + `plsp-it1 backend`; T3 stub rows are `skip_pack_missing`)
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own, `integration/`) — **96.55% lines**
- [x] 80% mutants on listed crates that exist — **N/A** (composition root + integration harness only; no listed crate source change)
- [x] No `sleep` in crate unit tests (IT-2 waits on `workDoneProgress` with a deadline)
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change). Do not run it on a Darwin Mach-O.
- [x] [design-patterns.md](design-patterns.md) table updated for `ServeDiskWatch` / `CorpusPin` / `ExpectedGolden` / `It2BackendDriver` / `It2ReportRow`
- [x] Docs in this tree updated if a locked decision was refined

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the PD2 unit gate on macOS.
- `integration/harness/run-it2.sh auto` fetches URL+SHA corpora (no submodule mirrors) and runs stock stdio on the native Mach-O. In-tree fixtures + `csharp-mini` are supplements, not the only Java/C# proof.
- T3 rows (`ty`, `rust-analyzer`, `clangd`, `tsgo`, `phpantom`, `biome`, `superhtml`, `gopls`, `zls`) are `skip_pack_missing` when the prefix holds Darwin stubs. That is **not** a typed hover green and must not be reported as clangd/ty T3 pass.
- C# is `expected_ceiling` T1/T2. Java has no T3. `$/` / `workspace/filesSince` must be method-not-found.
- Linux CI with real musl packs is the T3 gate — same class as the M0 musl gap.

## PD3 — IT-3 extended protocol

**Status: SIGNED OFF** on branch `pd3`. Do not start PD4 until this section stays signed off.

Java / Python+ty / TypeScript+tsgo progressive client. Envelope + FilesSince / WatchBatch / TierReady / InstallPacks. Spec: [integration/03-extended-protocol.md](../integration/03-extended-protocol.md), API: [user/progressive-v1-api.md](user/progressive-v1-api.md).

**Exit:** IT-3.1–3.7 as specified; mux `pending_mux` if unimplemented.

**Sign-off checklist (PD3)**

- [x] Exit criteria for this WP met (Envelope dispatch + IT-3.1–3.7 on P-java / P-py / P-ts; mux is `pending_mux`)
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own, `integration/`) — **95.97% lines**
- [x] 80% mutants on listed crates that exist — control **83 caught / 93 scored (89.2%)**, 1 unviable; core **119 caught / 133 scored (89.5%)**, 10 unviable
- [x] No `sleep` in crate unit tests (IT-3 may deadline-poll the control socket)
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change). Do not run it on a Darwin Mach-O.
- [x] [design-patterns.md](design-patterns.md) table updated for `Envelope` / `ControlPlane` / `dispatch_envelope` / control-socket Adapter / `It3ProgressiveDriver` / `It3ReportRow`
- [x] Docs in this tree updated if a locked decision was refined (`InstallPacks` requires restart `serve` to attach an engine)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the PD3 unit gate on macOS.
- `integration/harness/run-it3.sh auto` runs P-java / P-py / P-ts against native Mach-O with `--control-socket` and Envelope frames. Default `serve` without `--control-socket` stays stock (IT-2).
- T3 types rows (`ty`, `tsgo`) are `skip_pack_missing` when the prefix holds Darwin stubs. That is **not** a typed hover green.
- `--mux` is `pending_mux` — do not silently retest the socket.
- Linux CI with real musl packs is the T3 gate — same class as the M0 musl gap.

## PD4 — T2 Strategy bake-off

**Status: SIGNED OFF** on branch `pd4`. Post-dev stack (PD0–PD4) is complete. There is no PD5 in the plan.

Plugin seam: T2 Strategy selectable per language; **default remains heuristics**. Pin stack-graphs by git SHA; measure vs held-out corpus. Spec: [spikes/t2-strategy-bakeoff.md](spikes/t2-strategy-bakeoff.md). Results: [spikes/t2-bakeoff-results.md](spikes/t2-bakeoff-results.md).

**Exit:** seam + config pick; bake-off table committed. Default `heuristic` (winner rule did not fire). TSG stitch column is `skip_runtime` (pin fetched; `build_stack_graph_into` panics on tree-sitter-java 0.23.5 vs pin `=0.23.4`).

**Sign-off checklist (PD4)**

- [x] Exit criteria for this WP met (T2 config pick; `StackGraphResolver` loads pinned Java TSG when selected; bake-off table; default stays heuristic)
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, vendored Tree-sitter C, engine pack source we do not own, `integration/`) — **95.24% lines**
- [x] 80% mutants on listed crates that changed — core **141 caught / 147 scored (95.9%)**, 8 unviable; resolve **117 caught / 141 scored (83.0%)**, 27 unviable; lang-java **82 caught / 94 scored (87.2%)**, 10 unviable
- [x] No `sleep` in crate unit tests
- [x] `check-static` if ELF changed — **N/A** (no shipped ELF change). Do not run it on a Darwin Mach-O.
- [x] [design-patterns.md](design-patterns.md) table updated for `T2Backend` / `T2Table` / `T2Strategy` / `TsgPin` / `TsgLoadState`
- [x] Docs in this tree updated if a locked decision was refined (heuristics remain default; plugin-sdk + language-matrix + spike agree)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the PD4 unit gate on macOS.
- Stack-graphs pin `https://github.com/github/stack-graphs.git` @ `fcb7705d5b38ae13b3665a9b2c882e5a97243d44` fetched. No `third_party/` dump.
- Optional `--features t2-stack-graphs` compiles the runtime. Slim default omits it. Do not treat Darwin RSS as a musl green.
- Post-dev stack ends here. Do not open a `pd5` branch.

## IDE-0 — POC IDE documentation

**Status: SIGNED OFF** on branch `ide0`. Do not start IDE-1 crates until this section is signed off (it is).

**Scope:** [poc-ide/](poc-ide/README.md) plus index/branch/milestone/pattern/testing updates. No `poc-ide` crate yet.

**Exit**

- [x] [poc-ide/README.md](poc-ide/README.md), [architecture.md](poc-ide/architecture.md), [third-party.md](poc-ide/third-party.md), [agent-context.md](poc-ide/agent-context.md) exist and agree (egui consumer, Ports, no Node, no musl IDE ELF).
- [x] branching / implementation-plan / this file list `ide0`–`ide5`.
- [x] [design-patterns.md](design-patterns.md) names every POC type in the architecture doc.
- [x] [testing.md](testing.md) states poc-ide coverage, mutants, ignore `main.rs`/`ui.rs`, `check-static` N/A.
- [x] [requirements.md](requirements.md) / [vision.md](vision.md) / [consumer.md](consumer.md) record that `poc-ide/` is a consumer sample, not the intelligence product.

**Sign-off checklist (IDE-0)**

- [x] Exit criteria met
- [x] Tests / llvm-cov / mutants / `sleep` / `check-static` — **N/A** (docs only)
- [x] Docs in this tree updated

## IDE-1 — Shell: open, tree, tabs, layout

**Status: SIGNED OFF** on branch `ide1`. Do not start IDE-2 until this section stays signed off. No buffers-as-rope, no LSP.

- `poc-ide` crate: lib + tiny `main.rs` composition root.
- `DialogPort` + `rfd` (bin); `FakeDialog` tests: open folder sets `WorkspaceRoot`; open file sets root to parent and a selected path.
- `FileTree` Composite via `FsPort` (`std::fs` recursion / `MemFs`); display-skip `.git` / `target` / `node_modules`.
- `LayoutState` left-panel width; persist in-process only (no config file required).
- `TabStrip` custom view in `ui.rs` (egui_dock 0.21 rust-version 1.95 does not pin on rustc 1.87); tests: open/focus/close tabs by path.

**Exit:** domain can open a folder or file, list a tree, hold tabs, and record a resizable panel width. `cargo run -p poc-ide` shows left tree + tabs + empty editor pane with a draggable splitter. CLI `--folder DIR` / `--file PATH` skip the dialog. No `thread::sleep`.

**Sign-off checklist (IDE-1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (exclude `xtask/`, bin `main.rs`, `poc-ide/src/ui.rs`, vendored Tree-sitter C, engine pack source we do not own, `integration/`) — **95.43% lines**
- [x] 80% mutants on listed crates that changed — poc-ide **100 caught / 100 scored (100%)**, 23 unviable
- [x] No `sleep` in crate unit tests
- [x] `check-static` if ELF changed — **N/A** (GUI is not a shipped musl ELF)
- [x] [design-patterns.md](design-patterns.md) table updated for IDE-1 types (`IdeError`, `DirEntry`)
- [x] Docs in this tree updated if a locked decision was refined (egui 0.36 `Panel::left`; custom tab bar; `std::fs` tree walk)

## IDE-2 — Edit + highlight + save

**Status: SIGNED OFF** on branch `ide2`. Do not start IDE-3 until this section stays signed off. No DiskWatch modal, no LSP.

- `OpenBuffer` + `ropey`; `EditCommand` insert/delete/select; cut/copy/paste via `ClipboardPort`.
- Save / open file bytes via `FsPort`.
- `Highlighter` Adapter (syntect); unknown language → no panic, plain style.
- Bin: multiline editor bound to the active buffer; syntax colors from highlighter tokens.

**Exit:** insert/delete/select/cut/copy/paste/save unit tests on `MemFs` + `FakeClipboard`. Highlighting returns non-empty spans for a `.rs` fixture and a no-panic path for `.unknown`.

**Sign-off checklist (IDE-2)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov on crates that exist (same excludes as IDE-1) — **95.59% lines**
- [x] 80% mutants on listed crates that changed — poc-ide **211 caught / 213 scored (99.1%)**, 39 unviable, 2 missed
- [x] No `sleep` in crate unit tests
- [x] `check-static` — **N/A**
- [x] Pattern table updated (`HighlightSpan`, `ArboardClipboard`)
- [x] Docs updated if a locked decision was refined (`ropey` 1.6.1, `syntect` 5.3.0, `arboard` 3.6.1)

## IDE-3 — Disk conflict modal

**Status: SIGNED OFF** on branch `ide3`. Do not start IDE-4 until this section stays signed off. No LSP.

- `DiskWatch` Observer on `WatchPort`; `FakeWatch` + `FakeClock` (local `ClockPort` in poc-ide; no `progressive-lsp-core` dep).
- Open buffer + watch event on that path → `ConflictModal` with `LoadDisk` | `KeepMemory`.
- Always prompt when the open path changes on disk (including clean buffers).
- `LoadDisk` rereads `FsPort` and clears dirty; `KeepMemory` keeps rope and records `ignored_mtime`.

**Exit:** FakeWatch modify on an open dirty buffer surfaces the modal; both choices have invariant tests. No `thread::sleep`.

**Sign-off checklist (IDE-3)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov (same excludes) — **95.79% lines**
- [x] 80% mutants on poc-ide (and listed crates that changed) — poc-ide **278 caught / 282 scored (98.6%)**, 60 unviable, 4 missed
- [x] No `sleep`
- [x] `check-static` — **N/A**
- [x] Pattern table updated (`DiskEvent`, `DiskEventKind`, `SystemClock`)
- [x] Docs updated if a locked decision was refined (`notify` 8.2.0; live `RecommendedWatcher` in the bin)

## IDE-4 — Language catalog + stock LSP discovery

**Status: SIGNED OFF** on branch `ide4`. Do not start IDE-5 until this section stays signed off. No Envelope console (read `experimental.progressiveLsp` only).

- `LanguageCatalog` extension → `languageId` (table in [poc-ide/architecture.md](poc-ide/architecture.md)).
- `ServeMode::StockStdio`; `LspClient` + `FakeLsp`. `ServeMode::ControlSocket` is present and unused.
- `didOpen` / `didChange` / `didSave` / `didClose`.
- `textDocument/definition`, `implementation`, `references` → jump list / open tab.
- Missing binary → domain error, editor remains usable.

**Exit:** FakeLsp answers definition/implementation/references for a fixture path; catalog maps the v1 extensions; plaintext skips `didOpen`. Integration against a live `progressive-lsp` binary is optional and must not use `sleep` (deadline poll only if added under `poc-ide` tests that are clearly integration-gated — prefer FakeLsp for the unit gate).

**Sign-off checklist (IDE-4)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov (same excludes) — **95.86% lines**
- [x] 80% mutants on poc-ide — poc-ide **535 caught / 555 scored (96.4%)**, 88 unviable, 17 missed, 3 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A**
- [x] Pattern table updated (`LspCall`, `ProgressiveLspCap` poc-ide)
- [x] Docs updated if a locked decision was refined (`lsp-types` 0.97.0; `ControlSocket` unused until IDE-5)

## IDE-5 — Protocol console + progressive.v1

**Status: SIGNED OFF** on branch `ide5`. Last POC branch. Do not open `ide6`. This stack is complete.

- `ServeMode::ControlSocket`; `ControlClient` using `progressive-lsp-control`.
- `ProtocolConsole`: append-only LSP JSON-RPC transcript + send; Envelope unary RPCs + `WatchBatch` / `TierReady` display.
- `FakeControl` for every RPC in the [user API table](user/progressive-v1-api.md). `--mux` is `pending_mux`.

**Exit:** FakeControl round-trips GetConfig / SetConfig / ReloadConfig / InstallPacks / WatchSubscribe / FilesSince / IndexStatus / TierStatus / ReloadScripts; pushes appear in the transcript with `request_id == 0`. Stock FakeLsp inspector still sends definition. Live serve is not required for sign-off.

**Sign-off checklist (IDE-5)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch
- [x] 95% llvm-cov (same excludes) — **95.99% lines**
- [x] 80% mutants on poc-ide — poc-ide **683 caught / 711 scored (96.1%)**, 115 unviable, 20 missed, 8 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A**
- [x] Pattern table updated (`ControlPush`, `TranscriptKind`, `IdeError::Control`)
- [x] Docs updated if a locked decision was refined (`progressive-lsp-control` consumer MAY; `--mux` is `pending_mux`)

## poc-log — per-run sqlite debug log

Post-IDE-5 slice on current `main` (branch `poc-log`). **Not IDE-6.** Adds `RunLog` so each `cargo run -p poc-ide` writes a sqlite file under `$HOME/.progressivelsp/poc-ide-runs/` (or `POC_IDE_LOG_DIR`). Tests inject `:memory:` / tempfile. No new milestone number.

## poc-tree-lazy — shallow FileTree load

Stacked on `poc-log` (not IDE-6). `FileTree::load` / `FsPort.read_tree` list one directory level; child dirs start unloaded. `FileTree::expand` fills the next level. Opening a folder paints immediately.

## poc-tree-collapsed — default collapsed tree

Stacked on `poc-tree-lazy` (not IDE-6). `TreeExpansion` starts empty; a path is expanded iff the user expands it. Opening a folder does not unfold every directory.

## poc-compact-folders — compact single-child directory chains

Stacked on `poc-tree-collapsed` (not IDE-6). Directories that each have exactly one child directory display as `a/b/c`. `CompactChain` is a view of already-loaded Composite children — an unloaded dir cannot claim "exactly one child." Expanding `a` may load `b`/`c` for one compact row without expanding nested `TreeExpansion` entries. The compact row path is the innermost directory.

## poc-context-menu — editor context menu for resolver actions

Stacked on `poc-compact-folders` (not IDE-6). Right-click on the editor (and file tree rows) offers Find Definition / Implementation / References. Those items run the same `DiscoverCommand` as Navigate / F12 (focused tab + cursor). No new milestone number.

## poc-navigate — deferred Navigate + editor caret sync

Stacked on `poc-context-menu` (not IDE-6). Navigate records `PendingDiscover` and applies after the menu closes (`close_kind(Menu)` so the shell does not collapse). After `TextEdit::show`, `CursorOffsets` copies the caret onto `OpenBuffer.selection` so Go to Definition / F12 / context menu use the visible caret, not a stale 0,0. Protocol console stays. No new milestone number.

## poc-no-console — drop the hand-typed protocol console

Stacked on `poc-navigate` (not IDE-6). The bin no longer draws a bottom Protocol console (method picker, JSON/TOML body, Send, transcript). Debug is `RunLog` sqlite. `ProtocolConsole` stays in the lib for Envelope/LSP transcript tests. `ControlClient` / `UnixControl` / `ServeMode::ControlSocket` remain. `PendingDiscover` still applies after the editor caret sync. No new milestone number.

## poc-dialog-defer — File dialog after the menu closes

Stacked on `poc-no-console` (not IDE-6). Open Folder / Open File records `PendingDialog` and applies on the next frame so `rfd` is not invoked from inside `menu_button` (that freeze looks like a hung UI with no dialog). Cancel is `DialogOutcome::Cancelled`. No new milestone number.

## poc-open-unblock — progressive folder bind

Stacked on `poc-dialog-defer` (not IDE-6). Opening a large tree no longer blocks the UI on a recursive `notify` watch or on `initialize`. Watch is `WatchDepth::Immediate`; nested dirs are watched on expand. LSP spawn+initialize runs on a worker thread (`LspSessionState::Connecting`) and `didOpen` is replayed when ready. No new milestone number.

## poc-tree-sort — directories first, dots last

Stacked on `poc-open-unblock` (not IDE-6). Tree listing order is non-dot directories, non-dot files, dot directories, dot files; lexicographic within each group. No new milestone number.

## poc-discover-log — IDE-side navigation diagnostics

Stacked on `poc-tree-sort` (not IDE-6). Discover sqlite rows include `path`, `uri`, `line`, `character`, and `location_count` so an empty server result is distinguishable from a jump. `file_uri` percent-encodes spaces. No new milestone number.

## Later post-v1 (not in PD0–PD4 / IDE-0–IDE-5)

Java in-house types (still no JVM). Dual-run PHP T3 if the other spike wins. oxc_type_checker as TS T3. Native macOS/Windows **server** hosts. WASM plugin ABI. HTTP/S3 transport in-tree. Buck2 if engine builds outgrow Docker cache. Watchman. `$/` JSON mirror of `progressive.v1` only if a real client cannot open a socket or mux.
