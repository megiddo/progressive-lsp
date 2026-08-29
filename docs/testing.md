# Testing and hygiene

Gates apply from the **first library crate** (M0). Sign-off: [implementation-plan.md](implementation-plan.md).

## Line coverage — 95%

```text
cargo llvm-cov --workspace --fail-under-lines 95 \
  --ignore-filename-regex 'xtask/|/src/main\.rs$|tree-sitter'
```

**Exclude from the denominator:** `xtask/`; bin `main.rs` (keep mains tiny: parse CLI, call lib); vendored Tree-sitter C / generated grammar (`tree-sitter` in the path); unmodified `third_party/stack-graphs`; engine pack **source we do not own**.

Empty Factory slots still count: tests that they return `UnsupportedLanguage` without panic.

## Mutation — 80% kill rate

`cargo-mutants` on crates that **exist** in the milestone:

- Always when present: `progressive-lsp-core`, `-control`, `-install`, `-watch`, `-index`, `-resolve`, `-script`, `-workspace`
- Language crates once they have real resolvers (not empty slots)
- `progressive-lsp-engine`: supervisor crash/backoff/hash/discovery only

Do not mutation-test clangd, ty, or rust-analyzer **source we do not own**. Engine wrappers: supervisor crash/backoff/hash/discovery only.

## Time and threads

- **No** `thread::sleep`, **no** wall-clock polling.
- Inject `ClockPort` / `FakeClock` / `FakeWatcher` / `FakeTransport`.
- Suite must pass `cargo test -- --test-threads=1`.

## Allocator bake-off (`xtask bench-alloc`)

musl default malloc is **unacceptable**. Go/Zig/C# keep their own heaps (not in the matrix). No tcmalloc.

**Pick rule — one method, many cells.** `xtask dist` **only** reads `xtask/allocator-matrix.toml`.

1. **Cell:** `(backend, arch, libc)`  
   - `backend` ∈ {`core`, `clangd`, `rust-engine`} (`rust-engine` = RA / ty / biome / oxc / PHPantom unless a pack diverges)  
   - `arch` ∈ {`x86_64`, `aarch64`}  
   - `libc` ∈ {`musl`, `glibc-static`}  
   - **Skip:** gopls, tsgo, zls, superhtml, csharp-ls
2. **Candidates** that pass `check-static` on that cell: musl mallocng (musl only), glibc ptmalloc (glibc-static only), mimalloc, jemalloc. Fail static → not a candidate.
3. **Bench:** pinned index-shaped fixture, ≥4 threads, fixed seed. Primary: **p99 wall time**. Secondary: **peak RSS**. Record **only** from the matching CI arch job (no laptop numbers).
4. **Winner:** lowest p99. If another is within **5% p99 and 5% RSS**, **tie** → mimalloc, then jemalloc, then libc default.
5. Empty cell → placeholder **mimalloc**. Re-run when allocator crate versions or the fixture change (nightly / dep bump, not every PR).
6. snmalloc may be added later; the rule does not change.

## Static check

`xtask check-static` fails if a shipped ELF has a dynamic interpreter **or** any `DT_NEEDED`. Same bar for musl and glibc-static. Go packs: `CGO_ENABLED=0`.

## Performance budgets (publish numbers in M5 benches)

| Path | Target | M5 recorded |
|---|---|---|
| Open-buffer Tree-sitter reparse | ~10 ms class | **71 µs** Darwin aarch64 sample (`xtask bench-perf`). CI required: still ~10 ms class on the matching arch. |
| T1/T2 `definition` p99 after index | &lt; 50 ms | **9 µs** Darwin aarch64 sample. CI required: &lt; 50 ms on the matching arch. |
| T2 debounce | 50–100 ms (FakeClock) | unchanged (FakeClock; no `thread::sleep`) |
| 10k watch events | 1 coalesced batch | 10k FakeWatcher events → 1 batch; overflow path sets `truncated` then FilesSince catch-up |
| Core RSS without engines | recorded; engines not charged to core | **~4.0 MiB** peak sample during `bench-perf` (4 194 304 bytes after / 1 966 080 before). Darwin `ps` RSS. **T3 not charged.** |
| External-edit burst (10k files) | published budget, M5 | **2212 µs** (~2.2 ms) Darwin aarch64 sample to inject+coalesce 10k events (FakeClock). Gate in tests: &lt; 2 s. Overflow+catch-up gate: &lt; 5 s. |

**Methodology vs CI-arch allocator winners**

- These rows are **Darwin host samples** from `xtask bench-perf` (macos/aarch64, Homebrew rustc). They are **not** `xtask bench-alloc` cell winners.
- [Allocator bake-off](#allocator-bake-off-xtask-bench-alloc) still records winners **only** from the matching CI arch job. Empty cells stay mimalloc placeholders. Do not copy laptop RSS/p99 into `allocator-matrix.toml`.
- Linux CI must re-run the same benches/fixtures on `x86_64`/`aarch64` × `musl`/`glibc-static` before treating a number as a matrix winner.
- `cargo test` on Darwin is the stand-in for “matrix CI green.” Linux CI must run `fixtures/matrix/` and `fixtures/lag/` on the matching arch.

T3 latency is per-engine and not a core fail if ty is slow.

## Script sandbox tests

- Ops limit / string cap → error, no I/O.
- `allow_shell` default false.
- Abort on `on_pre_index` skips that package; Abort on `on_watch` drops paths; Abort on `on_bootstrap` fails initialize.
- Abort on `on_engine_spawn` skips that engine (T1/T2 remain). `on_tier_ready` Abort cannot unwind intelligence.
- Abort on `on_install_verify` refuses the new binary (no rename, no exec).
- Scripts cannot register `textDocument/definition`.

## Integration suite (PD1–PD4)

Separate from this unit/mutation gate. Spec: [../integration/README.md](../integration/README.md).

- May use containers, real engines, and a **deadline** wait on `workDoneProgress` / `TierReady` (poll protocol, not `sleep(5)`).
- Must not weaken the crate suite: still no `thread::sleep` in `crates/` tests.
- Nightly / release, not every library PR.
- Darwin: do not treat missing musl ELFs as a green IT-1. `integration/harness/run-it1.sh` host_smoke is not IT-1.1; Linux CI + `compose.yaml` is the distro gate.
- IT-2 may wait on `workDoneProgress` with a deadline (poll protocol, not `sleep(5)`). T3 rows are `skip_pack_missing` when packs are Darwin stubs — not typed-hover greens.
- IT-3 may deadline-poll the control socket for `WatchBatch` / `TierReady`. `--mux` is `pending_mux` if unimplemented. T3 types rows stay `skip_pack_missing` on Darwin stubs.
- PD4 T2 bake-off may fetch the stack-graphs pin and junit4 at SHA (cache under `target/`). Default T2 stays heuristic unless the winner rule fires. Do not treat Darwin RSS as a musl green.

## Milestone sign-off (every WP)

- [ ] Tests for this WP live on this branch
- [ ] llvm-cov 95% on crates that exist
- [ ] mutants 80% on listed crates that exist
- [ ] no `sleep`
- [ ] `check-static` if a shipped ELF changed
- [ ] pattern table updated if a type was added
- [ ] exit row in [milestones.md](milestones.md)

**Docs-0:** tests, llvm-cov, mutants, `sleep`, and `check-static` are **N/A** (no crates). Do not invent tests to fill this checklist.
