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

## Rules

1. **Scope:** a branch contains that milestone’s work packages only ([implementation-plan.md](implementation-plan.md)). No extra language packs on `m1`.
2. **Sign-off before stacking the next branch:** [milestones.md](milestones.md) exit **and** [testing.md](testing.md) hygiene (95% on crates that exist; 80% mutants on listed crates for that milestone; no `sleep`; `check-static` if a bin changed).
3. **Tests are not deferred to `m6`.** Write them on the milestone branch.
4. **Merge to `main`:** sequential (`docs-0`, then `m0`, …) or rebase the stack. Either way **do not open `mN+1` until `mN` is signed off**.
5. **Spikes:** `spike/*` or notes. A spike that ships `DT_NEEDED` does not merge. Hygiene applies on merge to a milestone branch.
6. **Docs drift:** if implementation must change a locked decision, update `docs/` on the same branch and keep the set internally consistent.

## Suggested branch names

`docs-0`, `m0`, `m1`, `m2`, `m3`, `m4`, `m5`, `m6` stacked as above. Feature slices inside a milestone may be stacked on that milestone (`m1-watch`, `m1-java`) but must merge back to `m1` before `m2` starts.
