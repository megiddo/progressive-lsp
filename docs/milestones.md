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

- Content-addressed `IndexCache` under `$PREFIX/cache/` keyed `(grammar_ver, language_id, file_hash)`. Disk marker does not skip extract on cold start (symbols are in-memory only). Never written into the git worktree.
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

## LOG-0 — Global logging documentation

**Status: SIGNED OFF** on branch `log0`. Do not start LOG-1 crates until this section is signed off (it is). Parent is **current `main`** (PR #4 / `poc-discover-log` already merged). Do not stack on `poc-no-console`.

**Scope:** [logging.md](logging.md), [logging-plan.md](logging-plan.md), [logging/agent-context.md](logging/agent-context.md) plus index/branch/milestone/pattern/testing/architecture/detailed-design/host-deps updates. No `progressive-lsp-log` crate. No rusqlite in server. No `eprintln!` changes.

**Exit**

- [x] [logging.md](logging.md) is the source of truth; [logging-plan.md](logging-plan.md) is the mutation plan (LOG-1–LOG-4 file-level).
- [x] branching / implementation-plan / this file list `log0`–`log4` stacked on current `main`.
- [x] [design-patterns.md](design-patterns.md) names every LOG type in [logging.md](logging.md) (types do not exist in Rust yet).
- [x] [testing.md](testing.md) states `progressive-lsp-log` on 95% / 80% once it exists.
- [x] [host-deps.md](host-deps.md): sqlite amalgamation is our artifact (static C in the ELF).
- [x] [poc-ide/third-party.md](poc-ide/third-party.md): rusqlite allowed in `progressive-lsp-log`; two schemas.
- [x] poc-ide `RunLog` stays a separate schema. Do not merge.

**Sign-off checklist (LOG-0)**

- [x] Exit criteria met
- [x] Tests / llvm-cov / mutants / `sleep` / `check-static` — **N/A** (docs only)
- [x] Docs in this tree updated

## LOG-1 — Port + records (no sqlite in server)

**Status: SIGNED OFF** on branch `log1`. Do not start LOG-2 until this section is signed off (it is). Parent is `log0`. No `progressive-lsp-log` crate. No rusqlite in server. No `eprintln!` changes.

- `LogPort`, `LogRecord`, `LogLevel`, `LogOrigin`, `LogComponent`, `LogScope`, `FakeLog`, `MemoryLog`, `NullLog`, `NeverFailLog` in `progressive-lsp-core`.
- `[log]` overlay: `level`, `path`; invalid level → warning + default `info`.

**Exit**

- [x] FakeLog records; scope nest/restore; never-fail NullLog; config `[log]` unknown key still warns.
- [x] Core stays sqlite-free.

**Sign-off checklist (LOG-1)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p progressive-lsp-core -- --test-threads=1` (69 passed)
- [x] 95% llvm-cov on crates that exist — **96.29%** lines (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`)
- [x] 80% mutants on listed crates that changed — core **201 caught / 217 scored (92.6%)**, 33 unviable, 16 missed
- [x] No `sleep`
- [x] `check-static` — **N/A** (no rusqlite yet; Darwin: do not fake musl greens)
- [x] Docs in this tree updated

## LOG-2 — WAL repository crate

**Status: SIGNED OFF** on branch `log2`. Do not start LOG-3 until this section is signed off (it is). Parent is `log1`. No product `eprintln!` death. No serve/install bootstrap wire. No capture bridges.

- `progressive-lsp-log` workspace member: `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath`.
- Pin `rusqlite` **=0.40.2** `bundled`. Amalgamation is our artifact. `LIBSQLITE3_FLAGS` omits loadable extensions. `check-static` fails closed if `libdl` is `DT_NEEDED`.

**Exit**

- [x] tempfile WAL; `PRAGMA journal_mode` returns `wal`; injected COMMIT failure retries without panic; `BATCH_MAX=1`; FakeClock; re-entrancy; Drop flushes. No `thread::sleep`.
- [x] Core stays sqlite-free.

**Sign-off checklist (LOG-2)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p progressive-lsp-log -- --test-threads=1` (33 passed)
- [x] 95% llvm-cov on crates that exist — **96.04%** lines (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`)
- [x] 80% mutants on listed crates that changed — `progressive-lsp-log` **91 caught / 104 scored (87.5%)**, 17 unviable, 13 missed, 3 timeouts
- [x] No `sleep`
- [x] `check-static` — fixture ELFs including `libdl` `DT_NEEDED` fail-closed. Darwin: do not fake musl greens (see notes)
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-2 gate on macOS.
- This host cannot produce a musl `progressive-lsp` ELF (`xtask musl` is Docker). `xtask check-static` is unit-tested against in-tree fixture ELFs, including a `libdl.so.2` `DT_NEEDED` that must fail closed. A green fixture result is not a claim that a rusqlite-linked musl core ELF was built here.
- The bin is not wired to `SqliteLogRepository` (LOG-4). Linux CI must `check-static` the core ELF once rusqlite is linked into it. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-3 — Facades, bridges, eprintln death

**Status: SIGNED OFF** on branch `log3`. Do not start LOG-4 until this section is signed off (it is). Parent is `log2`. No sqlite serve/install bootstrap. No `LogScope` around didOpen. poc-ide `RunLog` unchanged. poc-ide `StdioLsp` still uses `stderr(Stdio::null())` (do not inherit engine stderr into the IDE).

- Bridges + child/file/LSP capture Adapters. Composition root passes `LogPort` (`MemoryLog` bootstrap). `ConfigLoad.warnings` emit. Product diagnostic `eprintln!` gone except CLI usage/help (IT-1.7).
- Allowlist clangd `--log=`; optional gopls `-logfile`. Not `-rpc.trace`, `RA_LOG_FILE`, or `TY_LOG_PROFILE`.
- `ChildIo` Value: stdout is LSP; stderr is an optional pipe. `NullStderrAdapter` is forbidden on prod pack spawn.

**Exit**

- [x] Grep of diagnostic `eprintln!` in `src/` and `progressive-lsp-*` is empty except tests and `CliUsageAdapter` (IT-1.7).
- [x] Engine stdout is never a log Adapter.

**Sign-off checklist (LOG-3)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1` (all passed)
- [x] 95% llvm-cov on crates that exist — **96.44%** lines (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`)
- [x] 80% mutants on listed crates that changed — `progressive-lsp-log` **137 caught / 151 scored (90.7%)**, 17 unviable, 14 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` — fixture ELFs including `libdl` `DT_NEEDED` fail-closed. Darwin: do not fake musl greens (sqlite serve bootstrap is LOG-4; no musl ELF on this host)
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-3 gate on macOS.
- `SqliteLogRepository` is not wired into `serve` / `install` (LOG-4). Linux CI must `check-static` the core ELF once rusqlite is linked into it. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-4 — Wire serve/install + docs lock

**Status: SIGNED OFF** on branch `log4`. Parent of `log5`. Do not reopen LOG-0–LOG-4. Remaining operational coverage is LOG-5+ ([logging.md](logging.md) coverage matrix). Parent is `log3`. poc-ide `RunLog` unchanged. IT-1.7 usage still stderr.

- Bootstrap order from [logging.md](logging.md): `MemoryLog` → prefix/`ensure_dirs` → `SqliteLogRepository` (keep `MemoryLog` if open fails) → replay ring → bridges → config + `ConfigWarnAdapter` → serve/install → `Flush` + join on shutdown.
- `LogScope` around didOpen/didChange/definition. Index/watch/install silent-failure paths emit.
- User troubleshooting: sqlite under `$PREFIX/log/`. `PROGRESSIVE_LSP_LOG` overrides path. stdout stays JSON-RPC.

**Exit**

- [x] One WAL file per serve/install process; `Flush` + join on shutdown; poc-ide `RunLog` unchanged.

**Sign-off checklist (LOG-4)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1` (all passed)
- [x] 95% llvm-cov on crates that exist — **95.93%** lines (ignore `xtask/`, `/src/main.rs$`, `tree-sitter`, `poc-ide/src/ui.rs`)
- [x] 80% mutants on listed crates that changed — log **140 caught / 154 scored (90.9%)**, 13 unviable, 12 missed, 2 timeouts; index **160 caught / 194 scored (82.5%)**, 22 unviable, 34 missed; install **114 caught / 122 scored (93.4%)**, 6 unviable, 8 missed; watch **97 caught / 101 scored (96.0%)**, 15 unviable, 4 missed
- [x] No `sleep`
- [x] `check-static` — fixture ELFs including `libdl` `DT_NEEDED` fail-closed. Darwin: do not fake musl greens (no musl ELF on this host). Linux CI must `check-static` the rusqlite-linked core ELF.
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-4 gate on macOS.
- The composition-root bin **is** wired to `SqliteLogRepository`. This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` that ELF. Do not run `check-static` on a Darwin Mach-O and call it green.
- IT-1.7 usage/help stays on stderr. Optional sqlite file after `serve` handshake is asserted on Linux CI / Docker (IT-1.1) and by the Darwin tempfile unit test.

## LOG-5 — Remaining-coverage documentation ingest

**Status: SIGNED OFF** on branch `log5`. Do not start LOG-6 crates until this section is signed off (it is). Parent is `log4`. Docs only. No Rust. No rusqlite changes. No `eprintln!` changes. Do not reopen LOG-0–LOG-5.

**Scope:** [logging.md](logging.md) coverage matrix + durable fallback, [logging-plan.md](logging-plan.md) LOG-5+ mutation, [logging/agent-context.md](logging/agent-context.md), this file, [implementation-plan.md](implementation-plan.md), [branching.md](branching.md), [design-patterns.md](design-patterns.md) `LogOpenPlan`, user troubleshooting. poc-ide `RunLog` unchanged.

**Exit**

- [x] [logging.md](logging.md) remaining problem + coverage matrix is the zero-blind-spots definition; locked decisions unchanged.
- [x] branching / implementation-plan / this file list `log5`–`log11` stacked on `log4`. “Do not open log5” superseded.
- [x] [design-patterns.md](design-patterns.md) names `LogOpenPlan` and updated ServeLogPath / supervisor / ScriptHost / LspFacade rows.
- [x] poc-ide `RunLog` stays a separate schema.

**Sign-off checklist (LOG-5)**

- [x] Exit criteria met
- [x] Tests / llvm-cov / mutants / `sleep` / `check-static` — **N/A** (docs only)
- [x] Docs in this tree updated

## LOG-6 — Supervisor + ScriptHost lifecycle emits

**Status: SIGNED OFF** on branch `log6`. Parent is `log5`. No `Command` spawn. No ChildIo pipe readers. No protocol/control emits. No `LogOpenPlan`. poc-ide `RunLog` unchanged.

- `EngineSupervisor::with_log` / `ScriptHost::with_log`. Emit `try_spawn` (NotDiscovered, Hash, Aborted, Backoff, stub/reserved Spawn), `note_crash`, bootstrap Abort, engine-spawn Skip, pre_index skip.
- Composition root **holds** the supervisor (do not `let _supervisor = supervisor`) and `try_spawn`s after initialize has a workspace root so the refuse is a sqlite row.

**Exit**

- [x] FakeLog asserts on `warn`/`info`, `operation=spawn`/`initialize`/`index`, `component=engine`/`script`. No sleep. Stub refuse is visible without real `Command`.

**Sign-off checklist (LOG-6)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.90%** lines
- [x] 80% mutants on listed crates that changed (engine, script) — engine **134 caught / 156 scored (85.9%)**, 63 unviable, 22 missed; script **64 caught / 76 scored (84.2%)**, 4 unviable, 12 missed; combined **198 caught / 232 scored (85.3%)**, 67 unviable, 34 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` — fixture `libdl` fail-closed. Darwin: do not fake musl greens
- [x] [design-patterns.md](design-patterns.md) rows updated (`EngineSupervisor` / `ScriptHost` take `LogPort`)
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-6 gate on macOS.
- `PackAdapter::spawn` still refuses exec. Tests assert the **warn** row, not a live child.
- This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` the rusqlite-linked ELF. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-7 — Protocol + control socket + install hash

**Status: SIGNED OFF** on branch `log7`. Parent is `log6`. No sqlite-open fallback. No child capture. No LSP/Envelope **bodies** in logs. No `-rpc.trace`. poc-ide `RunLog` unchanged.

- `LspFacade::with_log`: parse, missing Content-Length, method-not-found, mux errors.
- `bind_control_socket` / accept / `PayloadTooLarge` / unknown Envelope method / `Status::error` / `--control-fd` ignored.
- `Installer::apply_with_verify`: `InstallError::Hash` and verify refuse emit **before** `remove_or_emit`.

**Exit**

- [x] FakeLog asserts `operation=protocol`/`control`/`install`; hash expected/actual hex; no payload bytes in `message`/`extras`.

**Sign-off checklist (LOG-7)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.89%** lines
- [x] 80% mutants on listed crates that changed (protocol, control, install) — protocol **107 caught / 130 scored (82.3%)**, 18 unviable, 23 missed, 3 timeouts; control **88 caught / 95 scored (92.6%)**, 1 unviable, 7 missed; install **117 caught / 124 scored (94.4%)**, 10 unviable, 7 missed, 1 timeout; combined **312 caught / 349 scored (89.4%)**, 29 unviable, 37 missed, 4 timeouts
- [x] No `sleep`
- [x] `check-static` — fixture `libdl` fail-closed. Darwin: do not fake musl greens
- [x] [design-patterns.md](design-patterns.md) rows updated (`LspFacade` / `ControlServer` / `bind_control_socket` take `LogPort`)
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-7 gate on macOS.
- `--control-fd` stays parsed-and-ignored (`pending`); tests assert the **warn** row, not a live fd.
- This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` the rusqlite-linked ELF. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-8 — T3 skip + session completeness

**Status: SIGNED OFF** on `log8`. Parent is `log7`. Do not fail the user on T3 skip. Do not emit every `definition` after the first skip for that pair. No `Command` spawn.

- `EngineResolver` first in the session chain when supervisor attached; once-per `(language, package)` `info` `operation=resolve`.
- `initialize` success info / fail warn; `didClose` debug; `shutdown` debug; `FilesSince` truncated info; unknown language / `UnsupportedLanguage` info once.

**Exit**

- [x] FakeLog: one skip row then a second definition does not duplicate; T2/T1 still `Ready`; truncated FilesSince emits; initialize fail is in sqlite **and** JSON-RPC `-32002`.

**Sign-off checklist (LOG-8)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.01%** lines
- [x] 80% mutants on listed crates that changed (engine resolve, resolve chain) — engine **154 caught / 172 scored (89.5%)**, 67 unviable, 18 missed; resolve **113 caught / 131 scored (86.3%)**, 38 unviable, 18 missed; combined **267 caught / 303 scored (88.1%)**, 105 unviable, 36 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` — fixture `libdl` fail-closed. Darwin: do not fake musl greens
- [x] Pattern table updated (`EngineResolver` skip-once invariant)
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-8 gate on macOS.
- This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` the rusqlite-linked ELF. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-9 — Durable WAL fallback

**Status: SIGNED OFF** on `log9`. Parent is `log8`. No syslog / journald / OTel / JSON files. No child capture.

- `LogOpenPlan`: primary `ServeLogPath` → `fallback` → `in_temp`. Replay `MemoryLog`. `error` only if all three fail.

**Exit**

- [x] tempfile: primary open fails, fallback WAL exists, contains warn + replayed ring; `BATCH_MAX=1`; no `$HOME`; no sleep.
- [x] User README names `serve-fallback-*.sqlite` and the temp WAL.

**Sign-off checklist (LOG-9)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p progressive-lsp-log` and composition-root tests `--test-threads=1`
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.02%** lines
- [x] 80% mutants on listed crates that changed (`progressive-lsp-log`) — **140 caught / 153 scored (91.5%)**, 36 unviable, 12 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` — Darwin: do not fake musl greens
- [x] [design-patterns.md](design-patterns.md) names `LogOpenPlan`
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-9 gate on macOS.
- This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` the rusqlite-linked ELF. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-10 — Child capture wiring

**Status: SIGNED OFF** on `log10`. Parent is `log9`. Ready when spawn exists. **Do not** implement `PackAdapter` `Command`. Tests use `FakeChildStderr`. `NullStderrAdapter` forbidden on prod pack spawn. Never attach Adapter to engine stdout.

**Exit**

- [x] `FakeChildStderr` drain → FakeLog third-party `operation=spawn`; overflow drops oldest; stdout never attached.

**Sign-off checklist (LOG-10)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1` (llvm-cov workspace suite green; log 56, engine 38, composition-root 59)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.07%** lines
- [x] 80% mutants on listed crates that changed (log, engine) — log **146 caught / 159 scored (91.8%)**, 39 unviable, 12 missed, 1 timeout; engine **168 caught / 185 scored (90.8%)**, 69 unviable, 17 missed
- [x] No `sleep`
- [x] `check-static` — Darwin: do not fake musl greens
- [x] Pattern table: no new type unless a Port is required; reuse `ChildStderrAdapter`
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-10 gate on macOS.
- This host cannot produce a musl `progressive-lsp` ELF. Linux CI must `check-static` the rusqlite-linked ELF. Do not run `check-static` on a Darwin Mach-O and call it green.

## LOG-11 — Operational Err hygiene gate

**Status: SIGNED OFF** on `log11`. Parent is `log10`. Last LOG branch of this stack. No new Adapters. poc-ide `RunLog` unchanged. There is no `log12`.

- Grep/allowlist: every operational `Err` on `serve`/`install` emits or is listed as client-visible only with a reason in [logging.md](logging.md).

**Exit**

- [x] Hygiene test green; coverage matrix has no silent class; stack complete at `log11`.

**Sign-off checklist (LOG-11)**

- [x] Exit criteria met
- [x] Tests on this branch — hygiene test + `cargo test --workspace -- --test-threads=1` (hygiene 6; composition-root 59; log 56; engine 38)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.07%** lines
- [x] 80% mutants — **N/A** (hygiene test + docs only; no listed crate logic change)
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the LOG-11 gate on macOS.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green.

## poc-proof-log — empty discover + POC debug spawn

**Status: SIGNED OFF** on branch `poc-proof-log`. Parent is **current `main`** (log11 merge / `a0f10a2`). Do not start `poc-lsp-async` from this branch. Do not reopen LOG-0–LOG-11. There is no `log12`.

**Scope:** Serve empty F12 is an **info** row; `PROGRESSIVE_LSP_LOG_LEVEL`; poc-ide default `ControlSocket` + debug child + stderr→RunLog + `ProofStatus` footer; `xtask poc`. Not LSP IO threads, IndexStatus ingest fields, DiscoverOffer menus, or highlight cache.

**Exit**

- [x] Empty definition / implementation / references emit **info** with `location_count=0` extras; a hit stays debug; `didChange` stays debug.
- [x] `PROGRESSIVE_LSP_LOG_LEVEL` overrides `[log].level`; invalid → warn + `info`; stock serve without the env stays info.
- [x] poc-ide default-spawns `serve --control-socket` with `PROGRESSIVE_LSP_LOG_LEVEL=debug`, piped stderr into RunLog, and a footer that shows both sqlite paths + last discover.
- [x] `cargo xtask poc` is the supported proof launch.

**Sign-off checklist (poc-proof-log)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (core 72; composition-root lib 68; poc-ide lib 176; xtask 30; hygiene 6)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.10%** lines
- [x] 80% mutants on listed crates that changed — core in-diff **7/7 (100%)**; poc-ide in-diff **34/37 (91.9%)**; composition-root session/lib in-diff **13/13 (100%)**; combined **54/57 (94.7%)**
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green.

## poc-lsp-async — LSP/control IO threads

**Status: SIGNED OFF** on branch `poc-lsp-async`. Parent is `poc-proof-log` (`0d9f6a8`). `poc-tier-status` stacks on this branch. `RunLog` stays a separate schema from the serve WAL.

**Scope:** One `poc-ide-lsp` thread owns child stdin/stdout and the stderr drain. UI submits `LspIoRequest` and polls `LspIoEvent`. Discover / didChange never block `fn ui`. `$/progress` and `window/logMessage` are kept. Control IO is a second thread; `fn ui` never calls `index_status()` / `tier_status()`. In-flight discover disables F12 / context-menu items with `waiting for server`.

**Exit**

- [x] UI apply path never calls `LspTransport::request`; jump / last-discover apply when the inbox yields.
- [x] `$/progress` and `window/logMessage` are retained (`ProgressEvent` / `LogMessageEvent`).
- [x] Discover in flight: F12 and context-menu discover items disabled, label `waiting for server`; tree / tabs / typing stay live (`didChange` queued).
- [x] Control pushes (`TierReady`, `WatchBatch`) land in `ControlPushInbox` off `fn ui`.

**Sign-off checklist (poc-lsp-async)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (core 72; composition-root lib 68; poc-ide lib 188; xtask 30; hygiene 6)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.94%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **27/27 (100%)** caught (9 unviable)
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green.

## poc-tier-status — ingest field + T1/T2/T3 strip + honest menus

**Status: SIGNED OFF** on branch `poc-tier-status`. Parent is `poc-lsp-async` (`1407212`). Do not start `poc-no-stall` from this branch. Do not invent highlight cache or tree-expand workers. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Additive `IndexStatus.ingest`; T1/T2/T3 status strip; context / Navigate menus from `LanguageCatalog` × current tier. Not highlight cache or tree-expand worker.

**Exit**

- [x] `IndexStatusResponse.ingest` is `not_started` / `running` / `done`, filled from session ingest reality.
- [x] Status strip paints T1 / T2 / T3 for the focused package (`processing` / `done` / `not supported` / `skipped` / `n/a`). Folder open starts workspace ingest; T1/T2 follow that ingest with no file focused. Java T3 is `not supported`. Stub refuse is `skipped`, not `done`.
- [x] Context and Navigate menus use `DiscoverOffer`. Disabled items do not call FakeLsp.

**Sign-off checklist (poc-tier-status)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (control 19; poc-ide lib 193; composition-root lib 69; core 72; xtask 30)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.96%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **42/42 (100%)** caught (8 unviable); control in-diff **13/13 (100%)**; composition-root in-diff **3/3 (100%)**; combined **58/58 (100%)**
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green. Do not fake a types engine.

## poc-no-stall — highlight cache + tree expand worker + F12 until Ready

**Status: SIGNED OFF** on branch `poc-no-stall`. Parent is `poc-tier-status` (`2ac3992`). This is the last POC-proof slice. Do not reopen POC-proof WPs. The allowed next stack is `host0` (HOST-0). Do not implement PackAdapter `Command` spawn from this branch. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Highlight cache keyed by path + rope generation; tree expand `read_dir` on a worker (header may look open, children `loading…`); Navigate / F12 disabled until `LspSessionState::Ready`. Not PackAdapter spawn. Not merging RunLog with the serve WAL.

**Exit**

- [x] Second highlight of unchanged text does not re-tokenize (`tokenize_count` stays).
- [x] Tree expand lists via `ExpandChainCommand` on a worker; UI applies `CompactChainListing`; expand is disabled on that row until ready.
- [x] Navigate / F12 stay disabled with `connecting language server` until Ready. Keyboard F12 uses the same `DiscoverMenu` gate.

**Sign-off checklist (poc-no-stall)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (poc-ide lib 203; composition-root lib 69; core 72; xtask 30)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.96%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **67 caught / 78 scored (85.9%)**, 34 unviable, 10 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green. Do not fake a types engine.

## HOST-0 — native vs container open + RuntimePort

**Status: SIGNED OFF** on branch `host0`. Parent is `poc-no-stall` (`63507ff`). Do not open `host1` from this branch. Do not implement PackAdapter `Command` spawn. Do not `docker run` attach. Do not build musl images. Do not mux client. Tests never talk to a Docker daemon, registry, or AWS (`FakeRuntime` / scripted docker CLI only). `RunLog` stays a separate schema from the serve WAL.

**Scope:** Native vs container File menu; `T3HostOffer`; `LaunchJournal` / `StatusModal`; `RuntimePort` + `FakeRuntime`; `DockerRuntime` probe/image only (`start` still unwired); core `file_uri`; host stack docs. Not PackAdapter `Command`. Not runtime image. Not mux.

**Exit**

- [x] Non-Linux File menu: **Open Folder…** (native T1/T2) and **Open Folder in Container…** (one Linux serve). Linux has no container item; native open is the full host.
- [x] `T3HostOffer::NeedsContainer` paints T3 `skipped` / discover `open folder in container` on native non-Linux. Never two LSP processes.
- [x] T1/T2/T3 strip buttons open `StatusModal` + `LaunchJournal`. Close does not cancel work.
- [x] Container launch journal is probe → platform → image → mount → start → T3 preflight. Tests inject `FakeRuntime`. `DockerRuntime::start` errors (attach is a later slice).
- [x] Core `path_to_file_uri` / `path_from_file_uri` percent-encode the same as poc-ide; incoming `file:` URIs decode before index lookup.
- [x] Host stack docs: [host/agent-context.md](host/agent-context.md), branching `poc-no-stall └── host0` with `host1`–`host7` listed as future.

**Sign-off checklist (HOST-0)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (poc-ide lib 214; composition-root lib 73; core 74)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.99%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **129 caught / 140 scored (92.14%)**, 61 unviable, 10 missed, 1 timeout; core in-diff **46 caught / 50 scored (92.00%)**, 4 unviable, 1 missed, 3 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `HostOs`, `OpenMode`, `T3HostOffer`, `LaunchFlags`, `RuntimePort`, `FakeRuntime`, `DockerRuntime`, `RuntimeInfo`, `RuntimeSession`, `LaunchJournal`, `LaunchStep`, `StepState`, `StatusModal`, `RuntimeIo*`, `file_uri` / `path_to_file_uri`

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- Tests never talk to a Docker daemon. `DockerRuntime` uses a missing binary or a scripted CLI.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green. Do not fake a types engine.

## HOST-1 — PackAdapter Linux Command spawn

**Status: SIGNED OFF** on branch `host1`. Parent is `host0` (`1d33445`). Do not open `host2` from this branch. Do not build runtime images. Do not `docker run` attach. Do not mux client. Do not pack *builds* (ty/RA musl jobs). Tests never talk to a Docker daemon, registry, or AWS. No real ty/clangd download (`FakeEngineAdapter` / fixture bytes / `RecordingSpawnPort` only). `RunLog` stays a separate schema from the serve WAL.

**Scope:** `PackAdapter` Linux `Command` spawn; stub bytes still refuse exec; Darwin / non-Linux still refuse (`EngineError::Spawn`, reserved / not this OS); production spawn uses `ChildIo::lsp_with_stderr_pipe`; `SpawnPlan` value object so Darwin tests cover the Linux plan without `Command`; `RecordingSpawnPort` is would-have-spawned. Serve already holds the supervisor and `try_spawn`s (LOG-6) — not HOST-1.x. Not runtime image. Not attach. Not mux. Not pack builds.

**Exit**

- [x] After stub check, Linux `std::process::Command` pipes stdin/stdout for LSP and stderr for `ChildStderrAdapter` (`ChildIo::lsp_with_stderr_pipe`). Never `NullStderrAdapter`. Never a log Adapter on stdout.
- [x] Stub bytes still refuse exec (unchanged message). Hash mismatch still no spawn.
- [x] Darwin / non-Linux still refuse exec of non-stub real bytes (`EngineError::Spawn`, reserved / not this OS). This laptop does not exec musl ELFs or host clangd.
- [x] `SpawnPlan` (argv, cwd, env, `ChildIo`) is a value object. Darwin unit tests name the pattern and cover the Linux plan without `Command`.
- [x] LOG-10 attach-when-Read works when a real stderr pipe exists on Linux (`ChildHandle::has_os_stderr`); tests still use `FakeChildStderr`.
- [x] Supervisor still degrades T1/T2 if spawn fails.
- [x] Docs: PackAdapter row (Command on Linux; refuse stub + Darwin); branching `host0 └── host1`; `host2`–`host7` still future.

**Sign-off checklist (HOST-1)**

- [x] Exit criteria met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (engine lib 41; composition-root lib 73)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.99%** lines
- [x] 80% mutants on listed crates that changed — engine in-diff **26 caught / 26 scored (100%)**, 13 unviable
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `SpawnPlan`, `SpawnPort`, `CommandSpawnPort`, `RecordingSpawnPort`; PackAdapter / `ChildIo` updated

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the gate on macOS.
- Tests never talk to a Docker daemon. No real ty/clangd download. `RecordingSpawnPort` is not a musl green.
- No musl ELF change. Do not run `check-static` on a Darwin Mach-O and call it green.

## HOST-2 — musl core ELF extract both triples

**Status: SIGNED OFF** on branch `host2`. Parent is `host1` (`b9490b6`). Do not open `host3` from this branch. Do not build engine pack binaries. Do not build the runtime image. Do not `docker run` attach. Do not mux client. Crate tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). No real ty/clangd download. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Real musl **core** ELFs for `x86_64-unknown-linux-musl` (`linux/amd64`) and `aarch64-unknown-linux-musl` (`linux/arm64`) via `xtask musl`; `docker build --output` extract to `target/musl/<triple>/progressive-lsp`; `check-static` after extract; `MuslBuildPlan` + `DockerPort` / `RecordingDockerPort`. Not engine packs. Not runtime image. Not attach. Not mux. Allocator-matrix placeholders stay mimalloc.

**Exit**

- [x] `xtask musl` extracts a named `progressive-lsp` ELF under `target/musl/<triple>/` for both triples (or records an honest Darwin vs CI gap if the daemon / qemu triple fails).
- [x] After a successful extract, `xtask check-static` on that ELF (no `PT_INTERP`, no `DT_NEEDED`, rusqlite must not pull `libdl`). Mach-O still refused.
- [x] `MuslBuildPlan` is a value object (triple, docker platform, dockerfile, dest, `RUST_TARGET`). Darwin unit tests name the pattern and cover the plan without docker.
- [x] `RecordingDockerPort` is would-have-built (no daemon). Production is `CommandDockerPort`. Tests never start docker.
- [x] `docker/rust-musl.Dockerfile` still builds only `--bin progressive-lsp`. PR CI must not compile LLVM. Do not commit musl ELFs.
- [x] Docs: branching `host1 └── host2`; `host3`–`host7` still future. host-deps / testing dest convention.

**Sign-off checklist (HOST-2)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (37 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only)
- [x] No `sleep`
- [x] `check-static` — crate tests use fixture ELFs (including `libdl` fail-closed / Mach-O refuse). Real dest ELFs: **PASS** `aarch64-unknown-linux-musl` and **PASS** `x86_64-unknown-linux-musl` (`target/musl/<triple>/progressive-lsp`, `cargo xtask musl --both` on this Darwin host with Docker Desktop). Darwin: do not fake musl greens; these are extracted musl ELFs, not Mach-O.
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `MuslBuildPlan`, `DockerPort`, `CommandDockerPort`, `RecordingDockerPort`

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inject `RecordingDockerPort` and never start a Docker daemon.
- HOST-2 proof on this Darwin host (Docker Desktop 29.2.0): `cargo xtask musl --both` extracted both core ELFs and `check-static` **PASS**ed:
  - `target/musl/aarch64-unknown-linux-musl/progressive-lsp` — ELF 64-bit LSB executable, ARM aarch64, statically linked (~22 MiB)
  - `target/musl/x86_64-unknown-linux-musl/progressive-lsp` — ELF 64-bit LSB pie executable, x86-64, static-pie linked (~22 MiB)
  That is **not** a cargo test. qemu/`linux/amd64` succeeded here; a missing daemon on another Darwin machine is still a recorded gap, not a Mach-O green.
- Extract dest is gitignored under `target/`. Do not commit musl ELFs.
- Engine pack builds remain HOST-3. Runtime image remains HOST-4.

## HOST-3 — slim pack jobs both triples

**Status: SIGNED OFF** on branch `host3`. Parent is `host2` (`7e34b2c`). Do not open `host4` from this branch. Do not build clangd/tsgo/gopls/zls. Do not build the runtime image. Do not `docker run` attach. Do not mux client. Crate tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). No real ty/clangd download in tests. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Slim pack musl jobs for `python`/`ty`, `rust`/`rust-analyzer`, `phpantom`/`phpantom`, `biome`/`biome`, `superhtml`/`superhtml` on both triples via `xtask pack`; pinned upstream git SHAs in `xtask/pack-pins.toml`; `docker build --output` extract to `target/musl/<triple>/engines/<pack>/<binary>`; `check-static` after extract; `PackBuildPlan` + existing `DockerPort` / `RecordingDockerPort`. Not heavy packs. Not runtime image. Not attach. Not mux. Allocator-matrix placeholders stay mimalloc. Darwin `xtask dist` stubs are not musl greens.

**Exit**

- [x] `xtask pack` extracts named slim pack ELFs under `target/musl/<triple>/engines/<pack>/<binary>` for both triples (or records an honest Darwin vs CI gap if the daemon / qemu triple fails).
- [x] After a successful extract, `xtask check-static` on that ELF (no `PT_INTERP`, no `DT_NEEDED`). Mach-O and Darwin dist stubs still refused.
- [x] `PackBuildPlan` is a value object (pack, binary, triple, platform, dockerfile, dest, pinned SHA). Darwin unit tests name the pattern and cover the plan without docker.
- [x] Pins are 40-hex git SHAs in `xtask/pack-pins.toml` (not core crate semver, not `latest`).
- [x] `RecordingDockerPort` is would-have-built (no daemon). Production is `CommandDockerPort` / `DockerPort::extract`. Tests never start docker.
- [x] `docker/engine-pack.Dockerfile` is a real hermetic Rust pack job (not `cat /pack-id.txt`). Heavy packs fail closed. Do not commit musl ELFs.
- [x] Docs: branching `host2 └── host3`; `host4`–`host7` still future. host-deps / testing dest convention.

**Sign-off checklist (HOST-3)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (47 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only)
- [x] No `sleep`
- [x] `check-static` — crate tests use fixture ELFs. Live dest ELFs: **9/10 PASS** (table below). `superhtml` × `x86_64-unknown-linux-musl` is an honest qemu/Zig gap, not a Mach-O green.
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `PackPin`, `PackKind`, `PackBuildPlan`, `RustToolchainPin`, `ZigToolchainPin`; `DockerPort` extract

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inject `RecordingDockerPort` and never start a Docker daemon.
- HOST-3 proof is extracted musl ELFs, not Darwin `xtask dist` stubs. Dest is gitignored under `target/`.
- First live extract attempt (phpantom aarch64 on `rust:1.87-alpine`) failed: mago crates require rustc ≥ 1.97. Switched to `rust:1.98.0-bookworm` + musl *target* (`1.99.0` is unpublished; alpine musl-host rustup 1.98+ 404s).
- `x86_64-unknown-linux-musl` defaults to dynamic `ld-musl` (`PT_INTERP`). Pack Dockerfile now forces `+crt-static` / `link-self-contained` / `-static` via target-specific `RUSTFLAGS` (plain `RUSTFLAGS` was dropped by upstream `.cargo/config`).
- Live `check-static` on this Darwin host (Docker Desktop, native `linux/arm64` then qemu `linux/amd64`):

  | pack | binary | aarch64 | x86_64 |
  |---|---|---|---|
  | python | ty | PASS (~33 MiB static) | PASS (~45 MiB static) |
  | rust | rust-analyzer | PASS (~50 MiB static) | PASS (~57 MiB static) |
  | phpantom | phpantom | PASS (~46 MiB static) | PASS (~46 MiB static) |
  | biome | biome | PASS (~125 MiB static) | PASS (~118 MiB static) |
  | superhtml | superhtml | PASS (~7.9 MiB static) | **MISS** — qemu/Zig `access()` `Unexpected` on `.zig-cache` options files (`-j1` + `/tmp` cache still fails). Job/pin remain. Native linux/amd64 CI can close this; do not call it green here. |

- Runtime image remains HOST-4. Heavy packs remain HOST-7.

## HOST-4 — runtime image copy of core and slim packs

**Status: SIGNED OFF** on branch `host4`. Parent is `host3` (`0d24582`). Do not open `host5` from this branch. Do not `docker run` attach. Do not mux client. Do not build clangd/tsgo/gopls/zls. Do not compile cargo/LLVM/clang/zig/go inside the runtime image. Crate tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). No real ty/clangd download in tests. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Runtime image `progressive-lsp-runtime:local` via `xtask runtime-image`; copy prebuilt HOST-2 core + HOST-3 slim packs into `/opt/plsp` (`bin/progressive-lsp`, `engines/<pack>/<binary>`, empty `cache`/`log`/`run`/`scripts`, empty `/tmp`); `FROM scratch` Dockerfile that `COPY`s only; `RuntimeImagePlan` + existing `DockerPort` / `RecordingDockerPort` (`tag_image`). Not attach. Not mux. Not heavy packs. Allocator-matrix placeholders stay mimalloc.

**Exit**

- [x] `xtask runtime-image` stages prebuilt ELFs and tags `progressive-lsp-runtime:local` for both platforms (or records an honest Darwin vs CI gap if the daemon / qemu triple fails).
- [x] Image layout matches `PrefixLayout` under `/opt/plsp`. ENTRYPOINT is the core ELF; default args are `serve --prefix /opt/plsp` (host5 attach still unwired).
- [x] `RuntimeImagePlan` is a value object (platform, triple, dockerfile, core dest, pack dests, image tag). Darwin unit tests name the pattern and cover the plan without docker.
- [x] `docker/runtime.Dockerfile` is `FROM scratch` and `COPY`s only. No rustc/cargo/clang/LLVM/zig/go. Missing required core ELF fail closed. `superhtml` × x86_64 may be omitted (HOST-3 miss).
- [x] `RecordingDockerPort` is would-have-tagged (no daemon). Production is `CommandDockerPort` / `DockerPort::tag_image`. Tests never start docker.
- [x] Docs: branching `host3 └── host4`; `host5`–`host7` still future. host-deps: runtime image is our artifact; docker CLI remains a host tool; tests still FakeRuntime.

**Sign-off checklist (HOST-4)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (60 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only)
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELFs unchanged; image copies HOST-2/HOST-3 dests). Do not run `check-static` on a Darwin Mach-O
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `RuntimeImagePlan`, `PackImageCopy`; `DockerPort` `tag_image`

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inject `RecordingDockerPort` and never start a Docker daemon.
- HOST-4 proof is a tagged local image, not a cargo test. Staging dest is gitignored under `target/runtime-image/`. Do not commit musl ELFs.
- Live `cargo xtask runtime-image` on this Darwin host (Docker Desktop 29.2.0):

  | platform | triple | tag | image id | notes |
  |---|---|---|---|---|
  | `linux/arm64` | `aarch64-unknown-linux-musl` | `progressive-lsp-runtime:local` | `sha256:e017bf95856798a35767438f560ba1feb67082cbbab551adedc67d1373eb04b8` | native; ~85 MiB; ENTRYPOINT `/opt/plsp/bin/progressive-lsp`; CMD `serve --prefix /opt/plsp`; core + five slim packs |
  | `linux/amd64` | `x86_64-unknown-linux-musl` | built then retagged off `:local` | `sha256:42a7314349631cf0fb04be66deed203bc281389c75a462d73af603fb77699ac1` | ~86 MiB; same ENTRYPOINT/CMD; `superhtml` omitted (HOST-3 qemu/Zig miss). `:local` left on native arm64 |

- `DockerRuntime.start` remains unwired. Heavy packs remain HOST-7.

## HOST-5 — docker run stdio attach

**Status: SIGNED OFF** on branch `host5`. Parent is `host4` (`67ca485`). Do not open `host6` from this branch. Do not mux client / `serve --mux`. Do not build clangd/tsgo/gopls/zls. Do not rebuild the runtime image. Container LSP is **stdio only** (`ServeMode::StockStdio`). Unix sockets through Docker Desktop are forbidden. Tests never talk to a Docker daemon, registry, or AWS (`FakeRuntime` / scripted CLI / `DockerRunPlan` argv only). No real ty/clangd download. `RunLog` stays a separate schema from the serve WAL.

**Scope:** `DockerRunPlan` + `DockerRuntime::start` validates the plan (does not exec); poc-ide `LspIoAttach::Container` → `StdioLsp::from_command` is the single `docker run -i --rm`; bind-mount `$WS:$WS`; working directory `$WS`; image `progressive-lsp-runtime:local`; `serve --prefix /opt/plsp`. No local Darwin serve on container open. Not mux. Not full packs. HOST-5 exit is attach + initialize over stdio, not a full T3 hover green.

**Exit**

- [x] `DockerRunPlan` is a value object (docker binary, `-i --rm`, `-v WS:WS`, `-w WS`, image, `serve --prefix /opt/plsp`). Darwin unit tests name the pattern and cover the plan without exec. Never `-t`.
- [x] `DockerRuntime::start` validates the plan and returns `RuntimeSession`. It does not `docker run`. Empty workspace / missing absolute docker binary fail closed.
- [x] Container open attaches `StdioLsp` to that docker Command + `StockStdio`. Native open still uses `SpawnSpec` + `ControlSocket`. Never two serves.
- [x] FakeRuntime still drives the journal. Scripted CLI covers probe/inspect. `DockerRunPlan` tests do not start a daemon. No `thread::sleep`.
- [x] Docs: branching `host4 └── host5`; `host6`–`host7` still future. host-deps: attach is plan → stdio; docker CLI remains a host tool; tests still FakeRuntime.

**Sign-off checklist (HOST-5)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p poc-ide --lib -- --test-threads=1` (216 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **96.50%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff vs `67ca485` **19 caught / 23 scored (82.6%)**, 1 unviable, 4 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `DockerRunPlan`, `LspIoAttach`

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inspect `DockerRunPlan` argv and inject `FakeRuntime` / a scripted CLI; they never start a Docker daemon.
- HOST-5 proof is attach + initialize over stdio, not a cargo test and not a T3 hover. First live T3 on mounted source is later (Python/ty or PHP/phpantom).
- Live `docker run -i --rm` initialize on this Darwin host: **attempted and succeeded**. Image `progressive-lsp-runtime:local` (`sha256:e017bf95856798a35767438f560ba1feb67082cbbab551adedc67d1373eb04b8`, linux/arm64, ENTRYPOINT `/opt/plsp/bin/progressive-lsp`, CMD `serve --prefix /opt/plsp`). One-shot `docker run -i --rm -v $WS:$WS -w $WS` (no `-t`) answered `initialize` with `serverInfo.name=progressive-lsp` and `experimental.progressiveLsp` `{version:v1, socket:null, mux:false}`. Not a cargo test. Not a T3 hover.
- Heavy packs remain HOST-7. Mux remains HOST-6.

## HOST-6 — poc-ide mux client on container stdio

**Status: SIGNED OFF** on branch `host6`. Parent is `host5` (`b46ad0f`). Do not open `host7` from this branch. Do not build clangd/tsgo/gopls/zls. Do not rebuild the runtime image. Container LSP + control share one stdio (`ServeMode::Mux`, `serve --prefix /opt/plsp --mux`). Native Darwin open keeps `ServeMode::ControlSocket` (real Unix socket on the Mac is fine). Unix sockets through Docker Desktop remain forbidden. Tests never talk to a Docker daemon, registry, or AWS (`FakeRuntime` / scripted CLI / `DockerRunPlan` argv / pair mux frames). No real ty/clangd download. `RunLog` stays a separate schema from the serve WAL.

**Scope:** `ServeMode::Mux` + `MuxStdio` Adapter reuse protocol `MuxFrame` (`u8 channel | u32be length | payload`, 16 MiB cap). Channel 0 = opaque JSON-RPC body matching `LspFacade::serve_mux`. Channel 1 = the same length-prefixed Envelope as `ControlClient` on a Unix socket. `DockerRunPlan` includes `--mux`. `advertised_control` returns `ControlAttach::Mux` when mux is selected; `pending_mux` only when mux is advertised but not selected. `experimental.progressiveLsp.mux` true and `socket` null on container mux. One docker process. Not full packs.

**Exit**

- [x] `ServeMode::Mux` argv is `serve --mux`. Container `DockerRunPlan` is `serve --prefix /opt/plsp --mux`. Darwin unit tests name the pattern. Never `-t`. Never a second serve.
- [x] `MuxStdio` / `MuxLsp` / `MuxControl` encode/decode protocol `MuxFrame`. Unknown channel and payload > 16 MiB fail closed. Pair / Cursor tests. No `thread::sleep`. No docker daemon.
- [x] `advertised_control` + `ControlAttach::Mux` replace `pending_mux` on the selected mux path. Native `ControlSocket` still uses `advertised_control_socket`.
- [x] Docs: branching `host5 └── host6`; `host7` still future. host-deps: container attach is mux stdio; docker CLI remains a host tool; tests still FakeRuntime.

**Sign-off checklist (HOST-6)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p poc-ide --lib -- --test-threads=1` (225 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.95%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff vs `b46ad0f` **31 caught / 31 scored (100%)**, 42 unviable, 0 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `ServeMode::Mux`, `MuxStdio`, `MuxLsp`, `MuxControl`, `ControlAttach`

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inspect `DockerRunPlan` argv and inject `FakeRuntime` / a scripted CLI / in-memory mux frames; they never start a Docker daemon.
- HOST-6 proof is mux initialize (`mux: true`, `socket: null`), not a cargo test and not a T3 hover. First live T3 on mounted source is later (Python/ty or PHP/phpantom).
- Live `docker run -i --rm … serve --prefix /opt/plsp --mux` initialize: **attempted and succeeded**. Image `progressive-lsp-runtime:local` (`sha256:e017bf95856798a35767438f560ba1feb67082cbbab551adedc67d1373eb04b8`, linux/arm64, ENTRYPOINT `/opt/plsp/bin/progressive-lsp`). One-shot `docker run -i --rm -v $WS:$WS -w $WS` (no `-t`) with `serve --prefix /opt/plsp --mux` answered a channel-0 mux-framed `initialize` with `serverInfo.name=progressive-lsp` and `experimental.progressiveLsp` `{version:v1, socket:null, mux:true}`. Not a cargo test. Not a T3 hover.
- Full packs are HOST-7.

## HOST-7 — full flavor musl pack jobs

**Status: SIGNED OFF** on branch `host7`. Parent is `host6` (`d6f8fa8`). This is the last host slice — do not open `host8`. Do not rewire `DockerRuntime.start` except to copy extra packs into the runtime image when present. Container LSP + control stay mux stdio. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). No real LLVM download in tests. `RunLog` stays a separate schema from the serve WAL.

**Scope:** Full flavor packs on both musl triples: `clangd` (cache COPY; miss documented; never cmake in the default job), `tsgo` / `gopls` (`CGO_ENABLED=0` Go static), `zls` (Zig musl, same dockerfile as superhtml). Pins are 40-hex SHAs in `xtask/pack-pins.toml`. `--pack full` / `--pack clangd,tsgo,gopls,zls` allowed; slim default still works; unknown packs fail closed. Dest `target/musl/<triple>/engines/<pack>/<binary>` then `check-static`. `PackBuildPlan` Darwin unit tests cover heavy plans without docker. `xtask runtime-image` copies full packs when present (optional). Allocator-matrix placeholders stay mimalloc (skip matrix: gopls, tsgo, zls). Not a follow-on host8.

**Exit**

- [x] `xtask pack --pack full` (or named CSV) extracts / records per pack×triple. Slim still the default. Unknown packs fail closed.
- [x] After a successful extract, `xtask check-static` on that ELF (no `PT_INTERP`, no `DT_NEEDED`). clangd missing cache is a documented miss — do not ship dynamic. Mach-O and Darwin dist stubs still refused.
- [x] `PackBuildPlan` covers heavy kinds (`go`, `cached`, `cmake` cache-fill) without docker. Darwin unit tests name the pattern.
- [x] Pins are 40-hex git SHAs (gopls, tsgo, zls, llvm-project). Not `latest`.
- [x] Go packs: `CGO_ENABLED=0`. Zig toolchain only inside the pack build container. clangd default job never cmake; `--cache-fill` is dedicated and not PR CI.
- [x] Docs: branching `host6 └── host7`; no host8. host-deps: full packs via Docker/cache; PR CI must not compile LLVM.

**Sign-off checklist (HOST-7)**

- [x] Exit criteria met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (68 passed)
- [x] 95% llvm-cov on crates that exist (same ignores) — **95.95%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only; discovery.rs unchanged)
- [x] No `sleep`
- [x] `check-static` — crate tests use fixture ELFs. Live dest table below. Do not commit musl ELFs.
- [x] Docs in this tree updated (`RunLog` stays a separate schema)
- [x] [design-patterns.md](design-patterns.md) — `PackKind` (`go`/`cached`/`cmake`), `GoToolchainPin`, `PackBuildPlan` heavy plans, `PackImageCopy` optional full

**Darwin / CI notes**

- Native `cargo test -- --test-threads=1` is the unit gate on macOS. Tests inject `RecordingDockerPort` and never start a Docker daemon.
- HOST-7 proof is extracted musl ELFs (or honest misses), not Darwin `xtask dist` stubs. Dest is gitignored under `target/`. Do not commit musl ELFs.
- clangd is content-addressed by llvm-project SHA + triple. Cache hit COPY; miss is an honest gap. `--cache-fill` cmake is not the default pack job and must not run on every PR.
- Live `check-static` on this Darwin host (Docker Desktop 29.2.0):

  | pack | binary | aarch64 | x86_64 |
  |---|---|---|---|
  | gopls | gopls | PASS (~27 MiB static) | PASS (~29 MiB static) |
  | zls | zls | PASS (~17 MiB static) | **MISS** — qemu/Zig `access()` `Unexpected` on `.zig-cache` options files (same class as HOST-3 superhtml). Job/pin remain. Native linux/amd64 CI can close this; not a Mach-O green. |
  | tsgo | tsgo | PASS (~25 MiB static) | PASS (~28 MiB static) |
  | clangd | clangd | **MISS** — cache key `3623fe661ae35c6c80ac221f14d85be76aa870f1:aarch64-unknown-linux-musl` absent; not cmake | **MISS** — cache key `3623fe661ae35c6c80ac221f14d85be76aa870f1:x86_64-unknown-linux-musl` absent; not cmake |

## Later post-v1 (not in PD0–PD4 / IDE-0–IDE-5 / LOG-0–LOG-11)

Java in-house types (still no JVM). Dual-run PHP T3 if the other spike wins. oxc_type_checker as TS T3. Native macOS/Windows **server** hosts. WASM plugin ABI. HTTP/S3 transport in-tree. Buck2 if engine builds outgrow Docker cache. Watchman. `$/` JSON mirror of `progressive.v1` only if a real client cannot open a socket or mux. Read-only query of server logs from poc-ide (optional; do not merge schemas).
