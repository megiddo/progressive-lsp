# progressive-lsp

**Language intelligence for many languages on Linux** — go-to-definition, references, hover, symbols, and highlighting — as one Language Server Protocol process.

It is **not** an IDE, SSH, git, a file tree, or a terminal. Point your editor at `progressive-lsp serve` (stdio). One static Linux binary; optional engine packs add richer types.

**End users:** [docs/user/README.md](docs/user/README.md). Progressive hosts: [docs/user/progressive-v1-api.md](docs/user/progressive-v1-api.md).

**Design and milestones:** [docs/README.md](docs/README.md). POC IDE: [docs/poc-ide/README.md](docs/poc-ide/README.md). Branch stack: [docs/branching.md](docs/branching.md).

## Use & development

Everything maintainer-facing goes through **`./build`** at the repo root (wrapper around `cargo xtask`). Run `./build help` for flags; `./build help lsp` and `./build help run` for topics.

**Prerequisites:** Rust toolchain, Docker Desktop when you need the Linux runtime image or integration smoke (typical on macOS/Windows). Workspace artifacts live under `target/` (gitignored); `./build` compiles xtask into `target/operator/` so it does not fight rust-analyzer for the main `target/` lock.

### Two build phases

| Phase | What | When you need it |
|---|---|---|
| **1 — progressive-lsp (Rust)** | Native `progressive-lsp` + poc-ide; unit tests | Daily Rust work, T1/T2 on the host |
| **2 — engine packs + runtime image (Docker)** | Musl server, third-party LSP engines (clangd, tsgo, …), `progressive-lsp-runtime:local` | Container poc-ide, full T3, `./build integ` |

You can stay in phase 1 for a long time. Phase 2 is where compile time and disk go up — especially **clangd** (cached binary or local `--cache-fill`, not cmake on every `./build lsp`).

**Phase 1 commands**

```text
./build test          # workspace + harness tests (uses target/ and .cargo-home/)
./build ide           # poc-ide + native progressive-lsp binary (no UI)
./build run ide --native --folder /abs/path   # optional T1/T2-only on non-Linux
```

**Phase 2 commands**

```text
./build lsp aarch64                              # slim packs + musl core + runtime image
./build lsp aarch64 --flavor dogfood             # slim + full packs (C/C++, TS, Go, Zig T3)
./build run ide --smoke                          # Java fixture; builds image if missing
./build run ide --folder /abs/path/to/project    # container on macOS/Windows by default
./build integ                                      # hermetic integration smoke in Docker
```

**Pack flavors (`./build lsp`)**

- **`slim` (default)** — python, rust, java, php, html/css engines, etc. Enough for many fixtures and slim runtime images.
- **`dogfood`** — slim plus **full** packs: clangd, tsgo, gopls, zls. Required for the dogfood runtime image and for C/C++/TS/Go/Zig T3 in container mode. Dogfood tries to **pull** a pinned clangd cache before failing closed; see [docs/consumer.md](docs/consumer.md).

Freshness is stamp-based: unchanged sources skip rebuilds. Pass **`--force`** to rebuild musl core, packs, and the image anyway.

### Low-level packaging

For individual pack jobs, cache fill/pull, or dist layout, use `cargo xtask` after `./build` has built the xtask binary once — `./build help build` (via xtask) documents `build backends`, `pack`, `runtime-image`, etc. Pins live in `xtask/pack-pins.toml`. Do not commit musl ELFs or pack-cache blobs.
