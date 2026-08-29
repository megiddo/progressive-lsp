# Requirements

Normative. If code and this file disagree, fix the code or file an explicit doc change. Vision: [vision.md](vision.md).

## Goals

- Multi-language progressive LSP: T1 Tree-sitter → T2 heuristics/stack-graphs → T3 static engines.
- Target languages (v1): C, C++, C#, Rust, JavaScript, TypeScript, CSS, HTML, Python, PHP, Java, Go, Zig.
- Syntax highlighting via `textDocument/semanticTokens` (Tree-sitter; T3 overlay when ready).
- Type-resolution LSP where T3 exists: definition, references, type definition, implementation, generics.
- Real-time eval of open buffers; file monitoring and incremental reindex.
- Static compilation of everything we ship; allocator chosen by committed matrix.
- Optional protobuf control plane for remote-IDE extras.
- Host Rhai scripts and compile-time plugin traits.
- Reusable install library (`ArtifactTransport`); we do not implement SSH.

## Restrictions

| Restriction | Detail |
|---|---|
| No dynamic linking of our artifacts | `check-static`: no interpreter, no `DT_NEEDED`. |
| No Node / JVM / CPython as LSP runtime | No tsserver, JDT-LS, pylsp, pyright, intelephense. |
| Engine/backends | C, C++, C# Native AOT, Rust, Go, Zig, PHP (static pack only). |
| Java T3 | Out of v1. No JVM. |
| Host `php` | Not a T3 pack. |
| Not an IDE host | No SSH, git porcelain, PTY, file-tree CRUD in this repo. |
| No `$/` mirror of control RPCs in v1 | [lsp-contract.md](lsp-contract.md). |
| No `dlopen` plugins in v1 | Compile-time Factory + Rhai. |
| No Python/Node **build** scripts | `cargo xtask` orchestrates; Go/Zig/PHP toolchains are build-time for packs. |

## Functional requirements

### F1 — LSP (stock)

- `initialize` / `shutdown` / `exit`.
- `textDocument/didOpen` / `didChange` / `didClose`; incremental Tree-sitter `InputEdit` on change.
- `textDocument/definition`, `references`, `documentSymbol`, `workspace/symbol`.
- `textDocument/hover` (signature at T1; types when T2/T3 can).
- `textDocument/typeDefinition`, `implementation` when the active tier can answer; empty/omitted when not.
- `textDocument/semanticTokens`.
- Standard `workDoneProgress` during ingest.
- `Location.data.tier` = `syntax` | `graph` | `types` when we set `data`.
- Advertise `experimental.progressiveLsp` even if the control socket is off.

### F2 — Progressive quality

- Resolver **Chain**: T3 if engine ready for that package, else T2, else T1. Never block `didChange` highlighting on T3.
- Package-stream ingest: as a package finishes T2, cross-file nav in that package becomes available.
- Progressive clients get `TierReady` on the control plane.

### F3 — Watch and ghost disk

- Stock: server-side `notify`; coalesced dirty-set; FakeClock in tests.
- Optional client `workspace/didChangeWatchedFiles`; coalesce, do not double-walk.
- Progressive: `WatchBatch` push; `FilesSince` catch-up (not in stock LSP).
- Overflow → bounded `FilesSince` / rescan flag, not a naive full tree walk as the happy path.
- Ignore: `node_modules` internals, `.git/objects`, vendor object stores, `zig-cache` / `.zig-cache`, module download caches. Still watch manifests listed in [architecture.md](architecture.md).

### F4 — Workspace shape

- `WorkspaceSource` adapters: Directory, Maven, Gradle (one-shot classpath listing allowed), Eclipse `.classpath`, Composer PSR-4, `package.json`+`tsconfig`, Cargo, `compile_commands.json`, `*.csproj`, `pyproject.toml`, `go.mod`/`go.work`, `build.zig`/`build.zig.zon`.
- Adapters read metadata; they are not the compiler.

### F5 — Engines and packs

- Optional binaries under `$PREFIX/engines/`. Core boots without them.
- Pack select: explicit list or census ([detailed-design.md](detailed-design.md) `PackSelector`).
- T3 engines: clangd, rust-analyzer, ty, csharp-ls (AOT), tsgo, gopls, zls, superhtml, biome, PHP static pack (PHPantom preferred).
- Handoff: forward `didChange` / watches to children; crash/backoff; T2 remains if spawn skipped.

### F6 — Config, scripts, install

- `.progressivelsp` layout and merge chain: [architecture.md](architecture.md).
- Rhai hooks: [plugin-sdk.md](plugin-sdk.md). Scripts cannot register LSP methods.
- `progressive-lsp-install`: probe, plan, atomic place, SHA256. `LocalFs` in-tree. SSH is a consumer `ArtifactTransport`.

### F7 — Language versions

- Support LATEST + 2 prior per language, mixed in one workspace within that window.
- Effective version = `min(our window, grammar, T3 engine)`. Surface lag; do not panic on newer syntax.

## Non-functional requirements

| ID | Requirement | Gate |
|---|---|---|
| N1 | Core `textDocument/definition` p99 &lt; 50 ms once T1/T2 indexed (open-buffer path) | criterion, M1+ |
| N2 | Open-buffer Tree-sitter reparse ~10 ms class | criterion, M1+ |
| N3 | Core RSS without engines: budget in [testing.md](testing.md); engines billed separately | M5 |
| N4 | T2 debounce on the dirty file ~50–100 ms | FakeClock tests |
| N5 | 10k injected watch events → one coalesced batch | M1 |
| N6 | 95% llvm-cov on library crates; 80% mutants on listed crates | CI from first crate |
| N7 | No `sleep` in tests; `--test-threads=1` must pass | CI |
| N8 | `check-static` on every shipped ELF | `xtask` / CI |
| N9 | musl x86_64 and aarch64 for default dist; glibc-static optional same bar | M0 |
| N10 | Allocator from `xtask/allocator-matrix.toml` pick rule | [testing.md](testing.md) |
| N11 | Core usable alone; clangd not required for PHP/Java | pack tests |

## Out of v1 (scheduled later)

Java full type resolution (no JVM). WASM plugin ABI. `$/` JSON mirror of `progressive.v1`. Native macOS/Windows **hosts**. HTTP/S3 `ArtifactTransport` in-tree. Watchman. oxc_type_checker as TS T3 if it matures.
