//! `ConflictModal` / `ConflictChoice` Command: load disk or keep memory.

use std::path::{Path, PathBuf};

use crate::buffer::BufferMap;
use crate::error::IdeError;
use crate::ports::FsPort;

/// Choice on a pending [`ConflictModal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    LoadDisk,
    KeepMemory,
}

/// One pending prompt for an open path that changed on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictModal {
    path: PathBuf,
    mtime: u64,
}

impl ConflictModal {
    pub fn new(path: impl Into<PathBuf>, mtime: u64) -> Self {
        Self {
            path: path.into(),
            mtime,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mtime(&self) -> u64 {
        self.mtime
    }

    /// `LoadDisk` replaces the rope from `FsPort` and clears dirty.
    /// `KeepMemory` leaves the rope unchanged.
    pub fn apply(
        &self,
        choice: ConflictChoice,
        buffers: &mut BufferMap,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<(), IdeError> {
        match choice {
            ConflictChoice::LoadDisk => {
                let buf = buffers
                    .get_mut(&self.path)
                    .ok_or_else(|| IdeError::NotFound(self.path.clone()))?;
                buf.reload_from(fs)
            }
            ConflictChoice::KeepMemory => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::OpenBuffer;
    use crate::edit::EditCommand;
    use crate::ports::{FakeClipboard, MemFs};
    use std::path::Path;

    fn fixture() -> (MemFs, BufferMap) {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"disk-v1\n").unwrap();
        let mut buffers = BufferMap::new();
        buffers.open("/ws/a.rs", &fs).unwrap();
        (fs, buffers)
    }

    #[test]
    fn conflict_modal_value_object_path_and_mtime() {
        let modal = ConflictModal::new("/ws/a.rs", 9);
        assert_eq!(modal.path(), Path::new("/ws/a.rs"));
        assert_eq!(modal.mtime(), 9);
        assert_eq!(modal, ConflictModal::new("/ws/a.rs", 9));
        assert_ne!(modal, ConflictModal::new("/ws/a.rs", 10));
        assert_eq!(ConflictChoice::LoadDisk, ConflictChoice::LoadDisk);
        assert_ne!(ConflictChoice::LoadDisk, ConflictChoice::KeepMemory);
    }

    #[test]
    fn conflict_choice_load_disk_replaces_rope_and_clears_dirty() {
        let (mut fs, mut buffers) = fixture();
        let mut clip = FakeClipboard::new();
        EditCommand::insert("edit")
            .apply(buffers.get_mut("/ws/a.rs").unwrap(), &mut clip)
            .unwrap();
        assert!(buffers.get("/ws/a.rs").unwrap().is_dirty());
        fs.write(Path::new("/ws/a.rs"), b"disk-v2\n").unwrap();

        let modal = ConflictModal::new("/ws/a.rs", 3);
        modal
            .apply(ConflictChoice::LoadDisk, &mut buffers, &fs)
            .unwrap();
        let buf = buffers.get("/ws/a.rs").unwrap();
        assert_eq!(buf.text(), "disk-v2\n");
        assert!(!buf.is_dirty());
        assert_eq!(buf.dirty_flag(), crate::buffer::DirtyFlag::clean());
    }

    #[test]
    fn conflict_choice_keep_memory_keeps_rope() {
        let (mut fs, mut buffers) = fixture();
        let mut clip = FakeClipboard::new();
        EditCommand::insert("keep-me")
            .apply(buffers.get_mut("/ws/a.rs").unwrap(), &mut clip)
            .unwrap();
        fs.write(Path::new("/ws/a.rs"), b"ignored-disk\n").unwrap();

        let modal = ConflictModal::new("/ws/a.rs", 4);
        modal
            .apply(ConflictChoice::KeepMemory, &mut buffers, &fs)
            .unwrap();
        let buf = buffers.get("/ws/a.rs").unwrap();
        assert_eq!(buf.text(), "keep-medisk-v1\n");
        assert!(buf.is_dirty());
        assert_eq!(
            String::from_utf8(fs.read(Path::new("/ws/a.rs")).unwrap()).unwrap(),
            "ignored-disk\n"
        );
    }

    #[test]
    fn conflict_choice_load_disk_missing_buffer_is_domain_result() {
        let fs = MemFs::new();
        let mut buffers = BufferMap::new();
        let modal = ConflictModal::new("/ws/missing.rs", 1);
        let err = modal
            .apply(ConflictChoice::LoadDisk, &mut buffers, &fs)
            .unwrap_err();
        assert!(err.is_not_found());
        assert!(modal
            .apply(ConflictChoice::KeepMemory, &mut buffers, &fs)
            .is_ok());
        assert!(OpenBuffer::load("/ws/missing.rs", &fs)
            .unwrap_err()
            .is_not_found());
    }
}
