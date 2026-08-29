# IT-2 — Vanilla LSP on real codebases

**Goal:** Each **language backend** answers standard LSP on a real project tree. Stock client only (stdio JSON-RPC). No protobuf. No `$/` extensions.

“Backend” here means the intelligence path for that language: Tree-sitter ± heuristics ± optional T3 engine pack. Java has no T3 in v1.

## Shared protocol script

For every row:

1. `install --prefix $P --packs <packs for this language>` (empty packs for Java).
2. Copy engine binaries into `$P/engines/` when the dist includes them.
3. `serve --prefix $P` on stdio.
4. `initialize` with `rootUri` / `workspaceFolders` = corpus root. `initializationOptions` empty unless noted.
5. `initialized`.
6. Wait until `workDoneProgress` finishes **or** 60s deadline with last progress; then:
7. `textDocument/didOpen` on the **entry file** (table).
8. Requests below. Positions are 0-based, stored in `expected/<corpus>.json`.
9. `shutdown` / `exit`.

**Always assert (stock contract)**

| Method | Minimum |
|---|---|
| `initialize` capabilities | `definitionProvider`, `referencesProvider`, `hoverProvider`, `documentSymbolProvider`, `workspaceSymbolProvider`, `semanticTokensProvider`, `experimental.progressiveLsp` present |
| `textDocument/definition` | ≥1 `Location`; URI is in-workspace (or stdlib/project artifact). If `data.tier` is set, it is `syntax` \| `graph` \| `types` |
| `textDocument/references` | ≥1 location including or besides the definition (language-dependent; see row notes) |
| `textDocument/hover` | non-empty contents (signature at T1 is enough) |
| `textDocument/documentSymbol` | ≥1 symbol in the entry file |
| `textDocument/semanticTokens/full` | data array length > 0 |
| `textDocument/didChange` | one edit in the entry file; tokens or hover still succeed (server did not die) |

**When the active tier can:** `typeDefinition` / `implementation` may be empty at T1; **must** be non-empty for T3 rows marked “types.”

**Ghost disk (stock):** after didOpen, rewrite an **unopened** sibling file on disk (not via LSP). Within the watch coalescer window + 2s deadline, `workspace/symbol` or definition that depended on that file must update **or** a documented progress event fires. This proves server-side `notify` without a progressive client.

**Negative:** send `$/progressive/filesSince` or `workspace/filesSince`. Server must **not** implement it (method not found / ignored). FilesSince is protobuf-only.

## Corpora

Pin **git URL + commit SHA** in the harness (fetch in CI cache, not every PR). Prefer small-but-real trees with a normal manifest. Licenses: Apache-2 / MIT / BSD.

| Language | Corpus (suggested) | Manifest the adapter must see | Entry file (example) | Packs | Tier under test |
|---|---|---|---|---|---|
| Java | `junit-team/junit4` (pin SHA) or in-tree `fixtures/java-heuristic` **plus** one external Maven tree | `pom.xml` | a test class that calls another package | none | T1 then T2 after ingest |
| PHP | `php-fig/log` | `composer.json` | `src/LoggerInterface.php` use-site in a tiny app overlay | optional `phpantom` | T2 without pack; T3 with pack |
| JavaScript | `preactjs/preact` (pin) | `package.json` | component file with import | none / `tsgo` | T1; T2 oxc if present; T3 tsgo |
| TypeScript | `preactjs/preact` TS sources or `evanw/esbuild` is Go — use a small TS app: `microsoft/TypeScript-Website` is huge. Prefer `colinhacks/zod` (pin) | `package.json` + `tsconfig.json` | exported schema used from another file | `tsgo` | T3 types/generics |
| CSS | `picocss/pico` (pin) | none required | a `.css` file with a class reused | `biome` optional | T1 symbols/selectors; biome diagnostics if pack |
| HTML | same pico docs HTML or a small static site | none | `.html` with `id` referenced | `superhtml` optional | T1; T3 if pack |
| Go | `rsc.io/quote` or `hashicorp/go-version` (pin) | `go.mod` | file importing another package | `gopls` | T2 without `go`; T3 with pack + project `go` if needed |
| Zig | a pinned `build.zig` app (e.g. small tagged example, not `ziglang/zig`) | `build.zig` | `@import` of a relative `.zig` | `zls` | T1/T2 without `zig`; T3 with pack |
| Python | `pallets/flask` (pin a tag) | `pyproject.toml` | `flask/app.py` symbol used from examples | `ty` | T1 without pack; T3 with ty |
| Rust | `dtolnay/anyhow` (pin) | `Cargo.toml` | `src/lib.rs` public item | `rust-analyzer` | T1 without pack; T3 with RA + **project rustc** if sysroot needed |
| C | `redis/hiredis` (pin) | `compile_commands.json` **generated once in CI** (`bear` or cmake) on the **job** image that has cmake, then copy JSON + sources into the **distro-under-test** which still has no compilers | a `.c` file calling another | `clangd` | T3 |
| C++ | `fmtlib/fmt` is large; use `nlohmann/json` single-header **or** a tiny CMake C++ fixture with two TUs | `compile_commands.json` | `.cpp` using a header | `clangd` | T3 |
| C# | small `*.csproj` (e.g. Humanizer is large). Prefer a **pinned** two-project `net8.0` tree in `corpora/csharp-mini` **imported from a real public snippet** (keep `.csproj` authentic) | `*.csproj` | `Program.cs` calling a class library | `csharp-ls` | T3 if AOT pack exists; else **T1/T2 ceiling** — row marked `expected_ceiling` |

If a suggested upstream is too large for CI, replace with a **smaller real repo** of the same shape; do not replace with a 20-line synthetic file (that belongs in unit tests). `fixtures/java-multi` in the product repo is a **supplement** for Maven/Gradle, not the only Java proof.

## Per-language extra asserts

### Java (T2)

- F12 from `App` to `Lib` across packages (same as M1/M2 fixtures).
- Hover shows name + arity at T1; after ingest, `data.tier` becomes `graph` on a cross-file definition.
- **No JDK** in the container.

### PHP

- F12 across namespaces via Composer PSR-4 without `php` on PATH.
- With PHP pack: hover/types/implementation on a Composer fixture; without pack: T2 still navigates.

### JS / TS

- **No Node** in the container for the server. Project `node_modules` may exist as **source** on disk (copied), not as a runtime for our ELF.
- TS T3: `textDocument/typeDefinition` or implementation on a generic (tsgo row).

### Python

- **No CPython** required for the **server**. ty pack answers types. Without ty: T1 still tokens + local def.

### Rust

- Without rust-analyzer: T1 navigation in-file / crate files the grammar can see.
- With RA and no sysroot: hover or progress explains degrade; process stays up.
- With RA + sysroot (optional job with rustc installed **as project toolchain**, not as our libc): T3 types.

### C / C++

- Requires `compile_commands.json`. Do not invent a build inside the **musl-only** distro container; generate compile_commands in a builder image, copy in.
- `textDocument/implementation` or definition through a header.

### Go / Zig

- T3 may need project `go` / `zig` on PATH. Split rows: `no_toolchain` (T2) vs `with_toolchain` (T3). Do not bundle a Go/Zig SDK in core.

### HTML / CSS

- Document symbols + selector/`id` find-usages at T1. superhtml/biome if packs present.

### C#

- If csharp-ls pack missing or spike failed: assert T1/T2 only and record `ceiling` in the report. Do not fail the whole suite.

## Isolation asserts

- Opening the **Java** corpus with only the core binary does **not** require `engines/clangd` to exist.
- Opening **PHP** does not require clangd or host `php`.

## Report columns

`language`, `corpus_sha`, `pack`, `tier_observed`, `definition_ok`, `tokens_ok`, `ghost_edit_ok`, `notes`.
