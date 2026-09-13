# POC-REST live proof notes (orchestrator)

Not a `cargo test`. Records dogfood image / pack proof after POC-JAVA / POC-IMG.

## REST.2 clangd (required)

**Code (signed off on `t3-rest`, fixes on `main` via PR #12):** `RuntimeImagePlan` / `PackImageCopy` require `clangd` on both triples; `freshness` seeds full dogfood dests in unit tests.

**Live ELF — musl dests + check-static (PASS, human 2026-09-12):** `scripts/clangd-overnight.sh` on `main` completed cache-fill both triples, pack extract, and `check-static` for:

- `target/pack-cache/clangd/3623fe661ae35c6c80ac221f14d85be76aa870f1/{x86_64,aarch64}-unknown-linux-musl/clangd`
- `target/musl/{x86_64,aarch64}-unknown-linux-musl/engines/clangd/clangd`

**Live image copy (PROD-3 / POST-PROOF.2):** run after **all** dogfood pack dests exist (not clangd alone):

```sh
export CARGO_TARGET_DIR="$PWD/target"
./build lsp "$(uname -m | sed 's/arm64/aarch64/')" --flavor dogfood
cargo xtask runtime-image --both
docker images progressive-lsp-runtime:local
```

Record image ID/digest below when built. (Orchestrator 2026-09-13: `runtime-image --both` fail-closed on missing `aarch64` slim `java/javacs` — full dogfood `./build lsp` required first.)

**Image (PASS 2026-09-13):** `progressive-lsp-runtime:local` image ID **`7bd38315eb76`** (142MB content size; `./scripts/prod3-dogfood-and-image.sh`).

Maintainer re-run (only if pins or Dockerfile change):

```sh
export CARGO_TARGET_DIR="$PWD/target"
./scripts/clangd-overnight.sh 2>&1 | tee "target/clangd-overnight-logs/docker-$(date +%Y%m%d-%H%M%S).log"
```

Or manual steps:

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo xtask pack --pack clangd --cache-fill --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --cache-fill --target aarch64-unknown-linux-musl
cargo xtask pack --pack clangd --target x86_64-unknown-linux-musl
cargo xtask pack --pack clangd --target aarch64-unknown-linux-musl
cargo xtask check-static target/musl/x86_64-unknown-linux-musl/engines/clangd/clangd
cargo xtask check-static target/musl/aarch64-unknown-linux-musl/engines/clangd/clangd
cargo xtask runtime-image --both
```

**Failure history (orchestrator notes):**

- **2026-09-11:** x86_64 musl **OOM** during `ninja clangd`. Dockerfile caps **`NINJAFLAGS=-j2`** (override `NINJAFLAGS=-j1` if still OOM).
- **2026-09-12 build-host:** host tblgen cmake must use **`clang`/`clang++`** — Debian `cc` rejects LLVM’s `-Wcovered-switch-default` / `-Wstring-conversion` on `regcomp.c` (see `target/clangd-overnight-logs/docker-20260911-225432.log`; fixed on `main` in PR #12).
- **Musl link (PR #12):** musl stage uses host `clang-tidy-confusable-chars-gen`, **`clang++ --target=*-linux-musl -static`**, not `g++` + glibc libstdc++.a.

### Overnight run (when Docker is resourced)

1. **Docker Desktop → Settings → Resources:** **≥ 12 GiB RAM** (16 GiB safer). **≥ 80 GiB disk** free. Quit other heavy containers.
2. **Branch:** current `main` (or branch with the same `docker/engine-pack-clangd-cache-fill.Dockerfile`). Repo root = Google Drive checkout.
3. **Kick off** one command (logs under `target/clangd-overnight-logs/`):

```sh
mkdir -p target/clangd-overnight-logs
./scripts/clangd-overnight.sh 2>&1 | tee "target/clangd-overnight-logs/docker-$(date +%Y%m%d-%H%M%S).log"
```

If musl `ninja` OOMs: `export NINJAFLAGS=-j1` before the script. Expect **several hours per triple** for cache-fill; **do not** run two cache-fills in parallel.

4. **After exit 0:** optional `cargo xtask runtime-image --both` (PROD-3); paste image digests below.

Dest paths stay gitignored under `target/musl/<triple>/engines/` and `target/pack-cache/clangd/<sha>/<triple>/`.

## Hygiene / environment notes

| Gate | Result |
|---|---|
| `cargo test -p xtask -- --test-threads=1` | **PASS** (113+ tests; `CARGO_TARGET_DIR=$PWD/target`) |
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
| C/C++ T3 | static `clangd` in dogfood image | **PASS** musl dests (2026-09-12); **PASS** runtime image (2026-09-13, `7bd38315eb76`) |
