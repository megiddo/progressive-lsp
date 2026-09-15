//! Find-references result list. Value object; `ui.rs` renders the modal.

use crate::buffer::BufferMap;
use crate::error::IdeError;
use crate::lsp::LspLocation;
use crate::ports::FsPort;
use crate::tabs::{TabId, TabStrip};

/// Pending reference locations from `textDocument/references`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencesModal {
    locations: Vec<LspLocation>,
}

impl ReferencesModal {
    pub fn closed() -> Self {
        Self::default()
    }

    pub fn open(locations: Vec<LspLocation>) -> Self {
        Self { locations }
    }

    pub fn is_open(&self) -> bool {
        !self.locations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.locations.len()
    }

    pub fn locations(&self) -> &[LspLocation] {
        &self.locations
    }

    pub fn close(&mut self) {
        self.locations.clear();
    }

    /// One-based line/character for the modal link label.
    pub fn link_label(&self, index: usize) -> String {
        let Some(loc) = self.locations.get(index) else {
            return String::new();
        };
        let path = loc
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| loc.uri().to_string());
        format!(
            "{}:{}:{}",
            path,
            loc.start_line().saturating_add(1),
            loc.start_character().saturating_add(1)
        )
    }

    /// Open/focus the file and move the cursor to the reference range.
    pub fn navigate_to(
        &self,
        index: usize,
        tabs: &mut TabStrip,
        buffers: &mut BufferMap,
        fs: &impl FsPort,
    ) -> Result<(), IdeError> {
        let loc = self
            .locations
            .get(index)
            .ok_or_else(|| IdeError::lsp("reference index out of range"))?;
        loc.open_or_focus(tabs, buffers, fs)?;
        let path = loc.file_path()?;
        tabs.focus(&TabId::from_path(&path));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::lsp::LspLocation;
    use crate::ports::MemFs;

    #[test]
    fn references_modal_open_close_and_labels() {
        let loc = LspLocation::new("file:///ws/src/Foo.java", 10, 4, 10, 8);
        let modal = ReferencesModal::open(vec![loc]);
        assert!(modal.is_open());
        assert_eq!(modal.len(), 1);
        assert!(modal.link_label(0).contains("Foo.java"));
        assert!(modal.link_label(0).contains(":11:5"));
        let mut modal = modal;
        modal.close();
        assert!(!modal.is_open());
    }

    #[test]
    fn references_modal_navigate_focuses_tab() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/src/Foo.java", b"class Foo {}\n").unwrap();
        let loc = LspLocation::new("file:///ws/src/Foo.java", 0, 6, 0, 9);
        let modal = ReferencesModal::open(vec![loc]);
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        modal
            .navigate_to(0, &mut tabs, &mut buffers, &fs)
            .unwrap();
        assert_eq!(tabs.focused().map(|t| t.as_path()), Some(Path::new("/ws/src/Foo.java")));
    }
}
