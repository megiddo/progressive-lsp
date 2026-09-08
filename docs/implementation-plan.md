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
                          └── log4   # Wire serve/install + docs lock (SIGNED OFF)
                                └── log5   # remaining-coverage docs ingest
                                      └── log6   # supervisor + ScriptHost lifecycle
                                            └── log7   # protocol + control + install hash
                                                  └── log8   # T3 skip + session completeness
                                                        └── log9   # durable WAL fallback
                                                              └── log10  # child capture (FakeChildStderr)
                                                                    └── log11  # operational Err hygiene

main   # after log11 merge
  └── poc-proof-log   # empty F12 info; LOG_LEVEL env; poc-ide debug spawn
        └── poc-lsp-async   # LSP/control IO threads (do not start from poc-proof-log)
              └── poc-tier-status
                    └── poc-no-stall
                          └── host0   # native vs container File menu; T3HostOffer; RuntimePort
                                └── host1   # PackAdapter Linux Command spawn
                                      └── host2  # musl core ELF extract both triples
                                            └── host3  # slim pack jobs both triples
                                                  └── host4  # runtime image
                                                        └── host5  # docker run attach
                                                              └── host6  # mux client
                                                                    └── host7  # full flavor packs (last numbered host slice)
                                                                          └── fix-superhtml-x8664  # zig qemu faccessat
                                                                                └── host-cleanup  # require superhtml; operator-cli merged; not host8
                                                                                      # merged to main (PR #6 / #7)

main   # after host-cleanup + Java T3 wiring merge
  └── poc-uri  # identity file: URIs
        └── t2-coverage
              └── java-t3
                    └── t3-image
                          └── t3-rest
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
| M6.4 | Conformance dashboard | M5.2 | **SIGNED OFF.** [conformance.md](conformance.md); C# T1/T2 only; Java T3 is a static pack (0% on Darwin stubs) |
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

**Status: SIGNED OFF** on `log2`. LOG-3 may start after this WP; do not open `log3` from this branch. No product `eprintln!` death. No serve/install bootstrap. No capture bridges.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-2.1 | `progressive-lsp-log` workspace member: `SqliteLogRepository`, `WriterActor`, `CrashSafeBatch`, `ServeLogPath` | LOG-1 | **SIGNED OFF.** pin `rusqlite = { version = "=0.40.2", features = ["bundled"] }`. Core stays sqlite-free. |
| LOG-2.2 | musl amalgamation + `check-static` on the core ELF | LOG-2.1 | **SIGNED OFF.** `LIBSQLITE3_FLAGS` omits loadable extensions; `check-static` fail-closed on `libdl` fixtures. Darwin: do not fake musl greens (same class as M0). |

**Sign-off checklist (LOG-2)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p progressive-lsp-log -- --test-threads=1` (33 passed)
- [x] 95% llvm-cov on crates that exist — **96.04%** lines
- [x] 80% mutants on listed crates that exist — log crate **91 caught / 104 scored (87.5%)**, 17 unviable, 13 missed, 3 timeouts
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (bin not wired; no musl ELF on this host)
- [x] [design-patterns.md](design-patterns.md) names every new type (`ReentrancyGuard` on the Proxy / Guard row)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-3 (`log3` branch)

**Status: SIGNED OFF** on `log3`. LOG-4 may start after this WP; do not open `log4` from this branch. No sqlite serve/install bootstrap. No `LogScope` around didOpen.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-3.1 | `LogCrateBridge` / `TracingBridge` / `ChildStderrAdapter` / `LogFileTailAdapter` / `LspLogMessageAdapter` | LOG-2 | **SIGNED OFF.** never attach stderr Adapter to engine stdout; `ChildIo` is LSP stdout + optional stderr pipe; `NullStderrAdapter` forbidden on prod pack spawn |
| LOG-3.2 | Composition-root `LogPort`; emit `ConfigLoad.warnings`; replace product `eprintln!` | LOG-3.1 | **SIGNED OFF.** `MemoryLog` bootstrap (sqlite wire is LOG-4); CLI usage/help still stderr (IT-1.7); grep gate; allowlist clangd `--log=`; optional gopls `-logfile`; not `-rpc.trace` |

**Sign-off checklist (LOG-3)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **96.44%** lines
- [x] 80% mutants on listed crates that exist — log crate **137 caught / 151 scored (90.7%)**, 17 unviable, 14 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (bin not sqlite-wired; no musl ELF on this host)
- [x] [design-patterns.md](design-patterns.md) names every new type (`ChildIo` Value; `FakeChildStderr` test double)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-4 (`log4` branch)

**Status: SIGNED OFF** on `log4`. Parent of `log5`. Do not reopen LOG-0–LOG-4. Remaining coverage is LOG-5+.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-4.1 | Bootstrap order in `run`; `LogScope` around didOpen/didChange/definition; index/watch/install silent-failure emits | LOG-3 | **SIGNED OFF.** `Flush` + join on shutdown; one WAL per serve/install; `PROGRESSIVE_LSP_LOG` override; sqlite open fail keeps `MemoryLog` |
| LOG-4.2 | User troubleshooting + host-deps / third-party lock vs impl; IT-1.7 still stderr | LOG-4.1 | **SIGNED OFF.** sqlite under `$PREFIX/log/`; optional sqlite file after `serve` handshake (Linux CI); Darwin: do not fake musl greens |

**Sign-off checklist (LOG-4)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **95.93%** lines
- [x] 80% mutants on listed crates that changed — log **140/154 (90.9%)**; index **160/194 (82.5%)**; install **114/122 (93.4%)**; watch **97/101 (96.0%)**
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) names every type (no new types; existing rows updated)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-5 (`log5` branch)

**Status: SIGNED OFF** on `log5`. Parent is `log4`. Documentation ingest for remaining operational coverage. No crates. No Rust. Tests / 95% llvm-cov / 80% mutants / `sleep` / `check-static` are **N/A**. Do not open `log6` from this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-5.1 | Ingest coverage matrix + LOG-6–LOG-11 mutation; pattern rows; stack | LOG-4 | **SIGNED OFF.** Docs only. Files: [logging.md](logging.md), [logging-plan.md](logging-plan.md), [logging/agent-context.md](logging/agent-context.md), milestones, this file, [branching.md](branching.md), [design-patterns.md](design-patterns.md), [user/README.md](user/README.md). **Do not** start supervisor `with_log`. **Do not** start `LogOpenPlan` in Rust. **Do not** implement `Command` spawn. |

**Sign-off checklist (LOG-5)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — **N/A** (docs only)
- [x] 95% llvm-cov on crates that exist — **N/A**
- [x] 80% mutants on listed crates that exist — **N/A**
- [x] No `sleep` — **N/A**
- [x] `check-static` if ELF changed — **N/A**
- [x] [design-patterns.md](design-patterns.md) names `LogOpenPlan` (draft; Rust is LOG-9)
- [x] Docs in this tree updated if a locked decision was refined (LOG-0–LOG-5 stay signed off; `log5`–`log11` on `log4`)

## LOG-6 (`log6` branch)

**Status: SIGNED OFF** on `log6`. Parent is `log5`. Do not open `log7` until this table is signed off (it is). No `Command` spawn. No ChildIo readers. No protocol/control emits. No `LogOpenPlan`.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-6.1 | `EngineSupervisor::with_log`; emit `try_spawn` / `note_crash` / Hash / stub refuse / backoff | LOG-5 | **SIGNED OFF.** FakeLog: `operation=spawn`, `component=engine`. Serve **holds** supervisor and `try_spawn`s. **Do not** start `std::process::Command`. |
| LOG-6.2 | `ScriptHost::with_log`; bootstrap Abort, spawn Skip, pre_index skip, discover drop, install-verify Abort | LOG-6.1 | **SIGNED OFF.** FakeLog: `operation=initialize`/`spawn`/`index`/`install`/`script`. **Do not** start `EngineResolver` (LOG-8). |

**Sign-off checklist (LOG-6)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **95.90%** lines
- [x] 80% mutants on listed crates that changed — engine **134 caught / 156 scored (85.9%)**, 63 unviable, 22 missed; script **64 caught / 76 scored (84.2%)**, 4 unviable, 12 missed; combined **198 caught / 232 scored (85.3%)**, 67 unviable, 34 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) names every new type (prefer `with_log`; no new Facade)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-7 (`log7` branch)

**Status: SIGNED OFF** on `log7`. Parent is `log6`. Do not open `log8` until this table is signed off (it is). No LSP bodies. No `LogOpenPlan`. No child capture.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-7.1 | `LspFacade::with_log`; parse / Content-Length / method-not-found / mux | LOG-6 | **SIGNED OFF.** FakeLog `operation=protocol`. **Never** log payload bytes. |
| LOG-7.2 | Control bind/accept/`PayloadTooLarge`/unknown method/`Status::error`; `--control-fd` warn | LOG-7.1 | **SIGNED OFF.** FakeLog `operation=control`. **Do not** implement `--control-fd`. |
| LOG-7.3 | `InstallError::Hash` + verify refuse emit before `remove_or_emit` | LOG-6 | **SIGNED OFF.** FakeLog `operation=install`; expected/actual hex. **Do not** start durable fallback. |

**Sign-off checklist (LOG-7)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **95.89%** lines
- [x] 80% mutants on listed crates that changed — protocol **107 caught / 130 scored (82.3%)**, 18 unviable, 23 missed, 3 timeouts; control **88 caught / 95 scored (92.6%)**, 1 unviable, 7 missed; install **117 caught / 124 scored (94.4%)**, 10 unviable, 7 missed, 1 timeout; combined **312 caught / 349 scored (89.4%)**, 29 unviable, 37 missed, 4 timeouts
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) names every new type (prefer `with_log`; no new Facade)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-8 (`log8` branch)

**Status: SIGNED OFF** on `log8`. Parent is `log7`. Do not open `log9` until this table is signed off (it is). Do not fail the user on T3 skip. No `Command` spawn.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-8.1 | `EngineResolver` first in session chain when supervisor attached; once-per `(language, package)` skip | LOG-7 | **SIGNED OFF.** FakeLog `operation=resolve` `info`. Second definition does not duplicate. T2/T1 still `Ready`. |
| LOG-8.2 | initialize success/fail; `didClose` debug; `shutdown` debug; FilesSince truncated; unknown language | LOG-8.1 | **SIGNED OFF.** FakeLog. Truncated FilesSince emits from `ServeHost` only. Initialize fail is sqlite **and** JSON-RPC `-32002`. **Do not** emit every `didChange`. |

**Sign-off checklist (LOG-8)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1`
- [x] 95% llvm-cov on crates that exist — **96.01%** lines
- [x] 80% mutants on listed crates that changed — engine **154 caught / 172 scored (89.5%)**; resolve **113 caught / 131 scored (86.3%)**; combined **267 caught / 303 scored (88.1%)**
- [x] No `sleep`
- [x] `check-static` if ELF changed — fixture `libdl` fail-closed; Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) updated (`EngineResolver` skip-once invariant; `ServeHost` FilesSince emit)
- [x] Docs in this tree updated if a locked decision was refined

## LOG-9 (`log9` branch)

**Status: SIGNED OFF** on `log9`. Parent is `log8`. Do not open `log10` until this table is signed off (it is). No syslog / JSON / OTel.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-9.1 | `LogOpenPlan` + `ServeLogPath::fallback` / `in_temp`; `wire_process_log` uses it | LOG-8 | **SIGNED OFF.** tempfile: primary fail → fallback WAL has warn + replay. **Do not** start child capture. |
| LOG-9.2 | User troubleshooting for missing primary sqlite | LOG-9.1 | **SIGNED OFF.** `serve-fallback-*.sqlite` then temp WAL |

**Sign-off checklist (LOG-9)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p progressive-lsp-log` and bin tests `--test-threads=1`
- [x] 95% llvm-cov on crates that exist — **96.02%** lines
- [x] 80% mutants on listed crates that changed — `progressive-lsp-log` **140 caught / 153 scored (91.5%)**, 36 unviable, 12 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` if ELF changed — Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) names `LogOpenPlan`
- [x] Docs in this tree updated if a locked decision was refined

## LOG-10 (`log10` branch)

**Status: SIGNED OFF** on `log10`. Parent is `log9`. Ready when spawn exists. **Do not** implement `PackAdapter` `Command`. Do not open `log11` from this branch.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-10.1 | Attach `ChildStderrAdapter` when a stderr `Read` exists; tests `FakeChildStderr` | LOG-9 | **SIGNED OFF.** stdout never attached; `NullStderrAdapter` forbidden on prod spawn |
| LOG-10.2 | `LogFileTailAdapter` / `LspLogMessageAdapter` only when a tail path / proxied logMessage exists | LOG-10.1 | **SIGNED OFF.** tempfile / FakeLog. **Do not** enable `-rpc.trace`. |

**Sign-off checklist (LOG-10)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test --workspace -- --test-threads=1` (llvm-cov workspace suite green; log 56, engine 38, composition-root 59)
- [x] 95% llvm-cov on crates that exist — **96.07%** lines
- [x] 80% mutants on listed crates that changed — log **146 caught / 159 scored (91.8%)**, 39 unviable, 12 missed, 1 timeout; engine **168 caught / 185 scored (90.8%)**, 69 unviable, 17 missed
- [x] No `sleep`
- [x] `check-static` if ELF changed — Darwin: do not fake musl greens (no musl ELF on this host; Linux CI checks the rusqlite-linked ELF)
- [x] [design-patterns.md](design-patterns.md) — reuse existing Adapters; no new type unless a Port is required
- [x] Docs in this tree updated if a locked decision was refined

## LOG-11 (`log11` branch)

**Status: SIGNED OFF** on `log11`. Parent is `log10`. Last LOG WP of this stack. No new Adapters. There is no `log12`.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LOG-11.1 | Hygiene gate: every operational `Err` emits or is listed client-visible-only | LOG-10 | **SIGNED OFF.** `tests/log_hygiene.rs` + [logging.md](logging.md) matrix. **Do not** reopen LOG-0–LOG-10 types. |

**Sign-off checklist (LOG-11)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — hygiene test + `cargo test --workspace -- --test-threads=1` (hygiene 6; composition-root 59; log 56; engine 38)
- [x] 95% llvm-cov on crates that exist — **96.07%** lines
- [x] 80% mutants — **N/A** (hygiene test + docs only; no listed crate logic change)
- [x] No `sleep`
- [x] `check-static` if ELF changed — **N/A** (ELF unchanged)
- [x] Docs in this tree updated if a locked decision was refined
- [x] [design-patterns.md](design-patterns.md) — no new type; Domain Result row names the hygiene gate

## poc-proof-log (`poc-proof-log` branch)

**Status: SIGNED OFF** on `poc-proof-log`. Parent is current `main` (log11 merge / `a0f10a2`). Do not start `poc-lsp-async` from this branch. Do not reopen LOG-0–LOG-11. `RunLog` stays a separate schema from the serve WAL.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PPL-1 | Empty F12 / implementation / references is **info** with extras (`location_count=0`, path, line, character, language_id, package_id, tier) | log11 | **SIGNED OFF.** Non-empty stays debug. `didChange` stays debug. |
| PPL-2 | `PROGRESSIVE_LSP_LOG_LEVEL` overrides `[log].level`; invalid → warn + `info` | PPL-1 | **SIGNED OFF.** `LogLevel::from_env_or_config` + `LevelFilter`. Stock Neovim without the env stays info. |
| PPL-3 | poc-ide default `ControlSocket`, child `LOG_LEVEL=debug`, stderr→RunLog, `ProofStatus` footer | PPL-2 | **SIGNED OFF.** `run_start` records binary / argv / level / both sqlite paths. |
| PPL-4 | `xtask poc` builds serve then runs poc-ide with `PROGRESSIVE_LSP` | PPL-3 | **SIGNED OFF.** Supported proof launch. Args after `--` forward. Spawn shell N/A. |

**Sign-off checklist (poc-proof-log)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (core 72; composition-root lib 68; poc-ide lib 176; xtask 30; hygiene 6)
- [x] 95% llvm-cov on crates that exist — **96.10%** lines
- [x] 80% mutants on listed crates that changed — core in-diff **7/7 (100%)**; poc-ide in-diff **34/37 (91.9%)**; composition-root session/lib in-diff **13/13 (100%)**; combined **54/57 (94.7%)**
- [x] No `sleep`
- [x] `check-static` — **N/A** (musl ELF story unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `LevelFilter`, `ProofStatus`, `ChildStderrDrain`, `ServeSpawn`, `ControlSocketPath`, `ServeWalPath`, `RunStart`, `PocArgs`

## poc-lsp-async (`poc-lsp-async` branch)

**Status: SIGNED OFF** on `poc-lsp-async`. Parent is `poc-proof-log`. `poc-tier-status` stacks on this branch. `RunLog` stays a separate schema from the serve WAL.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PLA-1 | LSP IO thread owns stdin/stdout + stderr drain; `LspIoRequest` / `LspIoEvent`; keep `$/progress` | poc-proof-log | **SIGNED OFF.** UI apply never calls `LspTransport::request`. |
| PLA-2 | Discover in flight: F12 / context menu disabled (`waiting for server`); `didChange` queued | PLA-1 | **SIGNED OFF.** `DiscoverFlight` value object. |
| PLA-3 | Control IO thread / inbox; `fn ui` never calls `index_status` / `tier_status` | PLA-1 | **SIGNED OFF.** Records `ControlPush` in `ControlPushInbox` + RunLog. |

**Sign-off checklist (poc-lsp-async)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (core 72; composition-root lib 68; poc-ide lib 188; xtask 30; hygiene 6)
- [x] 95% llvm-cov on crates that exist — **95.94%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **27/27 (100%)** caught (9 unviable)
- [x] No `sleep`
- [x] `check-static` — **N/A** (musl ELF story unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `LspIoRequest`, `LspIoEvent`, `ProgressEvent`, `LspProgressKind`, `LogMessageEvent`, `LspIoMailbox`, `DiscoverFlight`, `ControlPushInbox`, `ControlIoEvent`

## poc-tier-status (`poc-tier-status` branch)

**Status: SIGNED OFF** on `poc-tier-status`. Parent is `poc-lsp-async`. `poc-no-stall` stacks on this branch. `RunLog` stays a separate schema from the serve WAL.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PTS-1 | Additive `IndexStatus.ingest` (`not_started` / `running` / `done`); fill from session | poc-lsp-async | **SIGNED OFF.** Next proto tag; no new RPC. |
| PTS-2 | `TierStrip` / `TierCell` for focused package (workspace aggregate fallback); IndexStatus / TierStatus requested on control IO thread | PTS-1 | **SIGNED OFF.** C# T3 `not supported`; Rust/CSS T2 `n/a`; stub refuse `skipped`. No-file strip paints T1/T2 from workspace ingest. |
| PTS-3 | `DiscoverOffer` + honest context / Navigate menus from LanguageCatalog × current tier | PTS-2 | **SIGNED OFF.** FakeLsp not called while disabled. |

**Sign-off checklist (poc-tier-status)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (control 19; poc-ide lib 193; composition-root lib 69; core 72; xtask 30)
- [x] 95% llvm-cov on crates that exist — **95.96%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **42/42 (100%)** caught (8 unviable); control in-diff **13/13 (100%)**; composition-root in-diff **3/3 (100%)**; combined **58/58 (100%)**
- [x] No `sleep`
- [x] `check-static` — **N/A** (musl ELF story unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `IngestState`, `WireTier`, `DiscoverOffer`, `TierStrip`, `TierCell`, `PackageTierMap`, `DiscoverMenu`

## poc-no-stall (`poc-no-stall` branch)

**Status: SIGNED OFF** on `poc-no-stall`. Parent is `poc-tier-status`. This is the last POC-proof slice. Do not reopen POC-proof WPs. The allowed next stack is `host0` (HOST-0). Do not implement PackAdapter `Command` spawn from this branch. `RunLog` stays a separate schema from the serve WAL.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| PNS-1 | `HighlightCache` keyed by path + rope generation; second highlight of unchanged text does not re-tokenize | poc-tier-status | **SIGNED OFF.** Unknown syntax stays empty spans. |
| PNS-2 | Tree expand worker: `ExpandChainCommand` / `TreeIoRequest` / `TreeExpandFlight`; children show `loading…` until the inbox fills | PNS-1 | **SIGNED OFF.** UI apply never calls `read_dir`. MemFs / FakeClock; no `thread::sleep`. |
| PNS-3 | Navigate / F12 disabled until `LspSessionState::Ready` (`connecting language server`) | PNS-2 | **SIGNED OFF.** Same `DiscoverMenu` as context menu; keyboard F12 uses `queue_discover`. Save / disk conflict modal stays. |

**Sign-off checklist (poc-no-stall)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (poc-ide lib 203; composition-root lib 69; core 72; xtask 30)
- [x] 95% llvm-cov on crates that exist — **95.96%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **67 caught / 78 scored (85.9%)**, 34 unviable, 10 missed, 1 timeout
- [x] No `sleep`
- [x] `check-static` — **N/A** (musl ELF story unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `HighlightKey`, `HighlightCache`, `ExpandChainCommand`, `CompactChainListing`, `TreeIoRequest`, `TreeIoEvent`, `TreeIoMailbox`, `TreeExpandFlight`

## HOST-0 (`host0` branch)

**Status: SIGNED OFF** on `host0`. Parent is `poc-no-stall` (`63507ff`). Do not open `host1` from this branch. Do not implement PackAdapter `Command` spawn. Do not `docker run` attach. Do not build musl images. Do not mux client. Tests never talk to a Docker daemon (`FakeRuntime` / scripted CLI). `RunLog` stays a separate schema from the serve WAL.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-0.1 | Native vs container File menu: `HostOs`, `OpenMode`, `LaunchFlags`, `DialogAction::OpenFolderInContainer` | poc-no-stall | **SIGNED OFF.** Linux has no container item; `--container` is a no-op there. |
| HOST-0.2 | `T3HostOffer` + strip / discover honest labels | HOST-0.1 | **SIGNED OFF.** Native non-Linux T3 is `skipped` / `open folder in container`. Never two LSP processes. |
| HOST-0.3 | `LaunchJournal` / `StatusModal` | HOST-0.2 | **SIGNED OFF.** Close does not cancel work. T1/T2/T3/container kinds. |
| HOST-0.4 | `RuntimePort` + `FakeRuntime` + `DockerRuntime` probe/image (`start` unwired) | HOST-0.3 | **SIGNED OFF.** Tests inject `FakeRuntime` or a scripted CLI; no daemon / registry / AWS. |
| HOST-0.5 | `RuntimeIoRequest` / `RuntimeIoEvent` mailbox | HOST-0.4 | **SIGNED OFF.** UI submits launch; tests `pump_runtime_io`. `DockerRuntime::start` still errors. |
| HOST-0.6 | Core `path_to_file_uri` / `path_from_file_uri` Adapter | poc-no-stall | **SIGNED OFF.** Same percent-encoding as poc-ide; session/serve_host decode incoming URIs. |
| HOST-0.7 | Host stack docs | HOST-0.1 | **SIGNED OFF.** [host/agent-context.md](host/agent-context.md); branching `poc-no-stall └── host0`; `host1`–`host7` listed as future only. |

**Sign-off checklist (HOST-0)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped + composition-root `--test-threads=1` (poc-ide lib 214; composition-root lib 73; core 74)
- [x] 95% llvm-cov on crates that exist — **95.99%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff **129/140 (92.14%)**; core in-diff **46/50 (92.00%)**
- [x] No `sleep`
- [x] `check-static` — **N/A** (musl ELF story unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `HostOs`, `OpenMode`, `T3HostOffer`, `LaunchFlags`, `RuntimePort`, `FakeRuntime`, `DockerRuntime`, `RuntimeInfo`, `RuntimeSession`, `LaunchJournal`, `LaunchStep`, `StepState`, `StatusModal`, `RuntimeIo*`, `file_uri`

## HOST-1 (`host1` branch)

**Status: SIGNED OFF** on `host1`. Parent is `host0` (`1d33445`). Do not open `host2` from this branch. Do not build runtime images, `docker run` attach, mux client, or pack *builds*. Tests never talk to a Docker daemon, registry, or AWS. No real ty/clangd download (`FakeEngineAdapter` / fixture bytes / `RecordingSpawnPort` only). `RunLog` stays a separate schema from the serve WAL.

Serve already holds `EngineSupervisor` and `try_spawn`s after initialize (LOG-6). Not HOST-1.x.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-1.1 | `SpawnPlan` value object (argv, cwd, env, `ChildIo`) | host0 | **SIGNED OFF.** Darwin unit tests cover the Linux plan without `Command`. Production plan is `lsp_with_stderr_pipe`. |
| HOST-1.2 | Linux `Command` spawn via `SpawnPort`; stub + Darwin refuse | HOST-1.1 | **SIGNED OFF.** Stub message unchanged. Non-Linux `EngineError::Spawn` (“not this OS”). `RecordingSpawnPort` is would-have-spawned. Hash mismatch still no spawn. Supervisor degrades T1/T2 if spawn fails. |
| HOST-1.3 | Docs sign-off | HOST-1.2 | **SIGNED OFF.** milestones HOST-1; branching `host0 └── host1`; `host2`–`host7` still future. PackAdapter row: Command on Linux; refuse stub + Darwin. |

**Sign-off checklist (HOST-1)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — crate-scoped `--test-threads=1` (engine lib 41; composition-root lib 73)
- [x] 95% llvm-cov on crates that exist — **95.99%** lines
- [x] 80% mutants on listed crates that changed — engine in-diff **26 caught / 26 scored (100%)**, 13 unviable
- [x] No `sleep`
- [x] `check-static` — **N/A** (ELF unchanged). Darwin: do not fake musl greens
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `SpawnPlan`, `SpawnPort`, `CommandSpawnPort`, `RecordingSpawnPort`; PackAdapter / `ChildIo` rows updated

## HOST-2 (`host2` branch)

**Status: SIGNED OFF** on `host2`. Parent is `host1` (`b9490b6`). Do not open `host3` from this branch. Do not build engine pack binaries (ty, rust-analyzer, phpantom, biome, superhtml, clangd, …). Do not build the runtime image, `docker run` attach, or mux client. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture ELF only). No real ty/clangd download. `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-2.1 | `MuslBuildPlan` value object (triple, docker platform, dockerfile, dest, `RUST_TARGET`) | host1 | **SIGNED OFF.** Darwin unit tests cover both triples without docker. Dest is `target/musl/<triple>/progressive-lsp`. Unknown triple / missing dockerfile fail closed. |
| HOST-2.2 | `docker build --output` extract + `DockerPort`; `check-static` after extract | HOST-2.1 | **SIGNED OFF.** `CommandDockerPort` is production. `RecordingDockerPort` is would-have-built (fixture ELF, not a musl green). Dockerfile still builds only `--bin progressive-lsp`. Mach-O still refused. |
| HOST-2.3 | Docs sign-off | HOST-2.2 | **SIGNED OFF.** milestones HOST-2; branching `host1 └── host2`; `host3`–`host7` still future. host-deps / testing dest convention. |

## HOST-3 (`host3` branch)

**Status: SIGNED OFF** on `host3`. Parent is `host2` (`7e34b2c`). Do not open `host4` from this branch. Do not build the runtime image, `docker run` attach, or mux client. Do not build clangd/tsgo/gopls/zls. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture ELF only). No real ty/clangd download in crate tests. `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-3.1 | `PackPin` / `PackKind` / `RustToolchainPin` / `ZigToolchainPin` + `xtask/pack-pins.toml` | host2 | **SIGNED OFF.** Slim packs pinned by 40-hex git SHA (not `latest`). Rustc 1.98.0; Zig 0.15.1 tarball SHA256. |
| HOST-3.2 | `PackBuildPlan` + `xtask pack` via existing `DockerPort::extract` | HOST-3.1 | **SIGNED OFF.** Dest `target/musl/<triple>/engines/<pack>/<binary>`. Unknown / heavy packs fail closed. Darwin unit tests name the pattern; `RecordingDockerPort` is would-have-built. |
| HOST-3.3 | Hermetic pack Dockerfiles + live extract both triples | HOST-3.2 | **SIGNED OFF.** `docker/engine-pack.Dockerfile` (ty, RA, phpantom, biome) and `docker/engine-pack-zig.Dockerfile` (superhtml). Live `check-static` **10/10 PASS**. `superhtml` × x86_64 qemu `faccessat` miss was closed later: host-native docker platform + Zig `-Dtarget`. |
| HOST-3.4 | Docs sign-off | HOST-3.3 | **SIGNED OFF.** milestones HOST-3; branching `host2 └── host3`; `host4`–`host7` still future. host-deps / testing dest convention. |

## HOST-4 (`host4` branch)

**Status: SIGNED OFF** on `host4`. Parent is `host3` (`0d24582`). Do not open `host5` from this branch. Do not `docker run` attach. Do not mux client. Do not build clangd/tsgo/gopls/zls. The runtime Dockerfile copies prebuilt ELFs only — no rustc/cargo/clang/LLVM/zig/go. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-4.1 | `RuntimeImagePlan` / `PackImageCopy` value objects | host3 | **SIGNED OFF.** Darwin unit tests name the pattern and cover both triples without docker. Tag is `progressive-lsp-runtime:local`. Prefix in image is `/opt/plsp`. Unknown triple / missing dockerfile / missing required core ELF fail closed. HOST-4 allowed omitting `superhtml` × x86_64 (HOST-3 qemu miss). **Superseded by HOST-CLEANUP:** slim including superhtml is required on both triples. |
| HOST-4.2 | `xtask runtime-image` + scratch Dockerfile + `DockerPort::tag_image` | HOST-4.1 | **SIGNED OFF.** Staging dir `target/runtime-image/<triple>` (not the git tree). `FROM scratch`; `COPY` only. `RecordingDockerPort` is would-have-tagged. Live tag both platforms (orchestrator proof). |
| HOST-4.3 | Docs sign-off | HOST-4.2 | **SIGNED OFF.** milestones HOST-4; branching `host3 └── host4`; `host5`–`host7` still future. host-deps: runtime image is our artifact; docker CLI remains a host tool; tests still FakeRuntime. |

**Sign-off checklist (HOST-4)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (60 passed)
- [x] 95% llvm-cov on crates that exist — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only; xtask is not on the 80% list)
- [x] No `sleep`
- [x] `check-static` — **N/A** (no new ELF; image copies HOST-2/HOST-3 dests). Do not run `check-static` on a Darwin Mach-O
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `RuntimeImagePlan`, `PackImageCopy`; `DockerPort` `tag_image` row updated

## HOST-5 (`host5` branch)

**Status: SIGNED OFF** on `host5`. Parent is `host4` (`67ca485`). Do not open `host6` from this branch. Do not mux client / `serve --mux`. Do not build clangd/tsgo/gopls/zls. Do not rebuild the runtime image except to *reference* `progressive-lsp-runtime:local`. Container LSP is stdio only (`ServeMode::StockStdio`). Tests never talk to a Docker daemon, registry, or AWS (`FakeRuntime` / scripted CLI / `DockerRunPlan` argv). `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-5.1 | `DockerRunPlan` value object + `DockerRuntime::start` validates (no exec) | host4 | **SIGNED OFF.** Darwin unit tests name the pattern. `run -i --rm -v WS:WS -w WS` + image `progressive-lsp-runtime:local` + `serve --prefix /opt/plsp`. Never `-t`. Empty / relative workspace and missing absolute docker binary fail closed. |
| HOST-5.2 | `LspIoAttach` + `StdioLsp::from_command` is the single exec | HOST-5.1 | **SIGNED OFF.** Container → docker Command + `StockStdio`. Native unchanged (`ControlSocket`). `start` does not `docker run`; one Linux serve. No Darwin `progressive-lsp` on container open. |
| HOST-5.3 | Docs sign-off | HOST-5.2 | **SIGNED OFF.** milestones HOST-5; branching `host4 └── host5`; `host6`–`host7` still future. host-deps: attach is plan → stdio; tests still FakeRuntime. Live initialize proof recorded in milestones (not a cargo test). |

**Sign-off checklist (HOST-5)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p poc-ide --lib -- --test-threads=1` (216 passed)
- [x] 95% llvm-cov on crates that exist — **96.50%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff vs `67ca485` **19 caught / 23 scored (82.6%)**, 1 unviable, 4 missed, 0 timeouts
- [x] No `sleep`
- [x] `check-static` — **N/A** (no shipped ELF change)
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `DockerRunPlan`, `LspIoAttach`

## HOST-6 (`host6` branch)

**Status: SIGNED OFF** on `host6`. Parent is `host5` (`b46ad0f`). Do not open `host7` from this branch. Do not build clangd/tsgo/gopls/zls. Do not rebuild the runtime image except to *reference* `progressive-lsp-runtime:local`. Container control is `--mux` on the same stdio (`ServeMode::Mux`). Native Darwin open keeps `ControlSocket`. Tests never talk to a Docker daemon, registry, or AWS (`FakeRuntime` / scripted CLI / `DockerRunPlan` argv / pair mux frames). `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-6.1 | `ServeMode::Mux` + `DockerRunPlan` includes `--mux` | host5 | **SIGNED OFF.** argv `serve --prefix /opt/plsp --mux`. Darwin unit tests name the pattern. HOST-5 “never `--mux`” assertion inverted. |
| HOST-6.2 | `MuxStdio` / `MuxLsp` / `MuxControl` reuse protocol `MuxFrame` | HOST-6.1 | **SIGNED OFF.** Channel 0 = opaque JSON-RPC body (same as `serve_mux`). Channel 1 = length-prefixed Envelope. Unknown channel / >16 MiB fail closed. Pair / Cursor tests. |
| HOST-6.3 | Docs sign-off | HOST-6.2 | **SIGNED OFF.** milestones HOST-6; branching `host5 └── host6`; `host7` was future (now stacked). host-deps: container attach is mux stdio. Live mux initialize recorded in milestones (not a cargo test). |

**Sign-off checklist (HOST-6)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p poc-ide --lib -- --test-threads=1` (225 passed)
- [x] 95% llvm-cov on crates that exist — **95.95%** lines
- [x] 80% mutants on listed crates that changed — poc-ide in-diff vs `b46ad0f` **31 caught / 31 scored (100%)**, 42 unviable
- [x] No `sleep`
- [x] `check-static` — **N/A** (no shipped ELF change)
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `ServeMode::Mux`, `MuxStdio`, `ControlAttach`

## HOST-7 (`host7` branch)

**Status: SIGNED OFF** on `host7`. Parent is `host6` (`d6f8fa8`). This is the last host slice — do not open `host8`. Do not rewire attach/mux. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). No real LLVM download in crate tests. `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay (no matching CI arch winner). Skip matrix: gopls, tsgo, zls.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-7.1 | Full pack pins + `PackKind` `go`/`cached`/`cmake` | host6 | **SIGNED OFF.** 40-hex SHAs; `--pack full` allowed; slim default; unknown fail closed. |
| HOST-7.2 | Go/Zig/clangd pack jobs + `check-static` | HOST-7.1 | **SIGNED OFF.** gopls/tsgo `CGO_ENABLED=0`; zls Zig musl; clangd cache COPY (miss documented); `--cache-fill` not PR CI. |
| HOST-7.3 | Optional full packs on `xtask runtime-image` | HOST-7.2 | **SIGNED OFF.** Copy when present; omit as HOST-7 miss. No cargo/LLVM in `docker/runtime.Dockerfile`. |
| HOST-7.4 | Docs sign-off | HOST-7.3 | **SIGNED OFF.** milestones HOST-7; branching `host6 └── host7`; no host8. host-deps: full packs via Docker/cache; PR CI must not compile LLVM. clangd cache miss stays a HOST-7 gap (not closed by host-cleanup). |

**Sign-off checklist (HOST-7)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (68 passed)
- [x] 95% llvm-cov on crates that exist — **95.95%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only)
- [x] No `sleep`
- [x] `check-static` — fixture path in crate tests; live table in milestones
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `GoToolchainPin`, `PackKind` go/cached/cmake, heavy `PackBuildPlan`

## HOST-CLEANUP (`host-cleanup` branch)

**Status: SIGNED OFF** on `host-cleanup`. Parent is `fix-superhtml-x8664` (`59781cf`), stacked on signed-off `host7` (`5815fcd`). `operator-cli` (`a86f8db`) merged into this branch (`cbabe13`) so `./build` is retained — not a separate lost stash. This is a cleanup break, **not** host8. Do not open `host8`. Do not rewire attach/mux. Do not `--cache-fill` / cmake LLVM. Tests never talk to a Docker daemon, registry, or AWS (`RecordingDockerPort` / fixture bytes only). `RunLog` stays a separate schema from the serve WAL. Allocator-matrix mimalloc placeholders stay.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| HOST-C.1 | Slim `PackImageCopy` required on both triples (including superhtml × x86_64) | fix-superhtml-x8664 | **SIGNED OFF.** Fail closed if dest missing. HOST-4 omit path deleted. |
| HOST-C.2 | clangd cache miss stays honest | HOST-C.1 | **SIGNED OFF.** Full packs remain optional. Default `xtask pack` never cmake. `--cache-fill` not this slice. Cache key `3623fe661ae35c6c80ac221f14d85be76aa870f1` still absent. |
| HOST-C.3 | Zig host-native platform locked for both host ISAs | HOST-C.1 | **SIGNED OFF.** `for_pin_on_host_arch` injects `x86_64` → `linux/amd64` (native CI, no qemu) and `aarch64` → `linux/arm64`. Rust/go/cached still follow the triple. Native linux/amd64 CI was not re-run in this Darwin session. |
| HOST-C.4 | Docs + live amd64 image with superhtml | HOST-C.1 | **SIGNED OFF.** milestones HOST-CLEANUP; branching `host7 └── fix-superhtml-x8664 └── host-cleanup` (`operator-cli` merged). Live `xtask runtime-image` amd64 includes superhtml. `:local` left on native arm64. |

**Sign-off checklist (HOST-CLEANUP)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `CARGO_TARGET_DIR=target/host-cleanup cargo test -p xtask -- --test-threads=1` (87 passed; see milestones)
- [x] 95% llvm-cov on crates that exist — **N/A to lower** (xtask excluded); workspace cov not re-run on this docs/image follow-up
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docs only)
- [x] No `sleep`
- [x] `check-static` — **N/A** (no new shipped ELF; image copies existing dests). Do not run `check-static` on a Darwin Mach-O
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `RuntimeImagePlan` / `PackImageCopy` required-slim; `PackBuildPlan` host-arch injection; `XtaskCommand` / `PocArgs` CLI rows kept from `operator-cli`

## Java T3 (static pack)

**Status: WIRING LANDED; live backends are POC-JAVA.** Plan: [poc-tier-plan.md](poc-tier-plan.md), [spike/java-t3.md](../spike/java-t3.md), [t3-linux-hosts.md](t3-linux-hosts.md). Do not open `host8`. Do not ship JDT-LS or a JVM. C# stays T1/T2. Java T3: x86_64 fully static; aarch64 native-image + host libc exception.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| JAVA-T3.1 | `PackKind` + pin + Graal dockerfile; slim pack name `java`; Darwin stub | host-cleanup | **LANDED.** |
| JAVA-T3.3 | Census + factory `EngineResolver` + poc-ide catalog | JAVA-T3.1 | **LANDED.** |
| JAVA-T3.2a | Live x86_64 fully static `javacs` | JAVA-T3.1 | POC-JAVA. `check-static` pass. |
| JAVA-T3.2b | Live aarch64 native-image `javacs` (libc allowed) | JAVA-T3.1 | POC-JAVA. No `libjvm`. |
| JAVA-T3.2c | xtask: no aarch64 Java Miss/omit | JAVA-T3.2a, JAVA-T3.2b | Dest required both triples. |

## POC tier stack

**Status: POC-URI SIGNED OFF.** Master: [poc-tier-plan.md](poc-tier-plan.md). Agent: [poc-tier/agent-context.md](poc-tier/agent-context.md). Parent of `poc-uri` is current `main` (`host-cleanup` merged).

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| URI.1 | Inventory `file:` URI producers/consumers | current `main` | **SIGNED OFF.** Share `file_uri` / `path_to_file_uri`. Table in [poc-tier-plan.md](poc-tier-plan.md). |
| URI.2 | Tests: identity mount + same `rootUri` native vs container | URI.1 | **SIGNED OFF.** No daemon. Rewriter type fail-closed. |
| URI.3 | Remove any rewriter/split | URI.2 | **SIGNED OFF.** No mapper. Duplicate poc-ide codec now wraps core. |
| T2-COV.1 | C / C++ heuristic T2 | POC-URI signed off | **SIGNED OFF.** `#include`, name/arity. |
| T2-COV.2 | Rust / Python heuristic T2 | T2-COV.1 | **SIGNED OFF.** TSG opt-in. |
| T2-COV.3 | CSS / HTML heuristic T2 | T2-COV.2 | **SIGNED OFF.** Conformance T2 leaves N/A. |
| IMG.1 | `javacs` required on both runtime-image triples | JAVA-T3.2c | |
| IMG.2 | aarch64 image glibc userspace for `javacs` | IMG.1 | x86_64 scratch OK. |
| IMG.3 | Image plan tests + live tag proof | IMG.2 | |
| REST.1 | Dogfood image includes tsgo, gopls, zls | POC-IMG | Both ISAs. |
| REST.2 | clangd both ISAs or honest miss | REST.1 | No `.so`; no default-PR cmake. |
| REST.3 | Rust Linux sysroot honesty in container | REST.1 | |
| REST.4 | Live POC proof notes | REST.1 | Not a cargo test. |

## T2 heuristic coverage (every v1 language)

**Status: SIGNED OFF** on `t2-coverage`. [t2-heuristic-coverage.md](t2-heuristic-coverage.md). Parent is `poc-uri`. Do not open `java-t3` until this table stays signed off.

## T3 Linux hosts

**Status: LOCKED IN DOCS.** [t3-linux-hosts.md](t3-linux-hosts.md). Live work is POC-JAVA / POC-IMG / POC-REST, not a separate T3-HOST.1 duplicate. clangd is REST.2.

## `./build lsp` freshness

**Status: IMPLEMENTED.** Make/cmake semantics: **Fresh** (dest exists + input stamp matches) skips; **Stale** / **Missing** (dest missing or inputs changed) rebuilds that artifact. Default `./build lsp all` rebuilds revised code / pins / dockerfiles / `Cargo.lock` and skips unchanged dests. `--force` treats every artifact as stale. Tests inject `RecordingDockerPort`; no daemon.

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| LSP-FRESH.1 | `LspArtifactStamp` / `Freshness` / `LspFlags`; parse `./build lsp {arch} [--force]` | operator-cli | Parse tests; stamp equality; Fresh skips, Stale/Missing rebuild; no docker. |
| LSP-FRESH.2 | Per-artifact rebuild: musl core, each pack, runtime image | LSP-FRESH.1 | Unchanged inputs skip that dest; revised inputs rebuild it; `--force` rebuilds all. |

## HOST-3 sign-off recap (do not reopen)

**Sign-off checklist (HOST-3)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (47 passed)
- [x] 95% llvm-cov on crates that exist — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only; xtask is not on the 80% list)
- [x] No `sleep`
- [x] `check-static` — crate tests use fixture ELFs. Live dest ELFs: **10/10 PASS** (ty, rust-analyzer, phpantom, biome, superhtml both triples). `superhtml` × x86_64 qemu miss closed later via Zig cross-compile — not a Mach-O green.
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `PackPin`, `PackKind`, `PackBuildPlan`, `RustToolchainPin`, `ZigToolchainPin`; `DockerPort` extract row updated

## HOST-2 sign-off recap (do not reopen)

**Sign-off checklist (HOST-2)**

- [x] Exit criteria for this WP met
- [x] Tests on this branch — `cargo test -p xtask -- --test-threads=1` (37 passed)
- [x] 95% llvm-cov on crates that exist — **96.00%** lines
- [x] 80% mutants on listed crates that changed — **N/A** (xtask / docker / docs only; xtask is not on the 80% list)
- [x] No `sleep`
- [x] `check-static` — fixture path in crate tests; real extracts **PASS** `target/musl/aarch64-unknown-linux-musl/progressive-lsp` and `target/musl/x86_64-unknown-linux-musl/progressive-lsp` (orchestrator proof, not a cargo test). Do not commit those ELFs.
- [x] Docs in this tree updated
- [x] [design-patterns.md](design-patterns.md) — `MuslBuildPlan`, `DockerPort`, `CommandDockerPort`, `RecordingDockerPort`

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
5. Stop at sign-off; do not start the next milestone branch (`pdN+1` until `pdN` signed off; `ideN+1` until `ideN` signed off; `logN+1` until `logN` signed off; `hostN+1` until `hostN` signed off).
6. POC orchestrators: pass [poc-ide/agent-context.md](poc-ide/agent-context.md) unchanged to every child.
7. LOG orchestrators: pass [logging/agent-context.md](logging/agent-context.md) unchanged to every child. Stack `log0` on current `main`, not `poc-no-console`. Parent of `log5` is `log4`. Do not reopen LOG-0–LOG-5.
8. POC-proof orchestrators: pass [poc-ide/proof-agent-context.md](poc-ide/proof-agent-context.md) unchanged to every child. Stack `poc-proof-log` on current `main` (after log11 merge), not on `log11` history. The POC-proof stack is complete at `poc-no-stall`. Do not reopen POC-proof WPs. The allowed next stack is `host0`.
9. HOST orchestrators: pass [host/agent-context.md](host/agent-context.md) unchanged. `host7` is the last numbered host slice — do not open `host8`.
10. POC-tier orchestrators: pass [poc-tier/agent-context.md](poc-tier/agent-context.md) unchanged. Stack `poc-uri` on current `main` (`host-cleanup` merged). POC-T2 is signed off on `t2-coverage`. Do not start `java-t3` until that stays signed off. Order: `poc-uri` → `t2-coverage` → `java-t3` → `t3-image` → `t3-rest`. Plan: [poc-tier-plan.md](poc-tier-plan.md).
