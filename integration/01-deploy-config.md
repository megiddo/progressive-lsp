# IT-1 — Easy deploy and config

**Goal:** A static `progressive-lsp` core binary + `install`/`serve` work on a **fresh** Linux userspace with no language runtimes installed. Config is obvious and git-safe.

## Distro matrix

Run the same script in Docker (linux/amd64 and linux/arm64 as CI allows). Images are **userspaces**, not build farms — copy a **prebuilt** artifact in; do not `cargo build` inside these containers.

| ID | Image | Role |
|---|---|---|
| D-arch | `archlinux:base` (pin digest) | Rolling glibc, no extra packages |
| D-rhel | `rockylinux:9` or `redhat/ubi9-minimal` | RHEL-family; old-enough glibc vs musl binary |
| D-debian | `debian:bookworm-slim` | Debian stable |
| D-ubuntu | `ubuntu:24.04` | Ubuntu LTS |

**Optional extra (glibc-static flavor only):** `debian:bullseye-slim` — proves the glibc-static tarball runs on an older glibc than the build image. Skip if only musl artifacts are under test.

Do not install `nodejs`, `openjdk`, `python3`, or `php` in the image. `ca-certificates` is allowed if the test copies files in via bind-mount (preferred: `docker cp` / bind-mount, **no network fetch** in the container).

## Artifact under test

From `xtask dist --libc musl` (default):

```text
progressive-lsp          # core ELF
SHA256SUMS
```

Packs are **not** required for IT-1. Optionally copy `engines/` empty.

On the host (CI job matching `arch`):

```text
readelf -l progressive-lsp   # no INTERP
readelf -d progressive-lsp   # no NEEDED
sha256sum -c SHA256SUMS
```

## Cases

### IT-1.1 — Copy, install prefix, serve initialize

**Steps**

1. Bind-mount the ELF to `/opt/plsp/progressive-lsp` (chmod +x).
2. `HOME=/tmp/home` (empty).
3. `/opt/plsp/progressive-lsp install --prefix /tmp/home/.progressivelsp --packs python`
4. Assert layout exists: `bin/` (or prefix dirs), `engines/`, `cache/`, `log/`, `run/`, `scripts/`, `workspaces/`, `config.toml`, `installed-packs.toml`.
5. Spawn `PROGRESSIVE_LSP_HOME=/tmp/home/.progressivelsp /opt/plsp/progressive-lsp serve` with stdio.
6. Send LSP `initialize` (no `rootUri` required beyond a temp empty folder) + `initialized` + `shutdown` + `exit`.

**Pass**

- Process exits 0 on shutdown.
- `initialize` result: `serverInfo.name` is `progressive-lsp`.
- `capabilities.experimental.progressiveLsp.version` is `"v1"`.
- `socket` is JSON `null` (default serve: control off).
- `mux` is `false`.
- Container has no `node`, `java`, `python3` on PATH (assert `command -v` fails).

### IT-1.2 — Default home without `--prefix`

**Steps**

1. `HOME=/tmp/alice`, unset `PROGRESSIVE_LSP_HOME`.
2. `serve` with no `--prefix`.
3. After initialize, prefix dir is `/tmp/alice/.progressivelsp` (created).

**Pass:** dirs + stub `config.toml` present. Empty config is valid.

### IT-1.3 — `PROGRESSIVE_LSP_HOME` vs `--prefix`

**Steps:** set env to `/tmp/envhome`, pass `--prefix /tmp/cliprefix`.

**Pass:** files land under `/tmp/cliprefix` (CLI wins). Documented in user README.

### IT-1.4 — Project overlay config merge

**Steps**

1. User global: `/tmp/home/.progressivelsp/config.toml` with `packs = ["rust"]`.
2. Workspace `/tmp/ws` with `/tmp/ws/.progressivelsp/config.toml` containing `packs = ["python"]` and an unknown key `future = 1`.
3. `serve --prefix /tmp/home/.progressivelsp`, `initialize` with `rootUri` = `/tmp/ws`.
4. If control is off, assert on-disk merge by reading files; with control (IT-3) `GetConfig` toml contains `python` not `rust`.

**Pass:** overlay wins for `packs`. Server still initializes (unknown keys do not fail). Workspace git repo: `cache/` is **not** created under `/tmp/ws/.progressivelsp/` (cache stays in prefix). Belt `.gitignore` inside overlay if the product writes one.

### IT-1.5 — Git exclude, no project `.gitignore` edit

**Steps**

1. `git init /tmp/ws` with a committed `.gitignore` containing only `/target`.
2. Initialize server on that workspace.
3. If the server would write cache under the overlay, it must append to `.git/info/exclude` instead of rewriting `.gitignore`.

**Pass:** project `.gitignore` bytes unchanged. No `cache/` committed (status clean except optional overlay config).

### IT-1.6 — Distro libc isolation

**Steps:** on each image, `ldd progressive-lsp` fails or reports “not a dynamic executable”. Run `serve` initialize.

**Pass:** same on Arch, Rocky, Debian, Ubuntu. Failure on one distro is a product bug (dynamic dep or assumed path), not “install libc.”

### IT-1.7 — Help and bad flags

**Steps:** `progressive-lsp`, `progressive-lsp help`, `serve --nope`.

**Pass:** non-zero exit, usage on stderr, process does not hang.

## Explicit non-goals for IT-1

- Engine child processes.
- F12 / highlighting quality.
- Network install from a URL (fetch off by default).
