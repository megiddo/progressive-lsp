# Host dependencies

What may exist on a machine that runs **our** binaries vs what is a **project** artifact vs what is forbidden.

Related: [architecture.md](architecture.md), [testing.md](testing.md) (`check-static`, allocator matrix).

## Three buckets

| Bucket | Examples | Rule |
|---|---|---|
| **Our artifacts** | `progressive-lsp`, engine packs we ship (clangd, ty, RA, tsgo, gopls, zls, biome, superhtml, csharp-ls, PHPantom / static phpactor) | Fully static ELF: **no interpreter, no `DT_NEEDED`**. musl default; glibc-static optional **same** bar. |
| **Project artifacts** | `compile_commands.json`, jars, `node_modules` **as source**, `Cargo.toml`, rustc **sysroot** and proc-macro `.so` produced by the **user’s** rustc, `.csproj` outputs, project `go` / `zig` on PATH, `composer.json` | We read them. We do not ship them. Missing → degrade tier and say so. |
| **Forbidden as our runtime** | Node, JVM, CPython, host `php` on PATH as T3, any `.so` we ship | Never. |

## Libc flavors

| Flavor | What it is | What it is not |
|---|---|---|
| **musl-static (default)** | `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl`, allocator from matrix (not musl malloc) | — |
| **glibc-static (optional)** | Embed glibc via `*-linux-gnu` + `crt-static`, `-static-libgcc -static-libstdc++` | **Not** dynamic glibc. **Not** “use the host’s libc.” |

`xtask dist --libc musl|glibc-static`. Both must pass `check-static`.

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

**Empty.** No libstdc++.so, no libphp, no libjvm.

Allowed **project** compilers on PATH for **accuracy**, not for our libc: `rustc` sysroot, `go`, `zig`. Absence is degrade-to-T2, not a dynamic link of our ELF.

## Build-time (CI / xtask)

Rust pinned toolchain, musl cc, optionally Go/Zig/PHP **in a pinned container** to **build packs**. Those toolchains are not runtime deps of the core. PR CI does not compile LLVM/clangd/tsgo from scratch; use content-addressed artifact cache keyed by upstream git SHA.
