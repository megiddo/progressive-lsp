//! `DialogPort` / `FsPort` plus production adapters and test doubles.
//! Tests never call `rfd` or touch host disk except through `StdFs` tests.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::error::IdeError;
use crate::tree::{FileTree, WorkspaceRoot};

/// Native dialogs go through this Port. Production `RfdDialog` lives in the bin.
pub trait DialogPort {
    fn open_folder(&mut self) -> Option<PathBuf>;
    fn open_file(&mut self) -> Option<PathBuf>;
}

/// Test double: returns queued folder/file paths. Empty queue is cancel (`None`).
#[derive(Clone, Debug, Default)]
pub struct FakeDialog {
    folders: VecDeque<Option<PathBuf>>,
    files: VecDeque<Option<PathBuf>>,
}

impl FakeDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_folder(&mut self, path: impl Into<PathBuf>) {
        self.folders.push_back(Some(path.into()));
    }

    pub fn queue_file(&mut self, path: impl Into<PathBuf>) {
        self.files.push_back(Some(path.into()));
    }

    pub fn queue_folder_cancel(&mut self) {
        self.folders.push_back(None);
    }

    pub fn queue_file_cancel(&mut self) {
        self.files.push_back(None);
    }

    pub fn pending_folders(&self) -> usize {
        self.folders.len()
    }

    pub fn pending_files(&self) -> usize {
        self.files.len()
    }
}

impl DialogPort for FakeDialog {
    fn open_folder(&mut self) -> Option<PathBuf> {
        self.folders.pop_front().flatten()
    }

    fn open_file(&mut self) -> Option<PathBuf> {
        self.files.pop_front().flatten()
    }
}

/// Immediate child of a directory. Crate-private listing DTO for `FsPort`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Tree / canonicalize go through this Port. Tests use [`MemFs`].
pub trait FsPort {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, IdeError>;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, IdeError>;
    fn read_tree(&self, root: &WorkspaceRoot) -> Result<FileTree, IdeError> {
        FileTree::load(root, self)
    }
}

/// Production adapter over `std::fs`.
#[derive(Clone, Debug, Default)]
pub struct StdFs;

impl FsPort for StdFs {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, IdeError> {
        if !path.is_absolute() {
            return Err(IdeError::NotAbsolute(path.to_path_buf()));
        }
        match std::fs::canonicalize(path) {
            Ok(p) => Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(IdeError::NotFound(path.to_path_buf()))
            }
            Err(e) => Err(IdeError::Io(e)),
        }
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, IdeError> {
        if !path.is_dir() {
            return Err(IdeError::NotADirectory(path.to_path_buf()));
        }
        let mut out = Vec::new();
        for ent in std::fs::read_dir(path)? {
            let ent = ent?;
            let file_path = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            let is_dir = ent.file_type()?.is_dir();
            out.push(DirEntry {
                name,
                path: file_path,
                is_dir,
            });
        }
        Ok(out)
    }
}

/// In-memory `FsPort`. No host disk.
#[derive(Clone, Debug, Default)]
pub struct MemFs {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_dir(&mut self, path: impl AsRef<Path>) -> Result<(), IdeError> {
        let path = require_absolute(path.as_ref())?;
        for ancestor in path.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            self.dirs.insert(ancestor.to_path_buf());
        }
        Ok(())
    }

    pub fn add_file(
        &mut self,
        path: impl AsRef<Path>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<(), IdeError> {
        let path = require_absolute(path.as_ref())?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.add_dir(parent)?;
            }
        }
        self.files.insert(path, bytes.as_ref().to_vec());
        Ok(())
    }
}

impl FsPort for MemFs {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, IdeError> {
        let path = require_absolute(path)?;
        if self.files.contains_key(&path) || self.dirs.contains(&path) {
            Ok(path)
        } else {
            Err(IdeError::NotFound(path))
        }
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains(path)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, IdeError> {
        if !self.dirs.contains(path) {
            if self.files.contains_key(path) {
                return Err(IdeError::NotADirectory(path.to_path_buf()));
            }
            return Err(IdeError::NotFound(path.to_path_buf()));
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for file in self.files.keys() {
            if file.parent() == Some(path) {
                if let Some(name) = file.file_name() {
                    let name = name.to_string_lossy().into_owned();
                    if seen.insert(file.clone()) {
                        out.push(DirEntry {
                            name,
                            path: file.clone(),
                            is_dir: false,
                        });
                    }
                }
            }
        }
        for dir in &self.dirs {
            if dir.parent() == Some(path) && seen.insert(dir.clone()) {
                if let Some(name) = dir.file_name() {
                    out.push(DirEntry {
                        name: name.to_string_lossy().into_owned(),
                        path: dir.clone(),
                        is_dir: true,
                    });
                }
            }
        }
        Ok(out)
    }
}

pub(crate) fn require_absolute(path: &Path) -> Result<PathBuf, IdeError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Err(IdeError::NotAbsolute(path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn fake_dialog_test_double_returns_queued_paths() {
        let mut dialog = FakeDialog::new();
        assert_eq!(dialog.pending_folders(), 0);
        assert_eq!(dialog.pending_files(), 0);
        assert!(dialog.open_folder().is_none());
        assert!(dialog.open_file().is_none());

        dialog.queue_folder("/ws");
        dialog.queue_folder_cancel();
        dialog.queue_folder("/other");
        assert_eq!(dialog.pending_folders(), 3);
        assert_eq!(dialog.open_folder().as_deref(), Some(Path::new("/ws")));
        assert!(dialog.open_folder().is_none());
        assert_eq!(dialog.open_folder().as_deref(), Some(Path::new("/other")));
        assert_eq!(dialog.pending_folders(), 0);

        dialog.queue_file("/ws/a.rs");
        dialog.queue_file_cancel();
        assert_eq!(dialog.pending_files(), 2);
        assert_eq!(dialog.open_file().as_deref(), Some(Path::new("/ws/a.rs")));
        assert!(dialog.open_file().is_none());
        assert_eq!(dialog.pending_files(), 0);
    }

    #[test]
    fn mem_fs_test_double_same_fs_port_no_host_disk() {
        let mut fs = MemFs::new();
        assert!(!fs.is_dir(Path::new("/ws")));
        fs.add_file("/ws/src/lib.rs", b"fn x() {}").unwrap();
        fs.add_dir("/ws/docs").unwrap();
        assert!(fs.is_dir(Path::new("/ws")));
        assert!(fs.is_dir(Path::new("/ws/src")));
        assert!(fs.is_dir(Path::new("/ws/docs")));
        assert!(!fs.is_dir(Path::new("/ws/src/lib.rs")));
        assert!(!fs.is_dir(Path::new("/missing")));
        assert_eq!(
            fs.canonicalize(Path::new("/ws/src/lib.rs")).unwrap(),
            PathBuf::from("/ws/src/lib.rs")
        );
        assert!(fs
            .canonicalize(Path::new("/missing"))
            .unwrap_err()
            .is_not_found());
        assert!(fs
            .canonicalize(Path::new("rel"))
            .unwrap_err()
            .is_not_absolute());
        assert!(fs.add_file("rel.rs", b"").unwrap_err().is_not_absolute());
        assert!(fs.add_dir("rel").unwrap_err().is_not_absolute());

        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = fs.read_tree(&root).unwrap();
        assert_eq!(tree.children().len(), 2);
        let names: Vec<_> = tree
            .children()
            .iter()
            .map(|n| n.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "src"));
        assert!(names.iter().any(|n| n == "docs"));
    }

    #[test]
    fn mem_fs_read_dir_errors_on_file_and_missing() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"").unwrap();
        assert!(fs
            .read_dir(Path::new("/ws/a.rs"))
            .unwrap_err()
            .is_not_a_directory());
        assert!(fs.read_dir(Path::new("/nope")).unwrap_err().is_not_found());
        let entries = fs.read_dir(Path::new("/ws")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.rs");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn std_fs_port_adapter_tree_and_canonicalize() {
        let tmp = tempfile::tempdir().unwrap();
        let root_path = tmp.path();
        fs::create_dir_all(root_path.join("src")).unwrap();
        fs::write(root_path.join("src/lib.rs"), "ok").unwrap();
        fs::create_dir_all(root_path.join(".git")).unwrap();
        fs::write(root_path.join(".git/HEAD"), "ref").unwrap();
        fs::create_dir_all(root_path.join("target/debug")).unwrap();
        fs::write(root_path.join("target/debug/x"), "x").unwrap();
        fs::create_dir_all(root_path.join("node_modules/pkg")).unwrap();
        fs::write(root_path.join("README.md"), "hi").unwrap();

        let fs_port = StdFs;
        let canon = fs_port.canonicalize(root_path).unwrap();
        assert!(canon.is_absolute());
        assert!(fs_port.is_dir(&canon));
        assert!(!fs_port.is_dir(&canon.join("README.md")));
        let root = WorkspaceRoot::from_canonical(&canon).unwrap();
        let tree = fs_port.read_tree(&root).unwrap();
        let names: Vec<_> = tree
            .children()
            .iter()
            .map(|n| n.name().to_string())
            .collect();
        assert!(names.contains(&"src".into()));
        assert!(names.contains(&"README.md".into()));
        assert!(!names.contains(&".git".into()));
        assert!(!names.contains(&"target".into()));
        assert!(!names.contains(&"node_modules".into()));

        assert!(StdFs
            .canonicalize(Path::new("rel"))
            .unwrap_err()
            .is_not_absolute());
        assert!(StdFs
            .canonicalize(&root_path.join("missing"))
            .unwrap_err()
            .is_not_found());
        assert!(StdFs
            .read_dir(&root_path.join("README.md"))
            .unwrap_err()
            .is_not_a_directory());
    }

    #[test]
    fn std_fs_read_dir_io_error_maps_to_domain_result() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("only");
        fs::write(&file, b"x").unwrap();
        let err = StdFs.read_dir(&file).unwrap_err();
        assert!(err.is_not_a_directory());
    }

    #[cfg(unix)]
    #[test]
    fn std_fs_canonicalize_permission_denied_is_io() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let locked = tmp.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let inner = locked.join("file");
        fs::write(&inner, b"x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let result = StdFs.canonicalize(&inner);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.unwrap_err();
        assert!(
            err.is_io(),
            "unreadable parent must be Io, not NotFound: {err}"
        );
    }

    #[test]
    fn dir_entry_dto_fields_round_trip() {
        let e = DirEntry {
            name: "lib.rs".into(),
            path: PathBuf::from("/ws/lib.rs"),
            is_dir: false,
        };
        assert_eq!(e.name, "lib.rs");
        assert!(!e.is_dir);
        assert_eq!(e.path, PathBuf::from("/ws/lib.rs"));
        let d = DirEntry {
            name: "src".into(),
            path: PathBuf::from("/ws/src"),
            is_dir: true,
        };
        assert!(d.is_dir);
    }

    #[test]
    fn require_absolute_rejects_relative() {
        assert!(require_absolute(Path::new("rel"))
            .unwrap_err()
            .is_not_absolute());
        assert_eq!(
            require_absolute(Path::new("/abs")).unwrap(),
            PathBuf::from("/abs")
        );
    }
}
