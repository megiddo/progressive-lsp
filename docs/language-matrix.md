# Language matrix (v1)

Rolling pin of **latest language spec plus two prior**. CI fails if a matrix fixture regresses. Grammar/engine upgrades are **scheduled work**, not drive-by Cargo bumps.

**Effective supported version** = `min(our window, Tree-sitter grammar, T3 engine)`. Surface that in `initialize` / capabilities so a client can show “syntax unparsed.”

Unknown syntax → ERROR nodes; **server stays up**.

Detect per-package from manifests (`composer.json` `php`, Cargo edition, cmake/`-std=`, `LangVersion`, pom `release`, `requires-python`, `go` directive, Zig in `build.zig.zon` / toolchain). A workspace may **mix** versions within the window.

Window below is **as of 2026-08**. Update the table when LATEST moves; keep fixtures for LATEST, LATEST-1, LATEST-2.

## Window (pins)

| Language | LATEST | LATEST-1 | LATEST-2 | T1 | T2 | T3 engine | Notes |
|---|---|---|---|---|---|---|---|
| C | C23 | C17 | C11 | Tree-sitter | brief gap / optional TSG later | **clangd** | Needs `compile_commands.json`. One-shot `cmake -DCMAKE_EXPORT_COMPILE_COMMANDS` only if the project already uses CMake — do not invent a build. |
| C++ | C++26 | C++23 | C++20 | Tree-sitter | same | **clangd** | Same compile_commands rule. |
| C# | 14 | 13 | 12 | Tree-sitter | heuristics | **csharp-ls** Native AOT `linux-musl-*` | Spike: if AOT+project load fails, **T1/T2 ceiling** in v1. glibc-static csharp-ls may fail closed. |
| Rust | edition 2024 | 2021 | 2018 | Tree-sitter | — | **rust-analyzer** | Sysroot / proc-macro `.so` are **project** artifacts. No pack or no sysroot → **T1** + say so in hover/progress (no dedicated Rust T2). |
| JavaScript | ES2026 | ES2025 | ES2024 | Tree-sitter | **oxc_resolver + oxc_semantic**; optional TSG | **tsgo** | **Not** tsserver/Node. oxc_type_checker does not block tsgo. |
| TypeScript | current 3-release window | | | Tree-sitter | oxc + optional TSG | **tsgo** | Pin exact TS versions in CI fixtures. |
| CSS | current | −1 | −2 | Tree-sitter | — | **biome** | No Node CSS LS. |
| HTML | current | −1 | −2 | Tree-sitter | — | **superhtml** (Zig) | Fallback T1 if pack absent. |
| Python | 3.14 | 3.13 | 3.12 | Tree-sitter | optional TSG | **ty** | **Not** CPython, pylsp, pyright, ruff-as-types. Heuristics are not a dedicated Python T2; optional stack-graphs Strategy only. Pin inside ty’s window (ty: 3.10+; best on recent 3.x). |
| PHP | 8.5 | 8.4 | 8.3 | Tree-sitter | `use` + hierarchy | **PHPantom** preferred; else **static phpactor** only if fully static | **Not** intelephense. **Not** host `php`. Else T2 ceiling. |
| Java | 26 | 25 | 24 | Tree-sitter | name/arity, import, hierarchy, scope, jar `Proxy` `.class` | **none in v1** | **No JDT-LS / JVM.** kmp-lsp-style heuristics are inspiration, not a fork-by-default. |
| Go | 1.27 | 1.26 | 1.25 | Tree-sitter | `go.mod`/`go.work` + import paths | **gopls** `CGO_ENABLED=0` | Project `go` on PATH may be required for full types/cgo; else T2. Do not bundle a Go SDK. |
| Zig | pin with zls | −1 | −2 | Tree-sitter | `build.zig` / `@import` | **zls** | zls tracks Zig tightly; matrix lag expected. No project `zig` → T2. |

Exact compiler/grammar git SHAs belong in `xtask` / lockfiles. This table is the product window. M2 registers Java T1/T2, PHP T1/T2, HTML/CSS/JS T1, Go T1, Zig T1 (`lang-*` default-on). Remaining slots (C/C++/C#/Rust/Python) stay `UnsupportedLanguage`. Pins are not Cargo-bumped here.

**Java TSG (M2.3):** GitHub archived [stack-graphs](https://github.com/github/stack-graphs) (2025-09-09). Vendoring the archived C/Rust tree under `third_party/stack-graphs` is impractical on this milestone and would not beat name/arity/import/hierarchy/scope on the in-tree heuristic fixture. **Heuristics are the shipped T2 Strategy.** `StackGraphResolver` remains a `NotReady` Strategy slot. No `third_party/stack-graphs` vendor.

## Upstream lag policy

- Pin Tree-sitter grammars and engine versions here and in CI.
- Fixtures **per new syntax construct**.
- zls ↔ Zig coupling: workspace newer than pinned zls → T2 + capability note (same class as PHP engine lag).
- PHP engine may trail PHP LATEST; record `min(our window, engine)`.

## stack-graphs

GitHub **archived** the repo (2025-09-09). **Vendor a fork** under `third_party/stack-graphs/`. Shipped TSG exists for Java, JS, TS, Python (not C/C++/C#/PHP/HTML/CSS). TSG is **one T2 Strategy**. **Heuristic resolvers are the default.** Keep TSG only if it wins on correctness and memory (same adoption gate as the original Java strategy).

## Forbidden engines (never “better” in v1)

tsserver, JDT-LS, pylsp, pyright, ruff-as-types, intelephense, any Node/JVM/CPython language server as **our** T3.
