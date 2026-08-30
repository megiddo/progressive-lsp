# POC IDE architecture

Consumer sample. Product boundary for the **server** is unchanged: [../architecture.md](../architecture.md). This file is the POC only.

## Processes

```text
poc-ide (eframe)
  ├── FileTree / tabs / buffers / ConflictModal   (in-process)
  ├── LspClient Adapter  --stdio JSON-RPC-->  progressive-lsp serve
  └── ControlClient Adapter --unix socket Envelope-->  same serve (--control-socket)
```

One language-server process per workspace. **Selection of LSP backend** means: pick `ServeMode` (stock stdio vs control-socket) and set `textDocument/didOpen.languageId` from `LanguageCatalog`. It does **not** spawn clangd/ty/tsserver. Those stay inside `progressive-lsp` packs.

## Crate split

```text
poc-ide/                     workspace member; not a musl artifact
  src/lib.rs                 domain + ports
  src/main.rs               composition root: eframe, rfd (`RfdDialog`), wire ports
  src/ports.rs              DialogPort, ClipboardPort, FsPort, StdFs, WatchPort, ClockPort, LspTransport, ControlTransport
  src/layout.rs             LayoutState (left panel width)
  src/tree.rs               WorkspaceRoot, FileTree, TreeNode Composite, TreeExpansion
  src/tabs.rs               TabStrip, TabId
  src/buffer.rs             OpenBuffer, BufferMap, Selection, DirtyFlag
  src/edit.rs               EditCommand
  src/highlight.rs          Highlighter (syntect)
  src/conflict.rs           ConflictModal, ConflictChoice
  src/language.rs            LanguageCatalog, ServeMode
  src/lsp.rs                LspClient Facade, LspLocation, SpawnSpec, StdioLsp
  src/control.rs             ControlClient Adapter, UnixControl
  src/console.rs             ProtocolConsole Facade, TranscriptEntry
  src/watch.rs              DiskWatch Observer, NotifyWatch
  src/log.rs                RunLog Repository (per-run sqlite debug sink)
```

**Dependency rule:** the lib does not import `egui` / `eframe` / `egui_dock` / `rfd`. Those belong in `main.rs`. Tests never open a window.

Allowed lib deps: `ropey`, `syntect`, `walkdir`, `lsp-types`, `serde_json`, `thiserror`, `progressive-lsp-control`, `rusqlite` (`bundled`, poc-ide only — never the musl server crates), `notify` only behind `WatchPort` (prod adapter in lib or thin `notify` adapter; tests inject `FakeWatch`).

## Data flow

1. `DialogPort.open_folder` / `open_file` → `WorkspaceRoot` (canonical absolute path). File → parent directory is the root; that file is also opened as a tab.
2. `FsPort.read_tree` → `FileTree` shallow load of the workspace root's immediate children (skip `.git/`, `target/`, `node_modules/` — display filter, not server ignore). Child directories start unloaded. `TreeExpansion` starts empty (collapsed at every level). `FileTree::expand` lists one more directory via `FsPort.read_dir` when the user expands a path.
3. Click a file → `BufferMap.open` (read bytes, `LanguageCatalog.for_path`, `didOpen` if LSP is up).
4. Keystrokes → `EditCommand` on `OpenBuffer` → dirty → `didChange` incremental.
5. Save → `FsPort.write` → clear dirty → `didSave`.
6. `WatchPort` events for an open path → if the buffer is open, enqueue `ConflictModal` (always; even if clean). Choice `LoadDisk` replaces rope and clears dirty; `KeepMemory` keeps rope and records `ignored_mtime`.
7. Go to definition / implementation / references → `LspClient` → jump opens or focuses a tab at `LspLocation`.
8. Protocol console sends raw LSP methods or Envelope methods; transcripts are append-only `TranscriptEntry` DTOs.

## Ports (inject always)

| Port | Production | Test double |
|---|---|---|
| `DialogPort` | `RfdDialog` (`rfd` in main) | `FakeDialog` |
| `ClipboardPort` | `arboard` or egui clipboard in main | `FakeClipboard` |
| `FsPort` | `StdFs` (`std::fs`) | `MemFs` |
| `WatchPort` | `NotifyWatch` (`notify` adapter) | `FakeWatch` |
| `ClockPort` | reuse `progressive-lsp-core::ClockPort` **or** a local copy of the trait in poc-ide to avoid pulling core if that crate is too server-shaped | `FakeClock` |
| `LspTransport` | `StdioLsp` (child stdio Content-Length) | `FakeLsp` |
| `ControlTransport` | `UnixControl` (Unix socket + `encode_frame` / `decode_frame`) | `FakeControl` |
| `RunLog` (Repository) | rusqlite file under run-log dir | `:memory:` / tempfile path |

Prefer a **local `ClockPort`** in poc-ide (same invariant: tests never `thread::sleep`) rather than depending on `progressive-lsp-core`. Do not take a dependency on core just for the clock.

`RunLog` is a per-process sqlite Repository. Each `cargo run -p poc-ide` writes a new file under `$HOME/.progressivelsp/poc-ide-runs/poc-ide-{unix_ms}-{pid}.sqlite` (override with `POC_IDE_LOG_DIR`). Tests inject `:memory:` or a tempfile path. Rows are `timestamp_ms`, `category`, `event`, optional JSON payload (method + error, never file bodies / clipboard / secrets). A failed write is `IdeError::Log` and is ignored at the composition root — the editor does not panic.

## LSP client

Content-Length JSON-RPC, same shape as the integration harness, **copied as a new Adapter** — do not add a crate dep on `integration/harness`. Methods used in v1 of the POC:

- `initialize` / `initialized` / `shutdown` / `exit`
- `textDocument/didOpen` / `didChange` / `didSave` / `didClose`
- `textDocument/definition` / `implementation` / `references`
- Read `capabilities.experimental.progressiveLsp` (socket may be null in stock mode)

Unknown server methods in the console: send anyway; show the JSON-RPC error. Jump targets are `LspLocation` (uri + range). `SpawnSpec` resolves the `progressive-lsp` binary (env, then `target/{debug,release}/progressive-lsp`, then `PATH`); missing binary is a domain error, not a panic.

## Control client

Only when `ServeMode::ControlSocket`. Spawn:

```text
progressive-lsp serve --control-socket PATH [--prefix DIR]
```

After `initialize`, connect to `experimental.progressiveLsp.socket` (must match). Speak `Envelope` (`method`, `request_id`, `body`) as in [../user/progressive-v1-api.md](../user/progressive-v1-api.md). Console must be able to invoke every unary RPC in the RPC table and display `WatchBatch` / `TierReady` pushes (`request_id == 0`).

`--mux` stays `pending_mux` (same as IT-3). Do not silently retest the socket as mux.

## Language catalog

| Extensions | `languageId` |
|---|---|
| `.rs` | `rust` |
| `.py` | `python` |
| `.java` | `java` |
| `.js` `.mjs` `.cjs` `.jsx` | `javascript` |
| `.ts` `.tsx` | `typescript` |
| `.php` | `php` |
| `.html` `.htm` | `html` |
| `.css` | `css` |
| `.go` | `go` |
| `.zig` | `zig` |
| `.c` `.h` | `c` |
| `.cpp` `.cc` `.cxx` `.hpp` `.hxx` `.hh` | `cpp` |
| `.cs` | `csharp` |

Unknown extension → `plaintext`. Buffer still opens. LSP `didOpen` is skipped for `plaintext` (no factory). Override map is a `LanguageCatalog` method for tests; production table is the table above.

## UI mapping (bin only)

- Left: `egui::Panel::left("tree").resizable(true)` bound to `LayoutState.left_width` (egui 0.36 renamed `SidePanel` to `Panel`).
- Center: `TabStrip` rendered with a thin custom tab bar in `ui.rs` (egui_dock 0.21 rust-version 1.95 does not pin on this workspace’s rustc). Same `TabStrip` tests.
- Editor: `egui::TextEdit::multiline` + syntect layouter from `egui_extras` (or a galley built from `Highlighter` tokens). Rope is source of truth; the widget is a view.
- Modal: `egui::Modal` / `Window` for `ConflictModal`.
- Bottom or right: `ProtocolConsole` (collapsible); transcript rows are append-only `TranscriptEntry` DTOs.

## Failure modes

- Missing `progressive-lsp` binary: editor still edits/highlights; discovery commands return a domain error; console shows spawn failure. Not a panic.
- Control socket refused: stock LSP remains; console control tab errors.
- T3 stub / method empty: show empty location list; do not fake a hit.
