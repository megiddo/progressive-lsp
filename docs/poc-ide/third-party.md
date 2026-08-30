# POC IDE third-party pins

Lean on OSS. Do not write an editor, highlighter, or dialog toolkit. If a crate will not pin cleanly, keep the **Port** and swap the Adapter.

## Locked set (implement from this table)

| Concern | Crate | Role | Fallback |
|---|---|---|---|
| Window + immediate mode | `eframe` **0.36.1** + `egui` **0.36.1** | Window, `Panel::left` resize, modal | none (POC is native egui) |
| Extra widgets / syntect layouter | `egui_extras` **0.36.1** (`syntect` feature if required) | Editor view highlighting | call `syntect` from `Highlighter` and paint runs |
| Tabs | `egui_dock` **0.21.x** (egui 0.36) | Tab bar | **IDE-1 uses the fallback:** custom `TabStrip` in `ui.rs`. Published 0.21.1 declares rust-version **1.95**; this workspace is rustc **1.87**. |
| Native open file/folder | `rfd` **0.15.4** | `DialogPort` prod | `FakeDialog` in tests; CLI flags `--folder` / `--file` also acceptable as a second Adapter |
| Rope buffer | `ropey` | `OpenBuffer` storage | none |
| Syntax tokens | `syntect` | `Highlighter` Adapter | none (do not use Tree-sitter in the IDE; tokens from syntect, intelligence from LSP) |
| Walk tree | `walkdir` | `FsPort.read_tree` | **IDE-1 used the fallback:** `std::fs` one-level listing behind `FsPort` / `FileTree::load`. `poc-tree-lazy` keeps that Adapter; load is shallow, expand lists one more level. |
| Disk events | `notify` | `WatchPort` prod Adapter | `FakeWatch` |
| Clipboard | `arboard` **or** egui ctx clipboard | `ClipboardPort` prod in **main** | `FakeClipboard` |
| LSP DTOs | `lsp-types` | params/results | `serde_json::Value` for `ProtocolConsole` lib tests |
| Control codec | `progressive-lsp-control` (in-repo) | Envelope + proto | none |
| Errors | `thiserror` | Domain Result | — |
| JSON-RPC | `serde` / `serde_json` | LSP frames | none |
| Per-run debug log | `rusqlite` **0.40.2** (`bundled`) | `RunLog` Repository in poc-ide (separate schema from the server WAL) | `:memory:` / tempfile in tests |

**Do not add:** `tokio` unless stdio/socket truly cannot stay on threads + channels; prefer `std::thread` + channels so tests stay FakeClock and sleep-free. If async is required, isolate it in the transport Adapter and do not `sleep` in tests.

## Explicitly rejected

| Candidate | Why not |
|---|---|
| Electron / Tauri + Monaco / CodeMirror | Node or web stack; fights in-tree “no Node as runtime” and coverage |
| VS Code extension as the POC | Cannot own `progressive.v1` Envelope in-process; not a folder we test at 95% |
| lapce / Zed / Helix as a fork | Too much product; we need a thin consumer |
| Tree-sitter highlighting in the IDE | Server already does semantic tokens; POC highlighting is syntect so the IDE does not vendor grammars |
| `integration/harness` as a dependency | Not a workspace library; copy the Content-Length idea as `LspTransport` |

## Version rule

Pin the **egui trio** (`egui`, `eframe`, `egui_extras`) to the same minor. `egui_dock` must declare that egui. If `egui_dock` lags on a future bump, drop docking and keep `TabStrip` — do not upgrade egui out of sync to chase docking.

Record the chosen exact versions in `poc-ide/Cargo.toml` on IDE-1. This file stays the policy.

**IDE-1 pins** (also in `poc-ide/Cargo.toml`): `eframe` / `egui` / `egui_extras` **=0.36.1**, `rfd` **=0.15.4**, `thiserror` workspace **2.0**. No `egui_dock`, `walkdir`, `ropey`, `syntect`, or `notify` on this crate yet.

**IDE-2 pins:** `ropey` **1.6.1**, `syntect` **5.3.0**, `arboard` **3.6.1** (bin `ArboardClipboard` only). Still no `egui_dock`, `walkdir`, or `notify`.

**IDE-3 pins:** `notify` **8.2.0** (`NotifyWatch` maps `notify` events in the lib; live `RecommendedWatcher` is wired in the bin). Tests inject `FakeWatch` only. Still no `egui_dock` or `walkdir`.

**IDE-4 pins:** `lsp-types` **0.97.0**, `serde` / `serde_json` workspace **1.0**. Content-Length framing is copied into `StdioLsp` — no `integration/harness` or `progressive-lsp-plugin` / `-resolve` / `-index` dep. Still no `egui_dock` or `walkdir`. `ControlSocket` is an unused `ServeMode` variant until IDE-5.

**IDE-5 pins:** `progressive-lsp-control` (workspace). Consumer MAY depend. Lib still no egui. `--mux` is `pending_mux` — do not silently retest the socket as mux. Still no `egui_dock` or `walkdir`.

**poc-log pin** (post-IDE-5 slice on `main`, not IDE-6): `rusqlite` **=0.40.2** with `bundled`. Share this pin with `progressive-lsp-log` (LOG-2+). Two schemas: poc-ide `RunLog` (`events` category/event/payload) vs server WAL `LogRecord`. Do not merge. The amalgamation is an **our-artifact** (static C in the ELF), not a host `.so`. **Supersedes** “rusqlite is poc-ide only — never musl server crates.”

**LOG-4 lock vs impl:** `rusqlite` is allowed in `progressive-lsp-log`. `serve` / `install` now write the **server** WAL (`LogRecord` columns under `$PREFIX/log/serve-<unix_ms>-<pid>.sqlite`). That file is still a **separate** schema from poc-ide `RunLog`. Do not merge. Do not write poc-ide rows into the server WAL.
