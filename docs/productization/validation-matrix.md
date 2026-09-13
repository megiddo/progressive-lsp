# Validation matrix (PROD-5)

Tiered checks for laptop POC and release gates. No `--cache-fill` in PR CI.

## Smoke tier 0 (automated)

```sh
./scripts/smoke-tier0.sh
```

| Check | Command / gate | CI |
|---|---|---|
| xtask unit | `cargo test -p xtask -- --test-threads=1` | PR |
| poc-ide unit | `cargo test -p poc-ide` | PR |
| Docker daemon | `cargo xtask smoke` → `docker info` | manual / nightly |
| Clangd pull | `cargo xtask smoke` → [verify-clangd-cache-pull.sh](../../scripts/verify-clangd-cache-pull.sh) | manual (needs local-artifacts) |

## Smoke tier 1 (container serve)

| Check | Bar |
|---|---|
| Image present | `docker image inspect progressive-lsp-runtime:local` |
| Open folder in container | `./build run ide --folder <fixture>` (non-Linux default container) |
| T1 attach | Journal shows serve ready; tree populates |
| One slim T3 | e.g. Python fixture with `ty` in image |

**Fixture dirs:** TBD under `integration/fixtures/` (PROD-5.2).

## Dogfood language matrix (manual until automated)

| Language | T1 | T2 | T3 engine | Gate |
|---|---|---|---|---|
| C/C++ | yes | heuristics | **clangd** musl static | REST.2 dests PASS; image copy PROD-3 |
| Java | yes | heuristics | **javacs** | aarch64 libc exception documented |
| Rust | yes | heuristics | **rust-analyzer** | sysroot honesty REST.3 |
| Python | yes | heuristics | **ty** | slim pack |
| TypeScript | yes | heuristics | **tsgo** | full pack |
| Go | yes | heuristics | **gopls** | full pack |
| Zig | yes | heuristics | **zls** | full pack |

Native **Open Folder…** (T1/T2 only): optional; must not be default on non-Linux.

## Release / nightly bar

| Event | Runs |
|---|---|
| PR | tier 0 unit only |
| Pre-release | tier 0 + verify-clangd-cache-pull + `./build lsp all --flavor dogfood` on maintainer host |
| Post GitHub engine release | pull clangd from HTTPS manifest on clean `pack-cache` |

See [integration/README.md](../../integration/README.md) for heavy IT harness scope.
