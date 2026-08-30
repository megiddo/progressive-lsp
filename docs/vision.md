# Vision

progressive-lsp is a **language-intelligence server**: parse, index, resolve, and answer LSP questions on a Linux host with **no dynamic linking** of our artifacts and **no Node / JVM / CPython** as the server runtime.

It is **not** a remote IDE, SSH agent, file tree, git UI, or terminal. Those belong to a consumer (example: zeds-dead). This product is easy for that consumer to use and equally usable from Neovim or a test harness.

Related: [requirements.md](requirements.md), [architecture.md](architecture.md), [consumer.md](consumer.md).

## Principles

1. **Intelligence vs presentation.** We own symbols, types, and indexes. The client owns pixels and keybindings.
2. **Stock LSP always works.** `progressive-lsp serve` on stdio is enough for definition, references, hover, tokens, and disk-watch reindex. No protobuf required.
3. **Progressive clients opt in.** `progressive.v1` protobuf adds FilesSince, WatchBatch, live config, pack install, tier status. Discovery is LSP `capabilities.experimental.progressiveLsp`.
4. **Progressive result quality.** Tree-sitter (T1) → heuristics / stack graphs (T2) → full engine (T3). Never block the editor on package ingest.
5. **Static artifacts only.** No `DT_NEEDED` on files we ship. musl-static is the default flavor; glibc-static is an optional second flavor with the same bar.
6. **Few host dependencies.** Core boots with zero language runtimes. Engine packs are optional. Project compilers (`go`, `zig`, `rustc` sysroot) are project artifacts, not our libc.
7. **Git-safe home.** Writable state lives in `$HOME/.progressivelsp/`. A workspace overlay is optional and shareable; cache/sockets never belong in a git tree.
8. **Named patterns, tested cores.** Every component maps to [design-patterns.md](design-patterns.md). Coverage and mutants are merge gates ([testing.md](testing.md)).
9. **Leverage existing engines.** clangd, rust-analyzer, ty, tsgo, gopls, zls, csharp-ls, biome/superhtml, PHPantom — statically compiled. Do not reimplement a compiler when an allowed-language engine exists.
10. **Upstream lag is a matrix, not hope.** LATEST + two prior language versions, pinned grammars/engines, fixtures, degrade instead of panic ([language-matrix.md](language-matrix.md)).

## Anti-goals

| Anti-goal | Why |
|---|---|
| Be zeds-dead-host (SSH, git, PTY, file tree, IDE config) | Wrong product. Example consumer only. In-tree `poc-ide/` is a separate consumer sample, not this process. |
| Node, JVM, or CPython as the language-server runtime | Portability and static-deploy. |
| Dynamic linking of our ELFs | The painful solution already exists. |
| `$/` JSON-RPC for FilesSince / pack install in v1 | Legal LSP extension, nonstandard *use*; stock clients ignore it anyway. |
| Scripts that implement go-to-definition | Resolver chain only. |
| Java T3 via JDT-LS | JVM forbidden. Java is T1/T2 in v1. |
| Host `php` on PATH as T3 | Someone else’s dynamic binary. |
| Collaboration, CRDTs, in-product agents | Out of scope. |

## Success bar

- A stock LSP client on a fresh Linux host: static core binary, highlighting and F12 at T1/T2, no Node/JVM/CPython installed for the server.
- A progressive client: same, plus FilesSince catch-up after a watch overflow, optional engine packs, no fork of this repo.
- Opening a PHP or Java workspace does not require clangd (or a PHP interpreter) to be present.
- Second connection or second workspace reuses `$HOME/.progressivelsp/` without rewriting the git tree.

## Users

**Primary:** an editor or remote-IDE host that wants language intelligence on a minimal Linux box.

**Secondary:** a human who drops `config.toml` and Rhai hooks in `.progressivelsp` to filter watches or skip an engine.

**Non-users:** people who want us to run tsserver, JDT, or pylsp.
