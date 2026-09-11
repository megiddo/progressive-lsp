# POC-REST live proof notes (orchestrator)

Not a `cargo test`. Records dogfood image / pack proof after POC-JAVA / POC-IMG.

## REST.2 clangd (required)

**Code (signed off on `t3-rest`):** `RuntimeImagePlan` / `PackImageCopy` require `clangd` on both triples; `freshness` seeds full dogfood dests in unit tests.

**Live ELF (pending):** populate cache then extract:

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo xtask pack --pack clangd --cache-fill --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --cache-fill --target aarch64-unknown-linux-musl
cargo xtask pack --pack clangd --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --target aarch64-unknown-linux-musl
cargo xtask check-static target/musl/x86_64-unknown-linux-musl/engines/clangd/clangd
cargo xtask runtime-image --both
```

**2026-09-11 session:** x86_64 `--cache-fill` ran in Docker; host-stage `ninja` hit **OOM** (`g++: fatal error: Killed`) around object ~1650/2962 with default parallelism. `docker/engine-pack-clangd-cache-fill.Dockerfile` now sets `NINJAFLAGS=-j2`. Re-run cache-fill locally (or raise Docker Desktop memory) before claiming REST.2 live proof.

Dest paths stay gitignored under `target/musl/<triple>/engines/` and `target/pack-cache/clangd/<sha>/<triple>/`.

## Hygiene / environment notes

| Gate | Result |
|---|---|
| `cargo test -p xtask -- --test-threads=1` | **PASS** (113 tests; `CARGO_TARGET_DIR=$PWD/target`) |
| Full workspace `cargo test -- --test-threads=1` | **Cursor sandbox:** `serve_host::tests::initialize_merges_overlay_and_excludes_without_editing_gitignore` fails (` .git/hooks/: Operation not permitted` in temp repo). Run the full suite on the host outside the agent sandbox for a green. |
| `cargo llvm-cov` / `cargo mutants` on `xtask/` | **N/A** per [testing.md](../testing.md) (xtask excluded from 95% / mutants denominator). |

## REST.3 Rust sysroot

Container ships **rust-analyzer** only. A Darwin rustc sysroot does **not** satisfy RA in Linux container. Tier strip uses `rust_degrade_reason` (`no rustc sysroot; T2 then T1`) — see `progressive-lsp-lang-rust`. Project `sysroot/` under the opened workspace is still required for Rust T3.

## REST.4 scenarios

| Scenario | Bar | Proof |
|---|---|---|
| Java tree → T3 | `javacs` in image; aarch64 glibc base | POC-JAVA live pack + POC-IMG `progressive-lsp-runtime:local` ([spike/java-t3.md](../../spike/java-t3.md)) |
| One other slim language | static slim ELF in image | `python`/`ty` staged with image |
| One full-pack language | tsgo / gopls / zls required dest | `RuntimeImagePlan` fail-closed if missing |
| C/C++ T3 | static `clangd` in image | REST.2 above (live pending cache-fill) |
