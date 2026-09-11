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

## In-tree POC editor

`poc-ide/` is a native egui sample that speaks stock stdio LSP and, optionally, `progressive.v1`. It is how we exercise this repo’s contracts in a folder-shaped UI. It is **not** zeds-dead and **not** a shipped musl artifact. On non-Linux, **Open Folder…** is a native T1/T2 serve; **Open Folder in Container…** is one Linux `progressive-lsp serve --mux` for T1/T2/T3. The container is a local Linux host: identity bind-mount, **same `file:` URIs**, no rewriter ([t3-linux-hosts.md](t3-linux-hosts.md)). Linux native open is the full host. Never two serves. See [poc-ide/README.md](poc-ide/README.md).

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

## SHA-pegged artifact store (POST-ART)

Engine blobs are **not** committed to git. Pins live in `xtask/pack-pins.toml` (`upstream_sha` per pack). Published bytes are addressed by SHA + triple + content hash.

**Manifest schema** (`StoreManifest`, one row per blob):

| Field | Meaning |
|---|---|
| `pack` | Pack name (`clangd`, …) |
| `upstream_sha` | 40-hex git SHA from `pack-pins.toml` |
| `triple` | Musl triple (`x86_64-unknown-linux-musl`, …) |
| `sha256` | Lowercase hex of the **downloaded archive** (or raw blob) |
| `url` | HTTPS (or `file://` in tests) fetch URL |
| `format` | `tar.gz` \| `tar` \| `zip` \| `tar.bz2` |

Schema reference: [xtask/artifact-manifest.example.json](../xtask/artifact-manifest.example.json) (empty until you push).

**Local store (default; no remote host required):** gitignored `target/local-artifacts/engines/{pack}/{upstream_sha}/{triple}.tar.gz` plus `target/local-artifacts/manifest.json` (`file://` URLs). After `--cache-fill`, run `cargo xtask pack --pack clangd --cache push` to archive the cache ELF and update the local manifest. Upload to CDN/GitHub later by copying those files and repointing `url` to `https://…`.

**Remote layout (optional later)** when `PROGRESSIVE_LSP_ARTIFACT_BASE` is set:

```text
{base}/engines/{pack}/{upstream_sha}/{triple}.{format}
```

Default `format` is `tar.gz` (`PROGRESSIVE_LSP_ARTIFACT_FORMAT` overrides).

**Pull** (populates `target/pack-cache/clangd/<sha>/<triple>/clangd` from local store first, then optional env overrides):

```text
cargo xtask pack --pack clangd --cache pull
```

Optional: `PROGRESSIVE_LSP_ARTIFACT_MANIFEST=…` or `PROGRESSIVE_LSP_ARTIFACT_BASE=https://…`.

`./build lsp --flavor dogfood` tries cache pull for `kind = cached` (clangd) before fail-closing. Remote upload only when `PROGRESSIVE_LSP_ARTIFACT_PUSH=1` **and** `PROGRESSIVE_LSP_ARTIFACT_BASE` are set.

**Fat release**: `cargo xtask dist --pack full` and `progressive-lsp-runtime:<tag>` are equivalent “everything under prefix” shapes for publishing one full tarball (+ optional OCI tag per ISA). See [testing.md](testing.md).

**Install / consumers**: in-tree `progressive-lsp install` stays verify-only (no silent network). Remote hosts depend on `progressive-lsp-install` and implement `ArtifactTransport` to fetch manifest URLs, verify sha256, then call `Installer::apply_manifest`. SSH/scp stays in the consumer (e.g. zeds-dead), not in this repo.

## What not to do

- Do not vendor this server inside an IDE agent as a fork of resolvers.
- Do not expect `$/` FilesSince in v1.
- Do not point engine discovery at a hardcoded consumer home dir; use `$PREFIX/engines/`.
