# Branching

One git branch per milestone, **stacked**. Do not dump all milestones on `main` in one PR.

```text
main
  └── docs-0          # documentation only
        └── m0        # skeleton, musl, proto, install schema
              └── m1  # watch, FilesSince, Java T1
                    └── m2
                          └── m3
                                └── m4
                                      └── m5
                                            └── m6
```

v1 merged to `main`. Post-dev stack starts from **current `main`**, not from `m6`:

```text
main
  └── pd0             # ingest user docs, integration designs, T2 spike notes
        └── pd1       # IT-1 deploy/config (containers)
              └── pd2 # IT-2 vanilla LSP corpora
                    └── pd3  # IT-3 progressive.v1
                          └── pd4  # T2 Strategy seam + bake-off
```

POC IDE stack starts from **current `main`**, not from `pd4`:

```text
main
  └── ide0            # POC IDE docs (architecture, OSS pins, patterns, WPs)
        └── ide1      # shell: open folder/file, tree, tabs, resizable panel
              └── ide2  # buffers, EditCommand, save, syntect
                    └── ide3  # DiskWatch + ConflictModal
                          └── ide4  # LanguageCatalog + stock LSP discovery
                                └── ide5  # ProtocolConsole + progressive.v1

main   # after IDE-5 merge
  └── poc-log         # per-run sqlite debug log (not IDE-6)
        └── poc-tree-lazy  # shallow FileTree load (not IDE-6)
              └── poc-tree-collapsed  # TreeExpansion default collapsed (not IDE-6)
                    └── poc-compact-folders  # compact a/b/c single-child dirs (not IDE-6)
                          └── poc-context-menu  # editor context menu for definition / implementation / references (not IDE-6)
                                └── poc-navigate  # deferred Navigate + editor caret sync (not IDE-6)
                                      └── poc-no-console  # drop the hand-typed protocol console; debug is RunLog (not IDE-6)
                                            └── poc-dialog-defer  # File Open Folder/File after the menu closes (not IDE-6)
                                                  └── poc-open-unblock  # non-recursive watch + deferred LSP initialize (not IDE-6)
                                                        └── poc-tree-sort  # dirs then files; dot names last (not IDE-6)
                                                              └── poc-discover-log  # discover uri/position/count in RunLog (not IDE-6)

main   # after poc-discover-log merge (current main)
  └── log0            # global logging docs (not a crate)
        └── log1      # LogPort + records (no sqlite in server)
              └── log2  # WAL repository crate
                    └── log3  # Facades, bridges, eprintln death
                          └── log4  # Wire serve/install + docs lock (SIGNED OFF; parent of log5)
                                └── log5  # remaining-coverage docs ingest
                                      └── log6  # supervisor + ScriptHost lifecycle emits
                                            └── log7  # protocol + control socket + install hash
                                                  └── log8  # T3 skip + session completeness
                                                        └── log9  # durable WAL fallback (SIGNED OFF)
                                                              └── log10 # child capture (FakeChildStderr; ready when spawn exists)
                                                                    └── log11 # operational Err hygiene gate
```

LOG-0–LOG-11 are **signed off**. Stack complete at `log11`. Do not reopen them. Parent of `log11` is `log10`. There is no `log12`. **Supersedes** “stack complete at `log4` / do not open `log5`.”

POC-proof stack starts from **current `main`** (after the log11 merge), not from `log11` or `poc-discover-log` history:

```text
main   # after log11 merge
  └── poc-proof-log
        └── poc-lsp-async
              └── poc-tier-status
                    └── poc-no-stall
                          └── host0          # native vs container File menu; T3HostOffer; RuntimePort
                                └── host1    # PackAdapter Linux Command spawn
                                      └── host2  # musl core ELF extract both triples
                                            └── host3  # slim pack jobs both triples
                                                  └── host4  # runtime image
                                                        └── host5  # docker run attach
                                                              └── host6  # mux client
                                                                    └── host7  # full flavor packs (last numbered host slice)
                                                                          └── fix-superhtml-x8664  # zig qemu faccessat
                                                                                └── host-cleanup  # require superhtml; operator-cli merged; not host8
```

## Rules

1. **Scope:** a branch contains that milestone’s work packages only ([implementation-plan.md](implementation-plan.md)). No extra language packs on `m1`.
2. **Sign-off before stacking the next branch:** [milestones.md](milestones.md) exit **and** [testing.md](testing.md) hygiene (95% on crates that exist; 80% mutants on listed crates for that milestone; no `sleep` in the **unit** suite; `check-static` if a bin changed). Integration tests ([../integration/README.md](../integration/README.md)) are a separate harness (containers/deadlines allowed there only).
3. **Tests are not deferred to `m6`.** Write them on the milestone branch.
4. **Merge to `main`:** sequential (`docs-0`, then `m0`, …) or rebase the stack. Either way **do not open `mN+1` / `pdN+1` / `ideN+1` / `logN+1` / `hostN+1` until the previous is signed off**. `log5` may start only after `log4` is signed off (it is). `host1` may start only after `host0` is signed off. `host2` may start only after `host1` is signed off. `host3` may start only after `host2` is signed off. `host4` may start only after `host3` is signed off. `host5` may start only after `host4` is signed off. `host6` may start only after `host5` is signed off. `host7` may start only after `host6` is signed off.
5. **Spikes:** `spike/*` or notes. A spike that ships `DT_NEEDED` does not merge. Hygiene applies on merge to a milestone branch.
6. **Docs drift:** if implementation must change a locked decision, update `docs/` on the same branch and keep the set internally consistent.

## Suggested branch names

`docs-0`, `m0`–`m6` as above (v1). Post-dev: `pd0`–`pd4` stacked on `main` (merged). POC IDE: `ide0`–`ide5` stacked on current `main`. Logging: `log0`–`log4` signed off; `log5`–`log11` stacked on `log4` (current `main`, not `poc-no-console`). POC-proof: `poc-proof-log` → `poc-no-stall` signed off. Host stack: `host0` on `poc-no-stall`; `host1` on `host0`; `host2` on `host1`; `host3` on `host2`; `host4` on `host3`; `host5` on `host4`; `host6` on `host5`; `host7` stacked on `host6` (last numbered host slice). `fix-superhtml-x8664` stacks on `host7`; `host-cleanup` stacks on `fix-superhtml-x8664`. `operator-cli` (`a86f8db`) merged into `host-cleanup` (`cbabe13`) so `./build` is retained — not a separate lost stash. Do not open a follow-on host8. Feature slices inside a milestone may be stacked on that milestone (`ide1-tree`, `ide1-layout`) but must merge back to `ide1` before `ide2` starts.
