# POC IDE architecture

Consumer sample. Product boundary for the **server** is unchanged: [../architecture.md](../architecture.md). This file is the POC only.

## Processes

```text
poc-ide (eframe)
  ├── FileTree / tabs / buffers / ConflictModal / StatusModal   (in-process)
  ├── LspClient Adapter  --stdio JSON-RPC-->  progressive-lsp serve
  │         native serve on Linux (T1/T2/T3) or Darwin T1/T2-only
  │         **or** one Linux container host (Open Folder in Container)
  └── ControlClient Adapter --unix socket Envelope-->  same serve (--control-socket)
```

One language-server process per workspace. **Never two serves** (no local T1 + container T3). **Selection of LSP backend** means: pick `ServeMode` and `OpenMode` (`Native` vs `Container`). It does **not** spawn clangd/ty/tsserver. Those stay inside `progressive-lsp` packs.

## Crate split

```text
poc-ide/                     workspace member; not a musl artifact
  src/lib.rs                 domain + ports
  src/main.rs               composition root: eframe, rfd (`RfdDialog`), wire ports
  src/ports.rs              DialogPort, ClipboardPort, FsPort, StdFs, WatchPort, ClockPort, LspTransport, ControlTransport
  src/layout.rs             LayoutState (left panel width)
  src/tree.rs               WorkspaceRoot, FileTree, TreeNode Composite, CompactChain, ExpandChainCommand, CompactChainListing, TreeExpansion, PendingDialog (`OpenFolderInContainer`)
  src/open_mode.rs          HostOs, OpenMode, T3HostOffer, LaunchFlags
  src/runtime.rs            RuntimePort, FakeRuntime, DockerRuntime, DockerRunPlan, LaunchJournal, StatusModal
  src/runtime_io.rs         RuntimeIoRequest / RuntimeIoEvent mailbox; launch worker
  src/tabs.rs               TabStrip, TabId
  src/buffer.rs             OpenBuffer, BufferMap, Selection, DirtyFlag
  src/edit.rs               EditCommand
  src/discover.rs           DiscoverCommand, DiscoverKind
  src/highlight.rs          Highlighter Adapter + HighlightCache (path + rope generation)
  src/tree_io.rs            TreeIoRequest / TreeIoEvent mailbox; ExpandChainCommand listing
  src/conflict.rs           ConflictModal, ConflictChoice
  src/language.rs            LanguageCatalog, ServeMode, ControlSocketPath, DiscoverOffer, WireTier
  src/tier.rs               TierStrip, TierCell, PackageTierMap, DiscoverMenu
  src/lsp.rs                LspClient Facade, LspLocation, SpawnSpec, ServeSpawn, StdioLsp
  src/lsp_io.rs             LspIoAttach Strategy, LspIoRequest Command, LspIoEvent, ProgressEvent, DiscoverFlight, LspIoMailbox
  src/control.rs             ControlClient Adapter, UnixControl, ControlPushInbox Observer
  src/console.rs             ProtocolConsole Facade, TranscriptEntry (lib + unit tests; not wired in the bin)
  src/watch.rs              DiskWatch Observer, NotifyWatch
  src/log.rs                RunLog Repository (per-run sqlite debug sink; separate schema from serve WAL)
  src/child_stderr.rs       ChildStderrDrain Observer (serve stderr → RunLog)
  src/proof.rs              ProofStatus DTO the footer renders
```

**Dependency rule:** the lib does not import `egui` / `eframe` / `egui_dock` / `rfd`. Those belong in `main.rs`. Tests never open a window.

Allowed lib deps: `ropey`, `syntect`, `walkdir`, `lsp-types`, `serde_json`, `thiserror`, `progressive-lsp-control`, `rusqlite` (`bundled`; share **0.40.2** with `progressive-lsp-log`; poc-ide `RunLog` schema stays separate — never merge with the server WAL), `notify` only behind `WatchPort` (prod adapter in lib or thin `notify` adapter; tests inject `FakeWatch`).

## Data flow

1. File → Open Folder / Open Folder in Container / Open File records `PendingDialog`; apply runs the Port after the menu closes so `rfd` is not invoked mid-layout. `DialogPort.open_folder` / `open_file` → `WorkspaceRoot` (canonical absolute path). File → parent directory is the root; that file is also opened as a tab. **Open Folder…** on non-Linux is native T1/T2 (`T3HostOffer::NeedsContainer`). **Open Folder in Container…** (non-Linux only) is one Linux `progressive-lsp serve` for T1/T2/T3: the UI submits `RuntimeIoRequest::launch`, opens `StatusModal` (`Container`), and paints `LaunchJournal` as probe → platform → image → mount → start → T3 preflight. Linux has no container item; native open is the full host.
2. `FsPort.read_tree` → `FileTree` shallow load of the workspace root's immediate children (skip `.git/`, `target/`, `node_modules/` — display filter, not server ignore). Child directories start unloaded. Listing order is non-dot dirs, non-dot files, then dot dirs / dot files, lexicographic within each group. `TreeExpansion` starts empty (collapsed at every level). Expanding a row submits `TreeIoRequest` to a `poc-ide-tree` worker (`ExpandChainCommand` + `FsPort.read_dir`); the collapsing header may look open with children `loading…` until `TreeIoEvent` fills the inbox. `fn ui` never calls `read_dir` on expand. `FileTree::apply_listing` grafts the listing. `CompactChain` is a view of already-loaded single-child directory chains (`a/b/c`); an unloaded dir cannot claim "exactly one child," so root children are not compact-chained until the user expands enough. The compact row's path is the innermost directory; expanding it loads that dir's children.
3. Click a file → `BufferMap.open` (read bytes, `LanguageCatalog.for_path`, `didOpen` if LSP is up).
4. Keystrokes → `EditCommand` on `OpenBuffer` → dirty → generation bump → `didChange` incremental. The layouter calls `Highlighter::highlight` with path + generation; unchanged text does not re-tokenize.
5. Save → `FsPort.write` → clear dirty → `didSave`.
6. `WatchPort` events for an open path → if the buffer is open, enqueue `ConflictModal` (always; even if clean). Choice `LoadDisk` replaces rope and clears dirty; `KeepMemory` keeps rope and records `ignored_mtime`. Folder open subscribes at `WatchDepth::Immediate` on the workspace root; nested directories are watched when expanded (and a file's parent when opened). Recursive OS watch is not used at bind time.
7. After the tree is bound, one `poc-ide-lsp` thread owns child stdin/stdout and the stderr drain. The UI submits `LspIoRequest` (initialize, didOpen/didChange/didSave/didClose, discover, shutdown) and polls `LspIoEvent`. **The UI thread never calls `LspTransport::request`.** `$/progress` and `window/logMessage` are kept as `ProgressEvent` / `LogMessageEvent`. Discover items (F12, Navigate, editor and file-tree context menus) use `DiscoverMenu` from `LanguageCatalog` × current wire tier × ingest: LSP not ready → `connecting language server`; T1 in progress → `building T1 index`; discover in flight → `waiting for server`; T1 done enables Definition / References; Implementation is enabled only if that language answers it at this tier, else `needs T2` / `needs T3` / `not supported`; T3 stub refuse disables typed-only actions with `T3 skipped (stub pack)`. Tree, tabs, and typing stay live (`didChange` is queued). `DiscoverCommand::apply_locations` jumps and writes `ProofStatus` last-discover when the inbox yields. Control IO is a second thread (`poc-ide-control`) that requests IndexStatus / TierStatus after connect and polls Envelope pushes (`TierReady`, `WatchBatch`) into `ControlPushInbox`; `fn ui` never calls `index_status()` / `tier_status()`. A T1/T2/T3 status strip paints `TierStrip` for the focused file’s package (workspace aggregate fallback). Navigate records `PendingDiscover` and submits after the menu closes (same `DiscoverOffer` ground truth as the context menu). The editor view copies caret char offsets onto `OpenBuffer.selection` via `CursorOffsets` so discover uses the visible caret, not a stale 0,0. Right-click uses the focused tab + cursor (same as F12), not the tree path.
8. Debug events go to `RunLog` (sqlite). `ProtocolConsole` remains a lib Facade for Envelope/LSP transcript tests; the bin has no hand-typed inspector.

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
| `RuntimePort` | `DockerRuntime` (`docker` CLI; no daemon in tests) | `FakeRuntime` |
| `RunLog` (Repository) | rusqlite file under run-log dir | `:memory:` / tempfile path |

Prefer a **local `ClockPort`** in poc-ide (same invariant: tests never `thread::sleep`) rather than depending on `progressive-lsp-core`. Do not take a dependency on core just for the clock.

`RunLog` is a per-process sqlite Repository. Each `cargo xtask poc` (supported launch) writes a new file under `$HOME/.progressivelsp/poc-ide-runs/poc-ide-{unix_ms}-{pid}.sqlite` (override with `POC_IDE_LOG_DIR`). Tests inject `:memory:` or a tempfile path. Rows are `timestamp_ms`, `category`, `event`, optional JSON payload (method + error, never file bodies / clipboard / secrets). Discover rows also store `path`, `uri`, `line`, `character`, `location_count`. Child stderr lines are `category=lsp`, `event=child_stderr`. `run_start` records the resolved binary, argv, log level, RunLog path, and serve WAL path (or “not open yet”). A failed write is `IdeError::Log` and is ignored at the composition root — the editor does not panic. Do **not** merge this schema with the server WAL.

## LSP client

Content-Length JSON-RPC, same shape as the integration harness, **copied as a new Adapter** — do not add a crate dep on `integration/harness`. Methods used in v1 of the POC:

- `initialize` / `initialized` / `shutdown` / `exit`
- `textDocument/didOpen` / `didChange` / `didSave` / `didClose`
- `textDocument/definition` / `implementation` / `references`
- Read `capabilities.experimental.progressiveLsp` (socket may be null in stock mode)

Unknown server methods on `ProtocolConsole` (lib/tests): send anyway; record the JSON-RPC error. Jump targets are `LspLocation` (uri + range). `SpawnSpec` resolves the `progressive-lsp` binary (env, then `target/{debug,release}/progressive-lsp`, then `PATH`); missing binary is a domain error, not a panic. poc-proof-log default spawn is `ServeMode::ControlSocket` with an owned socket (CLI path, else `$PREFIX/run/poc-ide.sock`, else temp). The child always gets `PROGRESSIVE_LSP_LOG_LEVEL=debug` and `stderr(Stdio::piped())` into `ChildStderrDrain` → `RunLog`. stdout stays JSON-RPC. Do not inherit stderr into the GUI tty.

## Control client

Default spawn is `ServeMode::ControlSocket`. Spawn:

```text
progressive-lsp serve --control-socket PATH [--prefix DIR]
```

After `initialize`, connect to `experimental.progressiveLsp.socket` (must match). Speak `Envelope` (`method`, `request_id`, `body`) as in [../user/progressive-v1-api.md](../user/progressive-v1-api.md). `ProtocolConsole` (lib) must be able to invoke every unary RPC in the RPC table and record `WatchBatch` / `TierReady` pushes (`request_id == 0`). The bin does not hand-type those RPCs; `RunLog` is the debug sink.

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
- Editor: `egui::ScrollArea::both` around `egui::TextEdit::multiline` + syntect layouter from `Highlighter` tokens (cached by path + rope generation; layouter rebuilds only on edit; lines do not wrap so the area can scroll horizontally). Rope is source of truth; the widget is a view. After `TextEdit::show`, caret char offsets are copied onto `OpenBuffer.selection` via `CursorOffsets`. `response.context_menu` on the editor (and file tree rows) offers Find Definition / Implementation / References from `DiscoverMenu` (honest labels; not a static three-item enabled list). Clicks and keyboard F12 queue `LspIoRequest` only when the item is enabled (`LspSessionState::Ready` and the rest of `DiscoverMenu`); jump runs when the inbox yields. Navigate / F12 stay disabled with `connecting language server` until Ready. Save / disk conflict modal stays; it does not block the rest of the shell.
- Status strip: three **buttons** T1 / T2 / T3 (`processing` / `done` / `not supported` / `skipped` / `n/a`). Click opens `StatusModal` with that tier’s journal. Folder open starts workspace ingest in `initialize`; the strip paints T1/T2 from that ingest with no file focused. Opening a file is not required to start T2. A focused language with no T2 (Rust/CSS) stays T2 `n/a`. T3 stays `n/a` until a language is known. Native open on non-Linux skips T3 (`open folder in container`). Java T3 is `not supported` until bytecode T3 lands. Stub refuse is `skipped`, not `done`.
- File menu: **Open Folder…** is native serve (Linux: T1/T2/T3; elsewhere T1/T2). Non-Linux also offers **Open Folder in Container…**, which launches one Linux serve host and opens a **Container launch** modal with live preflight/launch steps. Tests use `FakeRuntime` (no Docker daemon). Container attach is `DockerRunPlan` → `StdioLsp::from_command` (`docker run -i --rm`, bind-mount identity, `StockStdio`). Native attach is unchanged (`ControlSocket`). Never two serves. Mux is host6.
- Modal: `egui::Modal` / `Window` for `ConflictModal` and `StatusModal`.
- No bottom protocol console. Debug is `RunLog` sqlite, not a hand-typed inspector. RunLog stays a separate schema from the serve WAL.
- Footer (`ProofStatus`): binary basename, log level, RunLog path, serve WAL path (or “WAL not open yet”), last discover (`definition L23:88 → 0 locations`) from RunLog discover rows.

## Failure modes

- Missing `progressive-lsp` binary: editor still edits/highlights; discovery commands return a domain error; `RunLog` / status records spawn failure. Not a panic.
- Control socket refused: stock LSP remains; `RunLog` records the control connect error.
- T3 stub / method empty: show empty location list; do not fake a hit.
