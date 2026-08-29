# Consumer guide

How to run progressive-lsp from an editor or remote-IDE host.

**This repo must never import another product’s paths** (no `.zedsdead` in Rust). Prefix override is enough.

Related: [lsp-contract.md](lsp-contract.md), [control-protocol.md](control-protocol.md), [architecture.md](architecture.md).

## Stock LSP client (Neovim, VS Code lsp-mode, tests)

1. Place the static `progressive-lsp` binary (and optional packs) via `progressive-lsp install --prefix ...` or by copying a release tarball. Default prefix: `$HOME/.progressivelsp`.
2. Spawn: `progressive-lsp serve` with **stdio**.
3. Speak vanilla LSP. Ignore `experimental.progressiveLsp` if you do not understand it.
4. Ghost disk edits: rely on **server-side watching**. You may also send `workspace/didChangeWatchedFiles`; the server coalesces.
5. Config: `$HOME/.progressivelsp/config.toml` and optional `<workspace>/.progressivelsp/config.toml`.

No crate dependency. No protobuf.

## Progressive client (example: zeds-dead)

Same LSP stdio **plus** optional control:

1. Read `capabilities.experimental.progressiveLsp` from `initialize`.
2. Connect to `socket`, or spawn with `--control-socket` / `--mux` if you own the process.
3. Use `progressive-lsp-control` for codec/types if you are a Rust consumer.
4. Prefer **your** watches + `WatchBatch` / `FilesSince`. Do not force a second inotify if you already watch the tree.
5. Install: depend on `progressive-lsp-install`. Implement `ArtifactTransport` for how bytes reach the host (**scp lives in the consumer**, not here). Example prefix: `PROGRESSIVE_LSP_HOME=~/.zedsdead/lsp` or `--prefix` — that is an **example**, not a coupling.

zeds-dead-host stays thin: SSH, mux, file tree, git, PTY, IDE config remain **there**. Language intelligence remains **here**.

## Bootstrap

- Already have bytes: library `Installer` verify + place only. URL fetch **off by default**.
- Bare host: copy one static binary, then `progressive-lsp install --packs ...` from a release URL **if** the caller’s transport fetches (optional; not required in-tree).

## CLI (target)

```text
progressive-lsp serve [--prefix DIR] [--control-socket PATH] [--control-fd N] [--mux]
progressive-lsp install --prefix DIR --packs python,rust,...
```

Env: `PROGRESSIVE_LSP_HOME` same meaning as `--prefix`. When both are set, `--prefix` wins.

`progressive-lsp install --prefix DIR --packs python` produces a **verified** prefix: each pack binary and `manifest.json` is written via `Installer` (hash tmp, then atomic rename). Hash mismatch or `on_install_verify` Abort → no rename, no exec. No network fetch. SSH is not implemented in `progressive-lsp-install`; consumers implement `ArtifactTransport` (tests use `FakeRemoteTransport` for remote-like put/chmod/rename/hash).

Pack layout: `$PREFIX/engines/python/ty` plus `manifest.json` (engine SHA256). `xtask dist --pack slim|full --dest DIR` writes that layout **and** per-triple tarballs (`x86_64-unknown-linux-musl/<flavor>.tar`, `aarch64-unknown-linux-musl/<flavor>.tar`) with sidecar SHA256 and a dist `manifest.json`. On Darwin the pack payloads are **stubs** (not musl ELFs); do not run `check-static` on them. Linux CI / Docker produce the real static musl tarballs.

Workspace/core crate version is **0.1.0** (first published v1; 1.0.0 waits for native macOS/Windows hosts, which are post-v1). Engine SHAs are pack-manifest fields, not Cargo versions. Proto stays `progressive.v1`.

Default `serve` is stock stdio LSP with control **off** (`experimental.progressiveLsp.socket` is null, `mux` is false). `--control-socket PATH` / `--control-fd N` advertise a side channel. `--mux` uses one stdio stream: channel `0` = opaque JSON-RPC, channel `1` = length-prefixed protobuf.

## What not to do

- Do not vendor this server inside an IDE agent as a fork of resolvers.
- Do not expect `$/` FilesSince in v1.
- Do not point engine discovery at a hardcoded consumer home dir; use `$PREFIX/engines/`.
