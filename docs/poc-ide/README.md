# POC IDE (consumer sample)

An in-tree **proof-of-concept editor** that consumes `progressive-lsp`. It is **not** the language-intelligence product. The server still does not own pixels, git, SSH, or a PTY. This sample exists so we can open a folder, edit buffers, and exercise both stock LSP and `progressive.v1` against a real UI.

Related: [architecture.md](architecture.md), [third-party.md](third-party.md), [agent-context.md](agent-context.md), [../consumer.md](../consumer.md), [../lsp-contract.md](../lsp-contract.md), [../user/progressive-v1-api.md](../user/progressive-v1-api.md).

## What it must do

| Capability | How |
|---|---|
| Open folder or open file | `DialogPort` → native `rfd` in the bin; tests inject `FakeDialog` |
| Tree, tabs, editor, resizable left panel | Domain `FileTree` + `TabStrip` + `LayoutState`; eframe `SidePanel` in the bin |
| Syntax highlighting | `Highlighter` Adapter over **syntect** (egui layouter in the bin) |
| Background disk edits | `DiskWatch` Observer; dirty buffer → `ConflictModal` (load from disk or keep in memory) |
| Edit | `EditCommand`: insert, delete, select, cut, copy, paste, open, save |
| Discovery | Stock LSP `textDocument/definition`, `implementation`, `references` |
| Language by extension | `LanguageCatalog` Registry; one `progressive-lsp serve` per workspace |
| Exercise protocols | `ProtocolConsole` Facade: JSON-RPC inspector + Envelope RPCs |
| Per-run debug log | `RunLog` Repository → sqlite under `~/.progressivelsp/poc-ide-runs/` (or `POC_IDE_LOG_DIR`); one file per process |

## What it must not do

- Fork resolvers, vendor engine packs, or depend on `progressive-lsp-plugin` / `-resolve` / `-index`.
- Put FilesSince on `$/` or `workspace/filesSince`.
- SSH, git porcelain, PTY, file-tree create/rename/delete (view + open/save only).
- Node, Electron, Monaco, CodeMirror, VS Code, JVM, or CPython as the editor runtime.
- Ship as a musl ELF. `check-static` does not apply. `xtask musl` still builds only `--bin progressive-lsp`.

## Crate

`poc-ide/` is a workspace member. Library = testable domain. `src/main.rs` = composition root (eframe + `rfd`). Consumers of the server may depend on `progressive-lsp-control`; this sample does.

Run (after IDE-1+):

```text
cargo run -p poc-ide
cargo run -p poc-ide -- --folder DIR
cargo run -p poc-ide -- --file PATH
```

Spawn of `progressive-lsp serve` uses (first hit wins): `PROGRESSIVE_LSP` env, `target/{debug,release}/progressive-lsp`, then `progressive-lsp` on `PATH`.

## Milestones

Stacked on current `main` (not on `pd4` / `m6` history). Branches: `ide0` → `ide5`, then post-IDE-5 slices `poc-log` and `poc-tree-lazy` (not IDE-6). Exits: [../milestones.md](../milestones.md). Work packages: [../implementation-plan.md](../implementation-plan.md). Patterns: [../design-patterns.md](../design-patterns.md). Hygiene: [../testing.md](../testing.md).
