# Implementation plan

Work packages for agents. **Do not start a WP until its Depends-on milestone/WP is signed off.** Hygiene: [testing.md](testing.md). Exits: [milestones.md](milestones.md). Branches: [branching.md](branching.md).

## Stacked branches

```text
main
  └── docs-0
        └── m0
              └── m1
                    └── m2
                          └── m3
                                └── m4
                                      └── m5
                                            └── m6

main   # after v1 merge
  └── pd0
        └── pd1
              └── pd2
                    └── pd3
                          └── pd4

main   # after PD4 merge
  └── ide0
        └── ide1
              └── ide2
                    └── ide3
                          └── ide4
                                └── ide5

main   # after IDE-5 merge
  └── poc-log   # per-run sqlite debug log (not IDE-6)
        └── poc-tree-lazy   # shallow FileTree load (not IDE-6)
              └── poc-tree-collapsed   # TreeExpansion default collapsed (not IDE-6)
                    └── poc-compact-folders   # compact a/b/c single-child dirs (not IDE-6)
                          └── poc-context-menu   # editor context menu for resolver actions (not IDE-6)
                                └── poc-navigate   # deferred Navigate + editor caret sync (not IDE-6)
                                      └── poc-no-console   # drop the hand-typed protocol console; debug is RunLog (not IDE-6)
                                            └── poc-dialog-defer   # File Open Folder/File after the menu closes (not IDE-6)
                                                  └── poc-open-unblock   # non-recursive watch + deferred LSP initialize (not IDE-6)
                                                        └── poc-tree-sort   # dirs then files; dot names last (not IDE-6)
                                                              └── poc-discover-log   # discover uri/position/count in RunLog (not IDE-6)

main   # after poc-discover-log merge (current main)
  └── log0   # global logging docs (not a crate)
        └── log1   # LogPort + records (no sqlite in server)
              └── log2   # WAL repository crate
                    └── log3   # Facades, bridges, eprintln death
                          └── log4   # Wire serve/install + docs lock
```

A branch’s scope is that milestone’s WPs only. No “while we’re here” language packs on `m1`. Tests for the milestone are written **on that branch**.

## Sign-off checklist (copy onto every WP)

- [ ] Exit criteria for this WP met
- [ ] Tests on this branch
- [ ] 95% llvm-cov on crates that exist
- [ ] 80% mutants on listed crates that exist
- [ ] No `sleep`
- [ ] `check-static` if ELF changed
- [ ] [design-patterns.md](design-patterns.md) table updated if types added
- [ ] Docs in this tree updated if a locked decision was refined (do not contradict [requirements.md](requirements.md) without an explicit change)

## Docs-0

**Status: SIGNED OFF** on branch `docs-0`. M0.1 may start on `m0` after this WP; do not open `m0` from this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| D0 | Write `docs/` set (this tree) | — | **SIGNED OFF.** No crates; tests / 95% llvm-cov / 80% mutants / `sleep` / `check-static` are **N/A**. |

**Sign-off checklist (D0)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — **N/A** (no crates)
- [x] 95% llvm-cov on crates that exist — **N/A** (none)
- [x] 80% mutants on listed crates that exist — **N/A** (none)
- [x] No `sleep` — **N/A** (no tests)
- [x] `check-static` if ELF changed — **N/A** (no ELFs)
- [x] [design-patterns.md](design-patterns.md) table names every type in [detailed-design.md](detailed-design.md)
- [x] Docs in this tree updated if a locked decision was refined

## M0 (`m0` branch)

**Status: SIGNED OFF.** Do not open `m1` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M0.1 | Cargo workspace, `rust-toolchain.toml`, tiny bin `main` | D0 | **SIGNED OFF.** Composition root only |
| M0.2 | `progressive-lsp-core`: ids, errors, `ClockPort`, prefix | M0.1 | **SIGNED OFF.** FakeClock tests |
| M0.3 | `PluginRegistry` + empty `LanguageFactory` slots | M0.2 | **SIGNED OFF.** `UnsupportedLanguage` tests |
| M0.4 | `progressive-lsp-protocol`: initialize/shutdown | M0.3 | **SIGNED OFF.** experimental cap, socket null OK |
| M0.5 | proto + `progressive-lsp-control` codec | M0.2 | **SIGNED OFF.** round-trip; empty RPCs OK |
| M0.6 | `.progressivelsp` layout + git exclude helper | M0.2 | **SIGNED OFF.** fixture repo; never edit project `.gitignore` |
| M0.7 | `progressive-lsp-install`: LocalFs, hash, manifest schema | M0.2 | **SIGNED OFF.** no network |
| M0.8 | `xtask musl`, `check-static`, Docker stub | M0.1 | **SIGNED OFF.** both arches via Docker; fixture ELF tests on Darwin |
| M0.9 | `xtask bench-alloc` + `allocator-matrix.toml` placeholders | M0.8 | **SIGNED OFF.** mimalloc until CI rows |
| M0.10 | Spike notes under `spike/` (glibc-static, csharp-ls, PHP T3, …) | M0.8 | **SIGNED OFF.** notes; fail closed, do not ship `DT_NEEDED` |

## M1 (`m1` branch)

**Status: SIGNED OFF.** Do not open `m2` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M1.1 | `WatchCoalescer` + FakeWatcher | M0 signed | **SIGNED OFF.** 10k → 1 batch |
| M1.2 | FilesSince + overflow/`truncated` | M1.1, M0.5 | **SIGNED OFF.** control plane |
| M1.3 | Incremental Tree-sitter + dirty-set priority | M1.1 | **SIGNED OFF.** ~10 ms class |
| M1.4 | `WatchFilter` identity | M1.1 | **SIGNED OFF.** |
| M1.5 | `progressive-lsp-lang-java` T1 + tokens | M1.3 | **SIGNED OFF.** no JDK |
| M1.6 | Directory + Maven/Gradle/Eclipse adapters | M1.5 | **SIGNED OFF.** multi-package fixture |
| M1.7 | Server-side `notify` ghost edit reindex | M1.1, M1.5 | **SIGNED OFF.** no progressive client required |

## M2 (`m2` branch)

**Status: SIGNED OFF.** Do not open `m3` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M2.1 | Package-stream ingest + `workDoneProgress` + `data.tier` | M1 | **SIGNED OFF.** ingest never blocks didChange |
| M2.2 | Control `TierReady` | M2.1, M0.5 | **SIGNED OFF.** push when progressive connected |
| M2.3 | Java T2 heuristics; optional TSG eval | M2.1, M1.5 | **SIGNED OFF.** TSG dropped; `StackGraphResolver` slot unused |
| M2.4 | PHP T1/T2 + Composer adapter | M2.1 | **SIGNED OFF.** no interpreter |
| M2.5 | HTML/CSS/JS T1 | M2.1 | **SIGNED OFF.** split crates |
| M2.6 | Go T1 + `go.mod`; Zig T1 + `build.zig` | M2.1 | **SIGNED OFF.** no gopls/zls |
| M2.7 | Rhai `ScriptHost` + catalog subset | M0.2 | **SIGNED OFF.** sandbox + Abort tests |

## M3 (`m3` branch)

**Status: SIGNED OFF.** Do not open `m4` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M3.1 | `EngineSupervisor` + `EngineAdapter` trait | M2 | **SIGNED OFF.** crash/backoff tests; FakeClock; no sleep |
| M3.2 | Pack discovery `$PREFIX/engines/` | M3.1, M0.6 | **SIGNED OFF.** missing pack / bad hash → no spawn |
| M3.3 | ty pack + Python T3 handoff | M3.1 | **SIGNED OFF.** T1 without pack; Fake ty for T3; no CPython/pylsp/pyright |
| M3.4 | rust-analyzer pack + Rust T3 | M3.1 | **SIGNED OFF.** no pack / no sysroot → T1 (no dedicated Rust T2) |
| M3.5 | `on_engine_spawn` / `on_tier_ready` | M2.7, M3.1 | **SIGNED OFF.** Abort spawn skips engine; on_tier_ready cannot Abort intelligence |

## M4 (`m4` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M4.1 | clangd pack + compile_commands adapter | M3.1 | **SIGNED OFF.** slim dist default excludes clangd |
| M4.2 | csharp-ls AOT or matrix T2 ceiling | M3.1 | **SIGNED OFF.** T1/T2 ceiling; no csharp-ls pack |
| M4.3 | oxc T2 + tsgo T3 | M3.1 | **SIGNED OFF.** heuristic T2; Fake tsgo T3; no Node |
| M4.4 | PHP T3 pack (spike winner) | M3.1, M2.4 | **SIGNED OFF.** PHPantom winner |
| M4.5 | superhtml + biome | M3.1 | **SIGNED OFF.** adapter + T1 fallback |
| M4.6 | gopls + zls | M3.1, M2.6 | **SIGNED OFF.** T3 when pack+project; else T2/T1 |

## M5 (`m5` branch)

**Status: SIGNED OFF.** Do not open `m6` until this table stays signed off. No dist tarballs, conformance dashboard, or `on_install_verify` productization on this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M5.1 | Content-addressed index cache | M2.1 | **SIGNED OFF.** `$PREFIX/cache/`; tests inject prefix |
| M5.2 | LATEST+2 fixtures + mixed workspace | M4 languages as landed | **SIGNED OFF.** [language-matrix.md](language-matrix.md) 2026-08 window |
| M5.3 | Burst + FilesSince overflow catch-up | M1.2 | **SIGNED OFF.** 10k FakeWatcher; truncated tested |
| M5.4 | Lag fixtures (newer syntax, no panic) | M5.2 | **SIGNED OFF.** Java/PHP/JS/Python/Rust/C |
| M5.5 | RSS / p99 gates recorded | M5.1 | **SIGNED OFF.** Darwin samples; T3 not charged to core. Mutants: index 82.7%, watch 98.0%, core 88.0% |

## M6 (`m6` branch)

**Status: SIGNED OFF.** v1 complete (merged to `main`). Next stack is PD0–PD4, not M7.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M6.1 | `xtask dist` tarballs + SHA256 + slim/full | M0.8, M4 packs | **SIGNED OFF.** Per-triple musl tarballs + SHA256 + `manifest.json`. Darwin writes stub payloads; Linux CI is the real musl dist. Slim/full as M4. Dist only reads `xtask/allocator-matrix.toml`. |
| M6.2 | Install CLI + `on_install_verify` | M0.7, M2.7 | **SIGNED OFF.** Verified prefix (hash + atomic replace). `FakeRemoteTransport` (ssh-like put/chmod/rename/hash; no SSH types). Abort refuses the new binary. |
| M6.3 | Refresh control/lsp/plugin/consumer docs vs impl | M6.1 | **SIGNED OFF.** docs remain source of truth |
| M6.4 | Conformance dashboard | M5.2 | **SIGNED OFF.** [conformance.md](conformance.md); C# T1/T2 only; Java no T3; T3 0% on Darwin stubs |
| M6.5 | Versioning: core semver vs engine SHAs | M6.1 | **SIGNED OFF.** Workspace **0.1.0** (first published v1; not 1.0.0 — native macOS/Windows hosts are post-v1). Proto `progressive.v1`. Engine SHAs in pack manifests only. Hygiene: llvm-cov **96.26%** lines (ignore xtask/main/tree-sitter). Mutants on install+script+control+protocol: **333 caught / 392 scored (84.9%)**, 30 unviable, 4 timeouts. |

## PD0 (`pd0` branch)

**Status: SIGNED OFF** on `pd0`. Parent is `main`. PD1 may start after this WP.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PD0.1 | Ingest user docs, integration designs, T2 spike | M6 signed off / `main` | **SIGNED OFF.** Docs only. |

## PD1 (`pd1` branch)

**Status: SIGNED OFF.** Do not open `pd2` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PD1.1 | Integration harness (LSP stdio client) | PD0 | **SIGNED OFF.** `integration/harness` (`plsp-it1 handshake`); not a workspace member |
| PD1.2 | Distro compose: Arch, Rocky/UBI, Debian, Ubuntu | PD1.1 | **SIGNED OFF.** `integration/compose.yaml`; prebuilt musl ELF from CI; Darwin gap documented |
| PD1.3 | IT-1.1–1.7 deploy/config cases | PD1.2 | **SIGNED OFF.** [integration/01-deploy-config.md](../integration/01-deploy-config.md). Linux CI is the distro gate; Darwin host_smoke is not IT-1.1 |

## PD2 (`pd2` branch)

**Status: SIGNED OFF.** Do not open `pd3` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PD2.1 | Corpora fetch-at-SHA + expected goldens | PD1 | **SIGNED OFF.** `integration/corpora/pins.json` + `plsp-it1 fetch`; no submodule mirrors |
| PD2.2 | IT-2 per-language stock LSP | PD2.1 | **SIGNED OFF.** [integration/02-lsp-backends.md](../integration/02-lsp-backends.md). T3 stubs → `skip_pack_missing` |

## PD3 (`pd3` branch)

**Status: SIGNED OFF.** Do not open `pd4` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PD3.1 | Control Envelope if missing | PD2 | **SIGNED OFF.** Public `Envelope` dispatch (`method` + `request_id` + `body`) |
| PD3.2 | IT-3 Java / Python / TS progressive | PD3.1 | **SIGNED OFF.** [integration/03-extended-protocol.md](../integration/03-extended-protocol.md). Mux `pending_mux`. T3 stubs → `skip_pack_missing` |

## PD4 (`pd4` branch)

**Status: SIGNED OFF.** Post-dev stack complete. There is no PD5.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PD4.1 | T2 Strategy config pick; default heuristic | PD3 | **SIGNED OFF.** `[t2] java = "heuristic"` default; tests inject fake T2 |
| PD4.2 | Pin stack-graphs git SHA; bake-off table | PD4.1 | **SIGNED OFF.** Pin `fcb7705`; winner rule did not fire; [t2-bakeoff-results.md](spikes/t2-bakeoff-results.md) |

## IDE-0 (`ide0` branch)

**Status: SIGNED OFF** on `ide0`. Parent is `main`. IDE-1 may start after this WP; do not open `ide1` from this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-0.1 | POC IDE docs, OSS pins, pattern rows, stack | PD4 / `main` | **SIGNED OFF.** Docs only. No `poc-ide` crate. Tests / 95% llvm-cov / 80% mutants / `sleep` / `check-static` are **N/A**. |

**Sign-off checklist (IDE-0.1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — **N/A** (no crates)
- [x] 95% llvm-cov on crates that exist — **N/A** (none added)
- [x] 80% mutants on listed crates that exist — **N/A** (none added)
- [x] No `sleep` — **N/A** (no tests)
- [x] `check-static` if ELF changed — **N/A** (no ELFs)
- [x] [design-patterns.md](design-patterns.md) names every POC type in [poc-ide/architecture.md](poc-ide/architecture.md)
- [x] Docs in this tree updated if a locked decision was refined

## IDE-1 (`ide1` branch)

**Status: SIGNED OFF** on `ide1`. Do not open `ide2` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-1.1 | `poc-ide` crate skeleton, composition-root bin | IDE-0 | **SIGNED OFF.** lib + `main.rs`; not musl. Pins: eframe/egui/egui_extras **0.36.1**, rfd **0.15.4**. |
| IDE-1.2 | `DialogPort` + `FileTree` + `LayoutState` + `TabStrip` | IDE-1.1 | **SIGNED OFF.** `FakeDialog` / `MemFs`; resizable width is a value. llvm-cov **95.43%** lines. Mutants poc-ide **100/100 (100%)**, 23 unviable. |

## IDE-2 (`ide2` branch)

**Status: SIGNED OFF** on `ide2`. Do not open `ide3` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-2.1 | `OpenBuffer` / `EditCommand` / save | IDE-1 | **SIGNED OFF.** ropey 1.6.1; `FakeClipboard`; `FsPort` read/write |
| IDE-2.2 | `Highlighter` syntect Adapter | IDE-2.1 | **SIGNED OFF.** syntect 5.3.0; no Tree-sitter in the IDE. llvm-cov **95.59%** lines. Mutants poc-ide **211/213 (99.1%)**, 39 unviable |

## IDE-3 (`ide3` branch)

**Status: SIGNED OFF** on `ide3`. Do not open `ide4` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-3.1 | `DiskWatch` + `ConflictModal` | IDE-2 | **SIGNED OFF.** `FakeWatch` / `FakeClock`; no `sleep`. llvm-cov **95.79%** lines. Mutants poc-ide **278/282 (98.6%)**, 60 unviable |

## IDE-4 (`ide4` branch)

**Status: SIGNED OFF** on `ide4`. Do not open `ide5` until this table stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-4.1 | `LanguageCatalog` | IDE-1 | **SIGNED OFF.** Extension table; unknown → `plaintext`; plaintext skips `didOpen`. |
| IDE-4.2 | `LspClient` + definition / implementation / references | IDE-2, IDE-4.1 | **SIGNED OFF.** `FakeLsp`; stock stdio; `lsp-types` 0.97.0. llvm-cov **95.86%** lines. Mutants poc-ide **535/555 (96.4%)**, 88 unviable |

## IDE-5 (`ide5` branch)

**Status: SIGNED OFF** on `ide5`. Last POC WP. No `ide6`. `--mux` is `pending_mux`.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| IDE-5.1 | `ControlClient` Envelope Adapter | IDE-4 | **SIGNED OFF.** `progressive-lsp-control`; `FakeControl`; payload > 16 MiB fails. |
| IDE-5.2 | `ProtocolConsole` LSP + control | IDE-5.1 | **SIGNED OFF.** mux `pending_mux`. llvm-cov **95.99%** lines. Mutants poc-ide **683/711 (96.1%)**, 115 unviable |

## LOG-0 (`log0` branch)

**Status: SIGNED OFF** on `log0`. Parent is current `main` (PR #4 / `poc-discover-log` already merged). LOG-1 may start after this WP; do not open `log1` from this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-0.1 | Ingest [logging.md](logging.md) + [logging-plan.md](logging-plan.md); pattern rows; stack | current `main` | **SIGNED OFF.** Docs only. No `progressive-lsp-log` crate. No rusqlite in server. No `eprintln!` changes. Tests / 95% llvm-cov / 80% mutants / `sleep` / `check-static` are **N/A**. |

**Sign-off checklist (LOG-0.1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — **N/A** (no crates)
- [x] 95% llvm-cov on crates that exist — **N/A** (none added)
- [x] 80% mutants on listed crates that exist — **N/A** (none added)
- [x] No `sleep` — **N/A** (no tests)
- [x] `check-static` if ELF changed — **N/A** (no ELFs)
- [x] [design-patterns.md](design-patterns.md) names every type in [logging.md](logging.md)
- [x] Docs in this tree updated if a locked decision was refined (`log0`–`log4` on current `main`; rusqlite amalgamation is our artifact; poc-ide `RunLog` stays a separate schema)

## LOG-1 (`log1` branch)

**Status: SIGNED OFF** on `log1`. LOG-2 may start after this WP; do not open `log2` from this branch. No rusqlite in server. No `eprintln!` changes.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-1.1 | `LogPort` + `LogRecord` + scope / Fake / Memory / Null / NeverFail in `progressive-lsp-core` | LOG-0 | **SIGNED OFF.** core stays sqlite-free. llvm-cov **96.29%** lines. Mutants core **201/217 (92.6%)**, 33 unviable |
| LOG-1.2 | `[log]` config overlay (`level`, `path`); invalid level → warn + default | LOG-1.1 | **SIGNED OFF.** merge chain; unknown keys still warn |

**Sign-off checklist (LOG-1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p progressive-lsp-core -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **96.29%** lines
- [x] 80% mutants on listed crates that exist — core **201 caught / 217 scored (92.6%)**, 33 unviable, 16 missed
- [x] No `sleep`
- [x] `check-static` if ELF changed — **N/A** (no rusqlite; Darwin: do not fake musl greens)
- [x] [design-patterns.md](design-patterns.md) names every new type (`LogScopeGuard` on the `LogScope` row)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-2 (`log2` branch)

Do not open `log2` until LOG-1 stays signed off. No product `eprintln!` death yet.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-2.1 | `progressive-lsp-log` workspace member: `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath` | LOG-1 | pin `rusqlite = { version = "=0.40.2", features = ["bundled"] }` |
| LOG-2.2 | musl amalgamation + `check-static` on the core ELF | LOG-2.1 | fail closed if `libdl` is `DT_NEEDED`; tempfile WAL; `BATCH_MAX=1`; no `thread::sleep` |

## LOG-3 (`log3` branch)

Do not open `log3` until LOG-2 stays signed off.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-3.1 | `LogCrateBridge` / `TracingBridge` / `ChildStderrAdapter` / `LogFileTailAdapter` / `LspLogMessageAdapter` | LOG-2 | never attach stderr Adapter to engine stdout |
| LOG-3.2 | Composition-root `LogPort`; emit `ConfigLoad.warnings`; replace product `eprintln!` | LOG-3.1 | CLI usage/help still stderr (IT-1.7); grep gate; allowlist clangd `--log=`; optional gopls `-logfile`; not `-rpc.trace` |

## LOG-4 (`log4` branch)

Do not open `log4` until LOG-3 stays signed off. Last LOG WP. No `log5`.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-4.1 | Bootstrap order in `run`; `LogScope` around didOpen/didChange/definition; index/watch/install silent-failure emits | LOG-3 | `Flush` + join on shutdown |
| LOG-4.2 | User troubleshooting + host-deps / third-party lock vs impl; IT-1.7 still stderr | LOG-4.1 | optional sqlite file after `serve` handshake (Linux CI) |

## Spikes (do not skip hygiene on merge)

| Spike | Lives | Merge rule |
|---|---|---|
| glibc-static POC | `spike/glibc-static` | fold into M0 xtask only if `check-static` clean |
| csharp-ls AOT+musl | `spike/` notes | fail → C# T2 ceiling in matrix |
| PHPantom vs static phpactor | `spike/` | no host `php`; no Node |
| clangd static archive graph | notes | miss → document; do not ship `.so` |
| T2 Strategy bake-off | [docs/spikes/t2-strategy-bakeoff.md](spikes/t2-strategy-bakeoff.md) | PD4; heuristics stay default until numbers |

## Agent instructions

1. Read [README.md](README.md), then the docs for your WP.
2. Implement only that WP’s crates/files.
3. Map new types in [design-patterns.md](design-patterns.md).
4. Do not add Node/JVM/CPython, `$/` FilesSince, or SSH in the install crate.
5. Stop at sign-off; do not start the next milestone branch (`pdN+1` until `pdN` signed off; `ideN+1` until `ideN` signed off; `logN+1` until `logN` signed off).
6. POC orchestrators: pass [poc-ide/agent-context.md](poc-ide/agent-context.md) unchanged to every child.
7. LOG orchestrators: pass [logging/agent-context.md](logging/agent-context.md) unchanged to every child. Stack `log0` on current `main`, not `poc-no-console`.
