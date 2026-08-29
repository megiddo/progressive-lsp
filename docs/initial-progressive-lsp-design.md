> **ARCHIVE — not the source of truth.**
>
> This file is the original sketch. It is **wrong** on several locked decisions (ruff as Python T3, stc, no watches, fat clangd, Java/PHP T3 = stack-graphs only, Zig/Go out, no control plane).
>
> Implement from **[README.md](README.md)** and the rest of this `docs/` tree: [vision.md](vision.md), [requirements.md](requirements.md), [architecture.md](architecture.md).

Here is the complete plan re-rendered in plain Markdown format, stripped of custom UI components:

---

# Multi-Language Progressive LSP Implementation Plan

## Phase 1: Architecture & Host Stack

To maintain strict static compilation and zero host runtime dependencies, the system architecture separates the host LSP server from the language resolution drivers.

* **Core Engine Language:** Rust (compiled with the `x86_64-unknown-linux-musl` target or statically linked MSVC runtime on Windows).
* **Protocol / Network Layer:** `tower-lsp` and `lsp-types` in Rust to handle JSON-RPC message passing over standard input/output (stdio).
* **Parser Engine:** Tree-sitter (C core library, linked directly via Rust C-FFI).
* **Lightweight Definition Engine:** GitHub’s `stack-graphs` (native Rust crate).

### Target Execution Matrix

| Target Language | Phase 1: Progressive Tier (Tree-Sitter) | Phase 2: Progressive Tier (Stack Graphs) | Phase 3: Native Full Resolution Server | Host Runtime Required |
| --- | --- | --- | --- | --- |
| **C / C++** | Tree-sitter C/C++ | Stack Graphs TSG | `clangd` (Statically linked binary) | None |
| **C#** | Tree-sitter C# | Stack Graphs TSG | `csharp-ls` (Statically compiled AOT via `.NET Native AOT`) | None |
| **Rust** | Tree-sitter Rust | Stack Graphs TSG | `rust-analyzer` (Statically linked binary) | None |
| **JavaScript / TypeScript** | Tree-sitter JS/TS | Stack Graphs TSG | Built-in Rust Semantic Engine (`stc` / `oxc` crate embedded) | None |
| **Python** | Tree-sitter Python | Stack Graphs TSG | `ruff` (LSP mode in Rust) + Embedded Stack Graph engine | None |
| **HTML / CSS** | Tree-sitter HTML/CSS | Stack Graphs TSG | `biome` (Rust binary) / `superhtml` (Zig/C ABI static binary) | None |
| **Java** | Tree-sitter Java | Stack Graphs TSG | Fallback to Stack Graph Deep Resolution | None |
| **PHP** | Tree-sitter PHP | Stack Graphs TSG | Fallback to Stack Graph Deep Resolution | None |

---

## Phase 2: The Progressive Result Pipeline

When a workspace or file is opened, the LSP engine returns results incrementally through a three-stage progressive resolution pipeline.

```text
       [ incoming LSP Query: textDocument/definition ]
                            │
                            ▼
 ┌─────────────────────────────────────────────────────┐
 │ Tier 1: Tree-Sitter CST Parsing (~0-10ms)           │
 │  • Immediate syntax highlighting & Semantic Tokens  │
 │  • Local intra-file lexical scope resolution        │
 └──────────────────────────┬──────────────────────────┘
                            │ (Async Indexing In Progress)
                            ▼
 ┌─────────────────────────────────────────────────────┐
 │ Tier 2: Stack Graphs Graph Construction (~100ms-1s) │
 │  • Cross-file scoping rules via TSG DSL             │
 │  • Partial "Go to Definition" & "Find References"   │
 └──────────────────────────┬──────────────────────────┘
                            │ (Full Engine Background Load)
                            ▼
 ┌─────────────────────────────────────────────────────┐
 │ Tier 3: Full Semantic Engine / Native LSP (1s-10s+) │
 │  • Complete type checking & generic instantiation   │
 │  • "Find Implementation" & exact target resolution  │
 └─────────────────────────────────────────────────────┘

```

### Tier 1: Instant Local Syntax & Scope (~0–10ms)

* **Components:** C-based Tree-sitter parsers linked statically into the core Rust executable.
* **Capabilities:**
* Full syntax highlighting (via `tree-sitter-highlight`).
* `textDocument/semanticTokens` generation.
* Local variable lookup within the same block or function buffer.


* **Storage:** In-memory Concrete Syntax Tree (CST) updated incrementally on every keystroke (`textDocument/didChange`).

### Tier 2: Workspace Stack Graphs (~100ms–1s)

* **Components:** `stack-graphs` Rust library.
* **Capabilities:**
* Cross-file symbol resolutions without executing language compilers.
* Constructs a directed graph of definition nodes, reference nodes, and scope stack transitions using Tree-sitter Query (TSG) rules.


* **Operation:** As the workspace files are read, TSG patterns populate the stack graph asynchronously in background threads, enabling cross-file "Go to Definition" and "Find References" before Tier 3 completes initialization.

### Tier 3: Deep Type Resolution Engine (~1s–10s+)

* **Components:** Native, statically compiled binaries spawned as managed subprocesses via stdio channels or embedded Rust compilation passes.
* **Capabilities:**
* Exact type inference, full type checking, resolution of complex generic constraints, and "Find Implementation".


* **Progressive Handoff Mechanism:**
1. LSP request (e.g., `textDocument/definition`) arrives at the core host router.
2. If Tier 3 is ready and indexed, the host delegates the query directly to Tier 3.
3. If Tier 3 is still loading package dependencies or indexing, the core host returns Tier 2 (Stack Graphs) results immediately without blocking the client.



---

## Phase 3: Integration of Open Source Components

To avoid reinventing existing compiler engines, the architecture delegates to existing open-source projects, modified only for static compilation:

1. **Syntax Highlighting & Formatting:**
* **Tree-sitter Grammars:** Link native `.c` sources for `tree-sitter-c`, `tree-sitter-cpp`, `tree-sitter-c-sharp`, `tree-sitter-rust`, `tree-sitter-javascript`, `tree-sitter-typescript`, `tree-sitter-python`, `tree-sitter-java`, `tree-sitter-php`, `tree-sitter-html`, and `tree-sitter-css`.
* **Formatting:** Integrate `biome` (Rust binary) for JS/TS/CSS/JSON and `ruff` (Rust binary) for Python.


2. **Full Semantic Engine Strategy:**
* **C / C++:** Link or bundle static `clangd` binaries built using LLVM's `BUILD_SHARED_LIBS=OFF`.
* **Rust:** Integrate `rust-analyzer` compiled as a standalone static binary (`x86_64-unknown-linux-musl`).
* **C#:** Compile `csharp-ls` using .NET's Native AOT mode (`dotnet publish -r linux-x64 -c Release /p:PublishAot=true`), producing a dependency-free native executable.
* **JavaScript / TypeScript:** Embed the `oxc` / `stc` Rust libraries into the main engine process for AST type inference.
* **Java / PHP / Python (Runtime-Free Strategy):** Provide Tier 1 and Tier 2 capabilities (Tree-sitter + Stack Graphs) natively inside the core binary, and integrate `ruff` (Rust) for Python semantic diagnostics. This delivers fast definitions, references, and type tokens without introducing JVM, Node.js, or PHP host requirements.



---

## Phase 4: Build System & Static Distribution Strategy

To guarantee zero dependencies on target machines, all output binaries are produced via a unified C++ / Rust static toolchain setup:

* **Build Tool:** `cargo` combined with `cmake` / `ninja` for cross-language static linking.
* **C/C++ Standard Libraries:**
* **Linux:** Statically link against `musl-libc` and `libstdc++` (`-static-libgcc -static-libstdc++`).
* **Windows:** Statically link the C Runtime (`/MT` flag in MSVC).
* **macOS:** Statically link custom dependencies, dynamically linking only system frameworks (`CoreFoundation`, `libSystem`).



### Output Artifact Structure

A single distribution package contains:

```text
bin/
├── progressive-lsp-core    # Statically linked Rust host server (Tree-sitter + Stack-graphs)
├── clangd                  # Statically compiled Clangd binary
├── rust-analyzer           # Statically compiled Rust Analyzer binary
├── csharp-ls               # Native AOT compiled C# language server
└── ruff                    # Statically compiled Rust-based Python server

```

The `progressive-lsp-core` host executable acts as the single frontend endpoint for the IDE client, orchestrating internal Tier 1 / Tier 2 requests and delegating to child processes when Tier 3 indexing completes.