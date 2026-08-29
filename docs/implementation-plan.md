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

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M0.1 | Cargo workspace, `rust-toolchain.toml`, tiny bin `main` | D0 | Composition root only |
| M0.2 | `progressive-lsp-core`: ids, errors, `ClockPort`, prefix | M0.1 | FakeClock tests |
| M0.3 | `PluginRegistry` + empty `LanguageFactory` slots | M0.2 | `UnsupportedLanguage` tests |
| M0.4 | `progressive-lsp-protocol`: initialize/shutdown | M0.3 | experimental cap, socket null OK |
| M0.5 | proto + `progressive-lsp-control` codec | M0.2 | round-trip; empty RPCs OK |
| M0.6 | `.progressivelsp` layout + git exclude helper | M0.2 | fixture repo; never edit project `.gitignore` |
| M0.7 | `progressive-lsp-install`: LocalFs, hash, manifest schema | M0.2 | no network |
| M0.8 | `xtask musl`, `check-static`, Docker stub | M0.1 | both arches |
| M0.9 | `xtask bench-alloc` + `allocator-matrix.toml` placeholders | M0.8 | mimalloc until CI rows |
| M0.10 | Spike notes under `spike/` (glibc-static, csharp-ls, PHP T3, …) | M0.8 | notes; fail closed, do not ship `DT_NEEDED` |

## M1 (`m1` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M1.1 | `WatchCoalescer` + FakeWatcher | M0 signed | 10k → 1 batch |
| M1.2 | FilesSince + overflow/`truncated` | M1.1, M0.5 | control plane |
| M1.3 | Incremental Tree-sitter + dirty-set priority | M1.1 | ~10 ms class |
| M1.4 | `WatchFilter` identity | M1.1 | |
| M1.5 | `progressive-lsp-lang-java` T1 + tokens | M1.3 | no JDK |
| M1.6 | Directory + Maven/Gradle/Eclipse adapters | M1.5 | multi-package fixture |
| M1.7 | Server-side `notify` ghost edit reindex | M1.1, M1.5 | no progressive client required |

## M2 (`m2` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M2.1 | Package-stream ingest + `workDoneProgress` + `data.tier` | M1 | |
| M2.2 | Control `TierReady` | M2.1, M0.5 | |
| M2.3 | Java T2 heuristics; optional TSG eval | M2.1, M1.5 | keep TSG only if it wins |
| M2.4 | PHP T1/T2 + Composer adapter | M2.1 | no interpreter |
| M2.5 | HTML/CSS/JS T1 | M2.1 | |
| M2.6 | Go T1 + `go.mod`; Zig T1 + `build.zig` | M2.1 | no gopls/zls yet |
| M2.7 | Rhai `ScriptHost` + catalog subset | M0.2 | sandbox + Abort tests |

## M3 (`m3` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M3.1 | `EngineSupervisor` + `EngineAdapter` trait | M2 | crash/backoff tests |
| M3.2 | Pack discovery `$PREFIX/engines/` | M3.1, M0.6 | |
| M3.3 | ty pack + Python T3 handoff | M3.1 | T1 (optional TSG T2) without pack |
| M3.4 | rust-analyzer pack + Rust T3 | M3.1 | no pack / no sysroot → T1 (no dedicated Rust T2) |
| M3.5 | `on_engine_spawn` / `on_tier_ready` | M2.7, M3.1 | |

## M4 (`m4` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M4.1 | clangd pack + compile_commands adapter | M3.1 | slim dist default excludes |
| M4.2 | csharp-ls AOT or matrix T2 ceiling | M3.1 | spike fail → document |
| M4.3 | oxc T2 + tsgo T3 | M3.1 | no Node |
| M4.4 | PHP T3 pack (spike winner) | M3.1, M2.4 | |
| M4.5 | superhtml + biome | M3.1 | |
| M4.6 | gopls + zls | M3.1, M2.6 | degrade without project toolchain |

## M5 (`m5` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M5.1 | Content-addressed index cache | M2.1 | never in git worktree |
| M5.2 | LATEST+2 fixtures + mixed workspace | M4 languages as landed | [language-matrix.md](language-matrix.md) |
| M5.3 | Burst + FilesSince overflow catch-up | M1.2 | 10k files budget |
| M5.4 | Lag fixtures (newer syntax, no panic) | M5.2 | |
| M5.5 | RSS / p99 gates recorded | M5.1 | T3 not charged to core |

## M6 (`m6` branch)

| ID | Work package | Depends-on | Notes |
|---|---|---|---|
| M6.1 | `xtask dist` tarballs + SHA256 + slim/full | M0.8, M4 packs | |
| M6.2 | Install CLI + `on_install_verify` | M0.7, M2.7 | FakeTransport |
| M6.3 | Refresh control/lsp/plugin/consumer docs vs impl | M6.1 | docs remain source of truth |
| M6.4 | Conformance dashboard | M5.2 | per language per tier |
| M6.5 | Versioning: core semver vs engine SHAs | M6.1 | proto stays `v1` |

## Spikes (do not skip hygiene on merge)

| Spike | Lives | Merge rule |
|---|---|---|
| glibc-static POC | `spike/glibc-static` | fold into M0 xtask only if `check-static` clean |
| csharp-ls AOT+musl | `spike/` notes | fail → C# T2 ceiling in matrix |
| PHPantom vs static phpactor | `spike/` | no host `php`; no Node |
| clangd static archive graph | notes | miss → document; do not ship `.so` |

## Agent instructions

1. Read [README.md](README.md), then the docs for your WP.
2. Implement only that WP’s crates/files.
3. Map new types in [design-patterns.md](design-patterns.md).
4. Do not add Node/JVM/CPython, `$/` FilesSince, or SSH in the install crate.
5. Stop at sign-off; do not start the next milestone branch.
