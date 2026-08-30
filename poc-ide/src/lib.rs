//! POC IDE domain: Ports, file-tree Composite, tabs, layout, buffers, syntect,
//! and disk-conflict Observer. The lib does not import `egui`, `eframe`,
//! `egui_dock`, or `rfd`. Those stay in the composition-root bin (`main.rs` /
//! `ui.rs`).

pub mod buffer;
pub mod conflict;
pub mod edit;
pub mod error;
pub mod highlight;
pub mod layout;
pub mod ports;
pub mod tabs;
pub mod tree;
pub mod watch;

pub use buffer::{BufferMap, DirtyFlag, OpenBuffer, Selection};
pub use conflict::{ConflictChoice, ConflictModal};
pub use edit::EditCommand;
pub use error::IdeError;
pub use highlight::{HighlightSpan, Highlighter};
pub use layout::LayoutState;
pub use ports::{
    ClipboardPort, ClockPort, DialogPort, DiskEvent, DiskEventKind, FakeClipboard, FakeClock,
    FakeDialog, FakeWatch, FsPort, MemFs, StdFs, SystemClock, WatchPort,
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
        assert!(FileTree::skips_display_name(".git"));
        assert!(IdeError::NotAbsolute(std::path::PathBuf::from("rel"))
            .to_string()
            .contains("absolute"));
        assert!(IdeError::watch("x").is_watch());
    }
}
