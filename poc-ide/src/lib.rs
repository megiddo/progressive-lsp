//! POC IDE domain: Ports, file-tree Composite, tabs, and layout.
//!
//! The lib does not import `egui`, `eframe`, `egui_dock`, or `rfd`. Those stay
//! in the composition-root bin (`main.rs` / `ui.rs`).

pub mod error;
pub mod layout;
pub mod ports;
pub mod tabs;
pub mod tree;

pub use error::IdeError;
pub use layout::LayoutState;
pub use ports::{DialogPort, FakeDialog, FsPort, MemFs, StdFs};
pub use tabs::{TabId, TabStrip};
pub use tree::{FileTree, TreeNode, WorkspaceRoot};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_resolve() {
        let _ = FakeDialog::new();
        let _ = MemFs::new();
        let _ = StdFs;
        let _ = TabStrip::new();
        let _ = LayoutState::new();
        assert!(FileTree::skips_display_name(".git"));
        assert!(IdeError::NotAbsolute(std::path::PathBuf::from("rel"))
            .to_string()
            .contains("absolute"));
    }
}
