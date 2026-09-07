# T2 heuristic coverage

Every v1 language gets a **heuristic T2** in the core (same process as T1). No engine pack. No JVM/Node/CPython.

This file is the T2 design. Order and checklists: [poc-tier-plan.md](poc-tier-plan.md) (POC-T2). Work packages: [implementation-plan.md](implementation-plan.md) (T2-COV.*). Matrix: [language-matrix.md](language-matrix.md).

## Why

Vision is T1 → T2 → T3. Several languages skip T2 today (C, C++, Rust, Python, CSS, HTML). Local POC **Open Folder** is T1/T2 only, so those languages never get a graph-quality step on the Mac. That is a product hole, not a host limitation.

T3 in the Linux container does not replace this. T2 must work when T3 is skipped (Mac native) and when T3 is not ready yet (container, ingest still running).

## What T2 is

Same `HeuristicResolver` already in `progressive-lsp-resolve`: name, arity, import/include, hierarchy, scope, over the **graph index** for that language.

T2 is **not** types. Wrong overload / missed macro / CSS cascade is acceptable. Empty or T1-only when the indexer has no graph facts is a bug after this work lands.

C# stays T1/T2 (no T3). These six languages gain T2; they keep their T3 engines in the container.

## Per language (heuristic facts)

Reuse the Java/PHP/JS pattern: indexer `extract` / `extract_graph` fills symbols + edges; factory puts `T2Strategy` (default `heuristic`) between T3 and T1.

| Language | Graph facts T2 uses | Do not do |
|---|---|---|
| C | `#include` as imports; function name + arity; file-scope types | Parse `compile_commands.json` (that is T3/clangd) |
| C++ | `#include`; namespaces; name + arity (overloads); class/struct as containers | Template instantiation |
| Rust | `use` / `mod`; `fn` / `struct` / `enum` / `impl` names; methods on a type | Macro expansion, trait solving (T3 / rust-analyzer) |
| Python | `import` / `from`; `def` / `class` names; methods on a class | CPython types, optional stack-graphs as default (still opt-in) |
| CSS | selectors, custom properties, `@keyframes` names as symbols; same-name jump | Full cascade / specificity engine |
| HTML | `id` / `class` as symbols; `href` / `src` as import-like edges to same-tree files | A browser |

Existing T2 languages (Java, C#, JS, TS, PHP, Go, Zig) are unchanged.

## POC / catalog

`LanguageCatalog.has_t2` is true for every v1 `languageId` except `plaintext`. Strip T2 `n/a` goes away for Rust/CSS/HTML/Python/C/C++. Discover **Implementation** may still say `needs T2` until ingest reaches graph, then enable where that language’s heuristics answer it.

## Tests

Fixtures under `fixtures/matrix/` plus small heuristic corpora (same class as `fixtures/java-heuristic/`). Score definition/references at T2. No Docker. No `thread::sleep`. Conformance T2 cells leave `N/A` only for languages we explicitly refuse (none after this WP; C# T3 stays N/A).

## Out of scope

Stack-graphs as default (PD4 already lost). New T3 engines. C# T3. Changing the resolver chain order.
