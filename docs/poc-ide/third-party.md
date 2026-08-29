# POC IDE third-party pins

Lean on OSS. Do not write an editor, highlighter, or dialog toolkit. If a crate will not pin cleanly, keep the **Port** and swap the Adapter.

## Locked set (implement from this table)

| Concern | Crate | Role | Fallback |
|---|---|---|---|
| Window + immediate mode | `eframe` **0.36.x** + `egui` **0.36.x** | Window, `SidePanel` resize, modal | none (POC is native egui) |
| Extra widgets / syntect layouter | `egui_extras` **0.36.x** (`syntect` feature if required) | Editor view highlighting | call `syntect` from `Highlighter` and paint runs |
| Tabs | `egui_dock` **0.21.x** (egui 0.36) | Tab bar | custom `TabStrip` view in `main.rs` only |
| Native open file/folder | `rfd` | `DialogPort` prod | `FakeDialog` in tests; CLI flags `--folder` / `--file` also acceptable as a second Adapter |
| Rope buffer | `ropey` | `OpenBuffer` storage | none |
| Syntax tokens | `syntect` | `Highlighter` Adapter | none (do not use Tree-sitter in the IDE; tokens from syntect, intelligence from LSP) |
| Walk tree | `walkdir` | `FsPort.read_tree` | `std::fs` recursion behind the same Port |
| Disk events | `notify` | `WatchPort` prod Adapter | `FakeWatch` |
| Clipboard | `arboard` **or** egui ctx clipboard | `ClipboardPort` prod in **main** | `FakeClipboard` |
| LSP DTOs | `lsp-types` | params/results | `serde_json::Value` for the console inspector |
| Control codec | `progressive-lsp-control` (in-repo) | Envelope + proto | none |
| Errors | `thiserror` | Domain Result | — |
| JSON-RPC | `serde` / `serde_json` | LSP frames | none |

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
