# progressive-lsp

## What this is

One Language Server Protocol process for Linux. It answers definition, references, hover, symbols, and highlighting over stdio JSON-RPC. It is not an IDE, SSH, git, a file tree, or a terminal.

End-user install and editor wiring: [docs/user/README.md](docs/user/README.md). Progressive client APIs: [docs/user/progressive-v1-api.md](docs/user/progressive-v1-api.md).

## What it solves

Editors need language intelligence on machines that should not run Node, a JVM, or CPython as the server. progressive-lsp keeps the server runtime to a static Linux binary. Optional engine packs add full type engines when you install them.

The core boots without any pack. Opening Java does not require clangd. Opening PHP does not require clangd.

## Features

- **Static artifacts.** Shipped ELFs are musl-static with no dynamic linker dependency (see [docs/t3-linux-hosts.md](docs/t3-linux-hosts.md) for known exceptions).
- **Wide language surface.** C, C++, C#, Rust, JavaScript, TypeScript, CSS, HTML, Python, PHP, Java, Go, and Zig in the v1 matrix.
- **Three resolution tiers.** Tree-sitter (T1), heuristics (T2), optional full engines (T3). The editor is not blocked on ingest.
- **Stock LSP first.** `progressive-lsp serve` is enough for Neovim and similar clients. Protobuf control is optional.
- **Optional engines.** clangd, rust-analyzer, ty, tsgo, gopls, zls, and others are built as separate static pack binaries, not hosted runtimes.

## Structure

```text
progressive-lsp/          serve + install binary; wires crates
progressive-lsp-core/     ids, config, errors
progressive-lsp-*         index, resolve, watch, workspace, protocol, engine, install, …
progressive-lsp-lang-*    one factory crate per language
poc-ide/                  in-tree sample editor (consumer, not the server)
xtask/                    build, musl cross, engine packs, runtime image
integration/              container harness and tier-api-matrix suites
docs/                     product source of truth
```

Runtime layout on a host: `$HOME/.progressivelsp/bin/progressive-lsp` and `$HOME/.progressivelsp/engines/`. Writable state stays out of git trees.

Design detail: [docs/README.md](docs/README.md). Branch history: [docs/branching.md](docs/branching.md).

## Development

Operator entry is `./build` at the repo root (wraps `cargo xtask`). Help: `./build help`, `./build help lsp`, `./build help run`, `./build help build`.

**Prerequisites:** Rust toolchain. Docker Desktop for musl pack jobs, the runtime OCI image, integration smoke, and container poc-ide on macOS or Windows. Build output goes under `target/` (gitignored). xtask itself builds under `target/operator/` so it does not lock the main workspace target.

### Build

You need binaries before integration or smoke tests can run. Order:

1. **Native controller and poc-ide** (host development binary, no Linux image):

   ```sh
   ./build ide
   ```

2. **Linux musl server, engine packs, and runtime image** (what container mode and shipped layout use):

   ```sh
   ./build lsp aarch64
   ```

   Replace `aarch64` with `x86_64` or `all` as needed. Default flavor is **slim** (python, rust, java, php, html/css stacks, and related packs). Use **dogfood** for slim plus clangd, tsgo, gopls, and zls:

   ```sh
   ./build lsp aarch64 --flavor dogfood
   ```

   Dogfood is required for C/C++, TypeScript, Go, and Zig T3 in the runtime image. clangd is usually satisfied from pack cache pull, not a full local LLVM build; see [docs/consumer.md](docs/consumer.md).

   Stamps skip unchanged work. `--force` rebuilds musl core, packs, and the image.

   Pack pins: `xtask/pack-pins.toml`. Do not commit musl ELFs or pack-cache blobs.

Low-level pack, cache, and dist commands: `./build help build`.

### Test

After the build steps you need for the test type:

| Command | What it checks | Needs |
|---|---|---|
| `./build test` | Workspace and harness unit tests | Rust build; uses `target/` and `.cargo-home/` |
| `./build integ` | Hermetic discover + IT-TAM smoke in Docker | Runtime image `progressive-lsp-runtime:local`, harness `plsp-it1`; builds them if missing |
| `./build run ide --smoke` | poc-ide on in-tree Java fixture | Container on macOS/Windows; dogfood image when packs are missing |
| `./build run ide --folder /abs/path` | Manual poc-ide on your tree | Prior `./build lsp <arch>` for container mode; absolute path for bind mounts |

Native-only T1/T2 on a laptop without container:

```sh
./build run ide --native --folder /abs/path
```

That path does not replace the musl server and packs required for full T3 or CI parity.
