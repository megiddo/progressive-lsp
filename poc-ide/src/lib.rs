//! POC IDE domain: Ports, file-tree Composite, tabs, layout, buffers, syntect,
//! disk-conflict Observer, language catalog, stock LSP client, control Adapter,
//! and protocol console. The lib does not import `egui`, `eframe`, `egui_dock`,
//! or `rfd`. Those stay in the composition-root bin (`main.rs` / `ui.rs`).

pub mod buffer;
pub mod conflict;
pub mod console;
pub mod control;
pub mod edit;
pub mod error;
pub mod highlight;
pub mod language;
pub mod layout;
pub mod lsp;
pub mod ports;
pub mod tabs;
pub mod tree;
pub mod watch;

pub use buffer::{BufferMap, DirtyFlag, OpenBuffer, Selection};
pub use conflict::{ConflictChoice, ConflictModal};
pub use console::{ProtocolConsole, TranscriptEntry, TranscriptKind, STOCK_LSP_METHODS};
pub use control::{
    advertised_control_socket, ControlClient, ControlPush, UnixControl, CONTROL_UNARY_METHODS,
};
pub use edit::EditCommand;
pub use error::IdeError;
pub use highlight::{HighlightSpan, Highlighter};
pub use language::{LanguageCatalog, ServeMode};
pub use layout::LayoutState;
pub use lsp::{
    file_uri, path_from_file_uri, position_at, LspClient, LspLocation, ProgressiveLspCap,
    SpawnSpec, StdioLsp,
};
pub use ports::{
    ClipboardPort, ClockPort, ControlTransport, DialogPort, DiskEvent, DiskEventKind,
    FakeClipboard, FakeClock, FakeControl, FakeDialog, FakeLsp, FakeWatch, FsPort, LspCall,
    LspTransport, MemFs, StdFs, SystemClock, WatchPort,
};
pub use tabs::{TabId, TabStrip};
pub use tree::{FileTree, TreeNode, WorkspaceRoot};
pub use watch::{DiskWatch, NotifyWatch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_resolve() {
        let _ = FakeDialog::new();
        let _ = FakeClipboard::new();
        let _ = MemFs::new();
        let _ = StdFs;
        let _ = TabStrip::new();
        let _ = LayoutState::new();
        let _ = BufferMap::new();
        let _ = DirtyFlag::clean();
        let _ = Selection::collapsed(0);
        let _ = EditCommand::delete();
        let _ = Highlighter::new();
        let _ = HighlightSpan::new(0, 1, 0, 0, 0);
        let _ = FakeWatch::new();
        let _ = FakeClock::at_unix_ms(1);
        let _ = SystemClock;
        let _ = DiskWatch::new();
        let _ = NotifyWatch::new();
        let _ = ConflictModal::new("/ws/a.rs", 1);
        let _ = ConflictChoice::LoadDisk;
        let _ = ConflictChoice::KeepMemory;
        let _ = DiskEvent::modify("/ws/a.rs", 1);
        let _ = DiskEventKind::Modify;
        let _ = LanguageCatalog::new();
        let _ = ServeMode::StockStdio;
        let _ = ServeMode::ControlSocket;
        let _ = FakeLsp::new();
        let _ = LspCall::request("initialize", serde_json::json!({}));
        let _ = LspLocation::new("file:///ws/a.rs", 0, 0, 0, 0);
        let _ = SpawnSpec::from_path("/opt/progressive-lsp");
        let _ = position_at("fn x", 0);
        let _ = FakeControl::new();
        let _ = FakeControl::missing_socket();
        let _ = ControlClient::new(FakeControl::new());
        let _ = ProtocolConsole::new();
        let _ = TranscriptEntry::new(TranscriptKind::ControlPush, "WatchBatch", 0, "");
        let _ = TranscriptKind::ControlPush;
        let _ = STOCK_LSP_METHODS;
        let _ = CONTROL_UNARY_METHODS;
        assert!(FileTree::skips_display_name(".git"));
        assert!(IdeError::NotAbsolute(std::path::PathBuf::from("rel"))
            .to_string()
            .contains("absolute"));
        assert!(IdeError::watch("x").is_watch());
        assert!(IdeError::MissingBinary.is_missing_binary());
        assert!(IdeError::control("x").is_control());
        assert!(IdeError::control_socket_missing().is_control_socket_missing());
        assert!(IdeError::pending_mux().is_pending_mux());
        assert!(LanguageCatalog::new().skips_did_open("/ws/a.txt"));
        assert!(!ServeMode::StockStdio.is_control_socket());
        assert!(ServeMode::ControlSocket.is_control_socket());
        assert!(ProtocolConsole::new().is_empty());
        assert!(!STOCK_LSP_METHODS.is_empty());
        assert_eq!(CONTROL_UNARY_METHODS.len(), 9);
    }
}
