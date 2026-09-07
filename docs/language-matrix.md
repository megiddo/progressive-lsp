# Language matrix (v1)

Rolling pin of **latest language spec plus two prior**. CI fails if a matrix fixture regresses. Grammar/engine upgrades are **scheduled work**, not drive-by Cargo bumps.

**Effective supported version** = `min(our window, Tree-sitter grammar, T3 engine)`. Surface that in `initialize` / capabilities so a client can show “syntax unparsed.”

Unknown syntax → ERROR nodes; **server stays up**.

Detect per-package from manifests (`composer.json` `php`, Cargo edition, cmake/`-std=`, `LangVersion`, pom `release`, `requires-python`, `go` directive, Zig in `build.zig.zon` / toolchain). A workspace may **mix** versions within the window.

Window below is **as of 2026-08**. Update the table when LATEST moves; keep fixtures for LATEST, LATEST-1, LATEST-2.

## Window (pins)

| Language | LATEST | LATEST-1 | LATEST-2 | T1 | T2 | T3 engine | Notes |
|---|---|---|---|---|---|---|---|
| C | C23 | C17 | C11 | Tree-sitter | **heuristic** (`#include`, name/arity) — [t2-heuristic-coverage.md](t2-heuristic-coverage.md) | **clangd** | Needs `compile_commands.json`. One-shot `cmake -DCMAKE_EXPORT_COMPILE_COMMANDS` only if the project already uses CMake — do not invent a build. T2 not landed until T2-COV. |
| C++ | C++26 | C++23 | C++20 | Tree-sitter | **heuristic** (include, namespace, name/arity) — T2-COV | **clangd** | Same compile_commands rule. |
| C# | 14 | 13 | 12 | Tree-sitter | heuristics | **T1/T2 ceiling** (no csharp-ls pack) | Spike failed-closed on Darwin: no musl AOT ELF (`spike/csharp-ls.md`). No host `dotnet`. Census does not select csharp-ls. |
| Rust | edition 2024 | 2021 | 2018 | Tree-sitter | **heuristic** (`use`/`mod`/`fn`/`impl`) — T2-COV | **rust-analyzer** | Sysroot / proc-macro `.so` are **project** artifacts. No pack or no sysroot → T2 heuristics then T1; T3 only with pack + Linux sysroot. |
| JavaScript | ES2026 | ES2025 | ES2024 | Tree-sitter | **heuristic import/export** (oxc crates not wired) | **tsgo** | **Not** tsserver/Node. oxc_resolver/oxc_semantic evaluated; heuristic T2 Strategy ships. oxc_type_checker does not block tsgo. |
| TypeScript | current 3-release window | | | Tree-sitter | heuristic import/export | **tsgo** | Same T2 as JS. Fake tsgo covers go-to-type / generics. Pin exact TS versions in CI fixtures. |
| CSS | current | −1 | −2 | Tree-sitter | **heuristic** (selectors / custom properties) — T2-COV | **biome** | No Node CSS LS. Adapter + T1/T2 fallback. Darwin musl-clean unknown; real T3 is Linux. |
| HTML | current | −1 | −2 | Tree-sitter | **heuristic** (`id`/`class`/`href`) — T2-COV | **superhtml** (Zig) | Fallback T1/T2 if pack absent. |
| Python | 3.14 | 3.13 | 3.12 | Tree-sitter | **heuristic** (`import`/`def`/`class`) — T2-COV; optional TSG still opt-in | **ty** | **Not** CPython, pylsp, pyright, ruff-as-types. Pin inside ty’s window (ty: 3.10+; best on recent 3.x). |
| PHP | 8.5 | 8.4 | 8.3 | Tree-sitter | `use` + hierarchy | **PHPantom** (winner) | **Not** intelephense. **Not** host `php`. T3 when phpantom pack installed; else T2. Static phpactor not shipped. |
| Java | 26 | 25 | 24 | Tree-sitter | name/arity, import, hierarchy, scope, jar `Proxy` `.class` | **javac LS** (Graal native-image; x86_64 fully static, aarch64 may need libc) | **No JDT-LS / JVM.** [t3-linux-hosts.md](t3-linux-hosts.md). Heuristics remain T2. |
| Go | 1.27 | 1.26 | 1.25 | Tree-sitter | `go.mod`/`go.work` + import paths | **gopls** `CGO_ENABLED=0` | Project `go` on PATH may be required for full types/cgo; else T2. Do not bundle a Go SDK. |
| Zig | pin with zls | −1 | −2 | Tree-sitter | `build.zig` / `@import` | **zls** | zls tracks Zig tightly; matrix lag expected. No project `zig` → T2. |

Exact compiler/grammar git SHAs belong in pack `manifest.json` / lockfiles, **not** in the core crate semver (workspace **0.1.0**). This table is the product window. **Every v1 language has T1 and heuristic T2** ([t2-heuristic-coverage.md](t2-heuristic-coverage.md); T2-COV not all landed). T3 is a static Linux backend in the container / production host ([t3-linux-hosts.md](t3-linux-hosts.md)). C# T1/T2 ceiling. Java T3 is x86_64 Linux only until Graal fully-static ARM exists. Real T3 packs are Linux; Darwin dist stubs are not greens. Pins are not Cargo-bumped here. Pass %: [conformance.md](conformance.md).

**M5 fixtures (2026-08 window):** `fixtures/matrix/<lang>/<LATEST|LATEST-1|LATEST-2>/` holds representative syntax (not a full stdlib) for every row above. TypeScript pins in-tree are 5.9 / 5.8 / 5.7. CSS/HTML/Zig use `current` / `prior` / `prior2` labels where the matrix does not name a spec number. `fixtures/matrix/mixed/` is one workspace mixing Java 26, Python 3.12, Rust 2021, PHP 8.3, JS ES2024, Go 1.25, C11. `fixtures/lag/` has newer-than-window / unparsed samples for Java, PHP, JavaScript, Python, Rust, and C. `cargo test` on Darwin stands in for matrix CI green; Linux CI must run the same fixtures.

**Java TSG (PD4):** GitHub archived [stack-graphs](https://github.com/github/stack-graphs) (2025-09-09). Pin `https://github.com/github/stack-graphs.git` @ `fcb7705d5b38ae13b3665a9b2c882e5a97243d44` (Cargo git dep / fetch-at-SHA). No `third_party/` dump. Fetch succeeded. `build_stack_graph_into` panics on workspace `tree-sitter-java` 0.23.5 vs pin `=0.23.4` — stitch column is `skip_runtime`. **Heuristics remain the default T2.** `[t2] java = "stack-graphs"` is opt-in. Results: [spikes/t2-bakeoff-results.md](spikes/t2-bakeoff-results.md).

## Upstream lag policy

- Pin Tree-sitter grammars and engine versions here and in CI.
- Fixtures **per new syntax construct**.
- zls ↔ Zig coupling: workspace newer than pinned zls → T2 + capability note (same class as PHP engine lag).
- PHP engine may trail PHP LATEST; record `min(our window, engine)`.

## stack-graphs

GitHub **archived** the repo (2025-09-09). Pin git URL + SHA (not a vendor dump). Shipped TSG exists for Java, JS, TS, Python (not C/C++/C#/PHP/HTML/CSS). TSG is **one T2 Strategy**. **Heuristic resolvers are the default for every v1 language** ([t2-heuristic-coverage.md](t2-heuristic-coverage.md)). Keep TSG as default only if it wins on correctness and memory (PD4 bake-off: it did not; see [spikes/t2-bakeoff-results.md](spikes/t2-bakeoff-results.md)).

## Forbidden engines (never “better” in v1)

tsserver, JDT-LS, pylsp, pyright, ruff-as-types, intelephense, any Node/JVM/CPython language server as **our** T3.
