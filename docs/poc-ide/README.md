# POC IDE (consumer sample)

An in-tree **proof-of-concept editor** that consumes `progressive-lsp`. It is **not** the language-intelligence product. The server still does not own pixels, git, SSH, or a PTY. This sample exists so we can open a folder, edit buffers, and exercise both stock LSP and `progressive.v1` against a real UI.

Related: [architecture.md](architecture.md), [third-party.md](third-party.md), [agent-context.md](agent-context.md), [proof-agent-context.md](proof-agent-context.md), [../host/agent-context.md](../host/agent-context.md), [../consumer.md](../consumer.md), [../lsp-contract.md](../lsp-contract.md), [../user/progressive-v1-api.md](../user/progressive-v1-api.md).

## What it must do

| Capability | How |
|---|---|
| Open folder or open file | `DialogPort` → native `rfd` in the bin; tests inject `FakeDialog`. Non-Linux: **Open Folder…** is native T1/T2; **Open Folder in Container…** is one Linux serve (T1/T2/T3) with identity bind-mount (same `file:` URIs; no rewriter). Linux: Open Folder is the full host. |
| T1/T2/T3 strip | Buttons. Click opens `StatusModal` + `LaunchJournal`. Native non-Linux T3 is `skipped` (`open folder in container`). C# T3 is `not supported` (T1/T2 ceiling). After T2-COV every v1 language has T2. Java T3 follows the static pack on Linux. Stub refuse is `skipped`, not `done`. |
| Tree, tabs, editor, resizable left panel | Domain `FileTree` + `CompactChain` + `TabStrip` + `LayoutState`; eframe `SidePanel` in the bin. Single-child dir chains render as `a/b/c`. |
| Syntax highlighting | `Highlighter` Adapter + `HighlightCache` over **syntect** (egui layouter in the bin; keyed by path + rope generation) |
| Tree expand | `TreeIoRequest` / `ExpandChainCommand` on a background worker; header may look open with `loading…` until the inbox fills |
| Background disk edits | `DiskWatch` Observer; dirty buffer → `ConflictModal` (load from disk or keep in memory) |
| Edit | `EditCommand`: insert, delete, select, cut, copy, paste, open, save |
| Discovery | Stock LSP `textDocument/definition`, `implementation`, `references` (Navigate, F12, editor and file-tree context menu). Items use `DiscoverOffer` × current tier; disabled labels are honest (`connecting language server` / `building T1 index` / `waiting for server` / `needs T2` / `needs T3` / `not supported` / `T3 skipped (stub pack)` / `open folder in container`). |
| Language by extension | `LanguageCatalog` Registry + `DiscoverOffer`; one `progressive-lsp serve` per workspace |
| Per-run debug log | `RunLog` Repository → sqlite under `~/.progressivelsp/poc-ide-runs/` (or `POC_IDE_LOG_DIR`); one file per process. No hand-typed protocol console in the bin. `ProtocolConsole` stays a lib Facade for Envelope/LSP transcript tests. |

## What it must not do

- Fork resolvers, vendor engine packs, or depend on `progressive-lsp-plugin` / `-resolve` / `-index`.
- Put FilesSince on `$/` or `workspace/filesSince`.
- SSH, git porcelain, PTY, file-tree create/rename/delete (view + open/save only).
- Node, Electron, Monaco, CodeMirror, VS Code, JVM, or CPython as the editor runtime.
- Ship as a musl ELF. `check-static` does not apply. `xtask musl` still builds only `--bin progressive-lsp`.

## Crate

`poc-ide/` is a workspace member. Library = testable domain. `src/main.rs` = composition root (eframe + `rfd`). Consumers of the server may depend on `progressive-lsp-control`; this sample does.

Supported proof launch (`./build`; `./build help` / `help lsp` / `help ide` / `help run`):

```text
./build lsp {all|x86_64|aarch64}        # Linux static LSP (controller + backends + image)
./build ide                             # POC editor binary (does not start it)
./build run ide                         # start the POC editor
./build run ide --folder DIR
./build run ide --folder DIR --container
```

Omit the architecture after `./build lsp` to print the valid list. `run ide` rebuilds native `progressive-lsp`, then `cargo run -p poc-ide` with `PROGRESSIVE_LSP` set to that artifact. Container mode needs Docker Desktop, `./build lsp <arch>`, and an **absolute** folder path. `./build` is a shell wrapper around `cargo xtask` (no Python/Node).

Bare `cargo run -p poc-ide` is **not** the supported proof launch: it does not rebuild `progressive-lsp`. Spawn of `progressive-lsp serve` still uses (first hit wins): `PROGRESSIVE_LSP` env, `target/{debug,release}/progressive-lsp`, then `progressive-lsp` on `PATH`.

## Milestones

Stacked on current `main` (not on `pd4` / `m6` history). Branches: `ide0` → `ide5`, then post-IDE-5 slices `poc-log` … `poc-discover-log`, then LOG-0–LOG-11, then the POC-proof stack `poc-proof-log` → `poc-lsp-async` → `poc-tier-status` → `poc-no-stall` (not IDE-6, not `log12`). Host stack starts at `host0` on `poc-no-stall` ([host/agent-context.md](../host/agent-context.md)); do not reopen POC-proof WPs. Exits: [../milestones.md](../milestones.md). Work packages: [../implementation-plan.md](../implementation-plan.md). Patterns: [../design-patterns.md](../design-patterns.md). Hygiene: [../testing.md](../testing.md). Proof orchestrators: [proof-agent-context.md](proof-agent-context.md).
