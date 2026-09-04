# Host dependencies

What may exist on a machine that runs **our** binaries vs what is a **project** artifact vs what is forbidden.

Related: [architecture.md](architecture.md), [testing.md](testing.md) (`check-static`, allocator matrix).

## Three buckets

| Bucket | Examples | Rule |
|---|---|---|
| **Our artifacts** | `progressive-lsp`, engine packs we ship (clangd, ty, RA, tsgo, gopls, zls, biome, superhtml, csharp-ls, PHPantom / static phpactor), **sqlite amalgamation** compiled into the core ELF via `rusqlite` `bundled` (LOG-2+; not a host `libsqlite3.so`) | Fully static ELF: **no interpreter, no `DT_NEEDED`**. musl default; glibc-static optional **same** bar. Loadable sqlite extensions off (`SQLITE_OMIT_LOAD_EXTENSION`). |
| **Project artifacts** | `compile_commands.json`, jars, `node_modules` **as source**, `Cargo.toml`, rustc **sysroot** and proc-macro `.so` produced by the **user’s** rustc, `.csproj` outputs, project `go` / `zig` on PATH, `composer.json` | We read them. We do not ship them. Missing → degrade tier and say so. |
| **Forbidden as our runtime** | Node, JVM, CPython, host `php` on PATH as T3, any `.so` we ship | Never. |

## Libc flavors

| Flavor | What it is | What it is not |
|---|---|---|
| **musl-static (default)** | `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl`, allocator from matrix (not musl malloc) | — |
| **glibc-static (optional)** | Embed glibc via `*-linux-gnu` + `crt-static`, `-static-libgcc -static-libstdc++` | **Not** dynamic glibc. **Not** “use the host’s libc.” |

`xtask dist --libc musl|glibc-static --dest DIR` writes per-triple tarballs + SHA256 + `manifest.json`. On Darwin (this host) pack payloads are **stubs** — not musl ELFs. Do not run `check-static` on those stubs or claim greens. Linux CI must publish `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` tarballs that **do** pass `check-static`. Both libc flavors must pass `check-static` when they contain real ELFs.

### glibc-static obligations

- **LGPL on glibc:** static link has redistribution/relink obligations. Dist notes must not pretend this is MIT-like musl.
- **Do not “fix” `dlopen`:** if something needs gconv/NSS plugins, stop calling it or keep that engine musl-only / T2. Do not ship `.so`.
- **Embedded glibc CVEs:** the binary carries build-time glibc forever. Rebuild to patch (same class as embedding LLVM).
- **clangd:** closing the static archive graph (`zlib`, `libxml2`, `ncurses`/`tinfo`, `libffi`, `zstd`, `libedit`, …) is expected whack-a-mole. Record the first unclosable `.so`-only dep as a documented miss — do not ship dynamic.
- **csharp-ls + static glibc:** historically crashy. musl AOT is the supported static path. glibc-static csharp-ls is a POC gate: segfault → flavor out, musl AOT remains.
- **Go packs:** keep `CGO_ENABLED=0` (pure Go, **no libc**). Do not CGO-link glibc-static for gopls/tsgo.

POC lives on `spike/glibc-static` then folds into M0 xtask: tiny bin on a **different** distro than CI; then Tree-sitter; then clangd graph; csharp-ls pass or fail closed.

**Why musl by default:** musl is meant to be `-static` with Rust/Go musl targets. The operational problem is the **allocator**, replaced via the matrix — not by dynamically linking glibc. A type index should not call `getaddrinfo`.

## Allocator

See pick rule in [testing.md](testing.md). Skip matrix: gopls, tsgo, zls, superhtml, csharp-ls (their heaps). Empty cell → mimalloc placeholder.

## PHP T3

If phpactor: still a **static** ELF (static php), documented here. Host `php` is forbidden. PHPantom preferred (Rust, matrix `rust-engine` cell).

## Runtime host deps for **our** binaries

**Empty.** No libstdc++.so, no libphp, no libjvm. No host `libsqlite3.so` — the amalgamation is **static C in our ELF** ([logging.md](logging.md)). Fallback WALs (LOG-9 `serve-fallback-*` and temp-dir sqlite) use the same bundled crate. `check-static` must stay green after rusqlite is linked (no `DT_NEEDED`, no interpreter, no `libdl` from sqlite extensions).

Allowed **project** compilers on PATH for **accuracy**, not for our libc: `rustc` sysroot, `go`, `zig`. Absence is degrade-to-T2, not a dynamic link of our ELF.

## poc-ide container host (HOST-0+)

The **editor** process on Darwin/Windows may invoke the host `docker` CLI (`DockerRuntime` Adapter) to probe / inspect a local runtime image. That CLI is a **host tool**, not a shipped ELF and not a `DT_NEEDED` of `progressive-lsp`. Tests inject `FakeRuntime` or a scripted CLI; they never talk to a Docker daemon, registry, or AWS. `DockerRuntime::start` validates a `DockerRunPlan` and does not exec; poc-ide `MuxStdio::from_command` is the single `docker run -i --rm` attach (HOST-6; `serve --prefix /opt/plsp --mux` on that stdio). Native Darwin open still uses a real Unix control socket. Unix sockets through Docker Desktop stay forbidden. The **runtime image** `progressive-lsp-runtime:local` is **our artifact** (HOST-4: `xtask runtime-image` copies prebuilt HOST-2 core + HOST-3 slim ELFs into `/opt/plsp`, plus HOST-7 full packs when present; `FROM scratch`; no rustc/cargo/clang/LLVM/zig/go in that image). The serve binary inside the image remains musl-static ([above](#runtime-host-deps-for-our-binaries)).

## Build-time (CI / xtask)

Rust pinned toolchain, musl cc, optionally Go/Zig/PHP **in a pinned container** to **build packs**. Those toolchains are not runtime deps of the core. PR CI does not compile LLVM/clangd/tsgo from scratch; use content-addressed artifact cache keyed by upstream git SHA.

### M3 engine packs (ty, rust-analyzer)

`xtask dist --pack python,rust --dest DIR` writes `$PREFIX/engines/python/ty` and `$PREFIX/engines/rust/rust-analyzer` plus `manifest.json` (SHA256). On Darwin this host writes **stub** bytes (`progressive-lsp-pack-stub:…`) and `engines/DARWIN_CI_GAP.txt`. Those stubs are not musl ELFs and must not be `check-static` greened. HOST-3: `xtask pack` extracts real musl `ty` / `rust-analyzer` via Docker (`RecordingDockerPort` in crate tests). Tests use `FakeEngineAdapter` and fixture hashes; they do not download LLVM.

### M4 engine packs (clangd, tsgo, PHPantom, superhtml, biome, gopls, zls)

`xtask dist` **slim** (default, also `--pack slim` / `--slim`) writes python, rust, phpantom, superhtml, biome stubs. **Full** (`--pack full` / `--full`) adds clangd, tsgo, gopls, zls. Slim is the Java-only default and excludes those heavy packs. csharp-ls is **not** a pack (T1/T2 ceiling). Darwin `xtask dist` still writes stubs + `DARWIN_CI_GAP.txt` only — do not confuse stub hashes with `check-static` greens. HOST-3 builds real slim musl packs (`phpantom`, `superhtml`, `biome` plus M3 ty/RA) via `xtask pack` / Docker extract to `target/musl/<triple>/engines/<pack>/<binary>`. HOST-7 builds full packs (`gopls`/`tsgo` via `docker/engine-pack-go.Dockerfile` + `CGO_ENABLED=0`; `zls` via the Zig dockerfile; `clangd` cache COPY keyed by upstream SHA + triple). Missing clangd cache is a documented miss — PR CI must not cmake LLVM. `--cache-fill clangd` is a dedicated cache-fill Dockerfile, not the default pack job. Fail closed: do not ship `DT_NEEDED`. One-shot `cmake -DCMAKE_EXPORT_COMPILE_COMMANDS` only if `CMakeLists.txt` already exists — we do not invent a build. Project `go` / `zig` on PATH are project artifacts, not bundled SDKs. Zig/Go toolchains may exist **inside the pack build container**.

## Complete vs implementation (M0–M5)

What actually shipped through M5, mapped to the three buckets. No M6 dist tarballs / dashboard.

| Bucket | Implementation (this tree) | Forbidden / not shipped |
|---|---|---|
| **Our artifacts** | `progressive-lsp` bin (composition root). Engine **packs we name**: ty, rust-analyzer, clangd, tsgo, **PHPantom** (PHP T3 winner), superhtml, biome, gopls, zls. HOST-4 runtime image `progressive-lsp-runtime:local` (copy of prebuilt core + slim ELFs under `/opt/plsp`, plus optional HOST-7 full packs; not compiled in the image). `xtask dist` writes Darwin **stubs** + `DARWIN_CI_GAP.txt`; real static ELFs are Linux CI / Docker. Slim default: python, rust, phpantom, superhtml, biome. Full adds clangd, tsgo, gopls, zls. Index cache files under `$PREFIX/cache/` (content-addressed). | csharp-ls pack (spike failed-closed; C# **T1/T2 ceiling**). Static phpactor (not shipped; PHPantom won). Host `.so` we would ship. Dynamic interpreter / `DT_NEEDED` on a shipped ELF. Cache or bins inside a git worktree. |
| **Project artifacts** | `compile_commands.json` (read; one-shot cmake only if `CMakeLists.txt` exists). Jars / `.class` as disk facts. `node_modules` **as source**. `Cargo.toml` + rustc **sysroot** / proc-macro `.so` from the **user’s** rustc. `*.csproj` outputs. Project `go` / `zig` on PATH. `composer.json`. Manifests listed in [architecture.md](architecture.md). | We do not ship a JDK, Node, CPython, PHP runtime, Go SDK, or Zig toolchain. Missing project artifact → degrade tier and say so. |
| **Forbidden as our runtime** | Empty. No libstdc++.so, no libphp, no libjvm, no libnode. | Node, JVM, CPython, host `php` on PATH as T3, any `.so` **we** ship. tsserver, JDT-LS, pylsp, pyright, ruff-as-types, intelephense. |

C# stays T1/T2. Java stays T1/T2 (no T3). PHP T3 is PHPantom when the pack is present; else T2. Darwin `xtask dist` stubs are not musl ELF greens and must not be `check-static` greened. Allocator-matrix cells stay mimalloc placeholders until a matching CI arch job records a winner. sqlite amalgamation lives in `progressive-lsp-log` (LOG-2; `rusqlite` `bundled` + `SQLITE_OMIT_LOAD_EXTENSION`) and is **our artifact** (static C in the ELF, not a host `libsqlite3.so`). The composition-root bin **is** wired to `SqliteLogRepository` (LOG-4). HOST-2: `xtask musl` extracts the **core** ELF to `target/musl/<triple>/progressive-lsp` via Docker (`linux/amd64` ← `x86_64-unknown-linux-musl`, `linux/arm64` ← `aarch64-unknown-linux-musl`) and runs `check-static` on that file (no `PT_INTERP`, no `DT_NEEDED`, no `libdl`). HOST-3: `xtask pack` extracts **slim** musl packs to `target/musl/<triple>/engines/<pack>/<binary>` via the same `DockerPort` / `docker build --output` pattern (`docker/engine-pack.Dockerfile` for ty/RA/phpantom/biome; `docker/engine-pack-zig.Dockerfile` for superhtml). Pins are `xtask/pack-pins.toml` (git SHA, not `latest`). Darwin crate tests still inject `RecordingDockerPort` and never start a daemon — a fixture extract is not a musl green. HOST-3 proof is the extracted musl ELFs passing `check-static`, not Darwin dist stubs. Dest is under `target/` (gitignored); do not commit musl ELFs. A missing Docker daemon or a failed qemu/amd64 build is a recorded Darwin vs CI gap, not a Mach-O green. Runtime host deps for the core ELF stay **empty**. HOST-7: `xtask pack --pack full` extracts full flavor musl packs (gopls/tsgo/zls via Docker; clangd via content-addressed cache). PR CI still must not compile LLVM from scratch. HOST-4: `xtask runtime-image` copies prebuilt dests into `progressive-lsp-runtime:local` (`/opt/plsp`); full packs are optional copies when present (same miss rule as HOST-3 superhtml × x86_64); the image is **our artifact**; the docker CLI remains a **host tool**; crate tests still use `FakeRuntime` / `RecordingDockerPort`. `DockerRuntime::start` is HOST-5; mux is HOST-6.
