//! `WorkspaceRoot` value object and `FileTree` / `TreeNode` Composite.

use std::path::{Path, PathBuf};

use crate::error::IdeError;
use crate::ports::{require_absolute, DialogPort, DirEntry, FsPort};

/// Canonical absolute workspace path. Equality is path equality.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceRoot {
    path: PathBuf,
}

impl WorkspaceRoot {
    pub fn from_canonical(path: impl AsRef<Path>) -> Result<Self, IdeError> {
        let path = require_absolute(path.as_ref())?;
        Ok(Self { path })
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn from_folder_path(path: &Path, fs: &(impl FsPort + ?Sized)) -> Result<Self, IdeError> {
        let canon = fs.canonicalize(path)?;
        if !fs.is_dir(&canon) {
            return Err(IdeError::NotADirectory(canon));
        }
        Self::from_canonical(canon)
    }

    pub fn from_file_path(
        path: &Path,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<(Self, PathBuf), IdeError> {
        let file = fs.canonicalize(path)?;
        let parent = file
            .parent()
            .ok_or_else(|| IdeError::NoParent(file.clone()))?;
        let root = Self::from_folder_path(parent, fs)?;
        Ok((root, file))
    }

    pub fn open_folder(
        dialog: &mut impl DialogPort,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<Option<Self>, IdeError> {
        match dialog.open_folder() {
            None => Ok(None),
            Some(path) => Self::from_folder_path(&path, fs).map(Some),
        }
    }

    pub fn open_file(
        dialog: &mut impl DialogPort,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<Option<(Self, PathBuf)>, IdeError> {
        match dialog.open_file() {
            None => Ok(None),
            Some(path) => Self::from_file_path(&path, fs).map(Some),
        }
    }
}

/// Directory or file node. Directories contain children; files are leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeNode {
    File {
        name: String,
        path: PathBuf,
    },
    Directory {
        name: String,
        path: PathBuf,
        children: Vec<TreeNode>,
    },
}

impl TreeNode {
    pub fn name(&self) -> &str {
        match self {
            Self::File { name, .. } | Self::Directory { name, .. } => name,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::File { path, .. } | Self::Directory { path, .. } => path,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    pub fn children(&self) -> &[TreeNode] {
        match self {
            Self::Directory { children, .. } => children,
            Self::File { .. } => &[],
        }
    }

    fn from_entry(entry: DirEntry, fs: &(impl FsPort + ?Sized)) -> Result<Self, IdeError> {
        if entry.is_dir {
            let children = load_children(&entry.path, fs)?;
            Ok(Self::Directory {
                name: entry.name,
                path: entry.path,
                children,
            })
        } else {
            Ok(Self::File {
                name: entry.name,
                path: entry.path,
            })
        }
    }
}

/// Workspace listing. Skip `.git` / `target` / `node_modules` from display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTree {
    root: WorkspaceRoot,
    children: Vec<TreeNode>,
}

impl FileTree {
    pub fn skips_display_name(name: &str) -> bool {
        matches!(name, ".git" | "target" | "node_modules")
    }

    pub fn load(root: &WorkspaceRoot, fs: &(impl FsPort + ?Sized)) -> Result<Self, IdeError> {
        if !fs.is_dir(root.as_path()) {
            return Err(IdeError::NotADirectory(root.as_path().to_path_buf()));
        }
        let children = load_children(root.as_path(), fs)?;
        Ok(Self {
            root: root.clone(),
            children,
        })
    }

    pub fn root(&self) -> &WorkspaceRoot {
        &self.root
    }

    pub fn children(&self) -> &[TreeNode] {
        &self.children
    }

    pub fn find(&self, path: &Path) -> Option<&TreeNode> {
        find_node(&self.children, path)
    }
}

fn load_children(dir: &Path, fs: &(impl FsPort + ?Sized)) -> Result<Vec<TreeNode>, IdeError> {
    let mut entries = fs.read_dir(dir)?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut nodes = Vec::new();
    for entry in entries {
        if entry.is_dir && FileTree::skips_display_name(&entry.name) {
            continue;
        }
        nodes.push(TreeNode::from_entry(entry, fs)?);
    }
    Ok(nodes)
}

fn find_node<'a>(nodes: &'a [TreeNode], path: &Path) -> Option<&'a TreeNode> {
    for node in nodes {
        if node.path() == path {
            return Some(node);
        }
        if node.is_dir() {
            if let Some(hit) = find_node(node.children(), path) {
                return Some(hit);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::MemFs;

    fn sample_fs() -> MemFs {
        let mut fs = MemFs::new();
        fs.add_file("/ws/src/lib.rs", b"fn x() {}").unwrap();
        fs.add_file("/ws/src/main.rs", b"fn main() {}").unwrap();
        fs.add_file("/ws/README.md", b"hi").unwrap();
        fs.add_file("/ws/.git/HEAD", b"ref").unwrap();
        fs.add_file("/ws/target/debug/poc", b"x").unwrap();
        fs.add_file("/ws/node_modules/pkg/index.js", b"1").unwrap();
        fs.add_file("/ws/.github/workflows/ci.yml", b"on").unwrap();
        fs.add_dir("/ws/empty").unwrap();
        fs
    }

    #[test]
    fn workspace_root_value_object_equality_is_path_equality() {
        let a = WorkspaceRoot::from_canonical("/ws").unwrap();
        let b = WorkspaceRoot::from_canonical("/ws").unwrap();
        let c = WorkspaceRoot::from_canonical("/other").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_path(), Path::new("/ws"));
        assert!(WorkspaceRoot::from_canonical("rel")
            .unwrap_err()
            .is_not_absolute());
        assert!(WorkspaceRoot::from_canonical("")
            .unwrap_err()
            .is_not_absolute());
    }

    #[test]
    fn workspace_root_open_folder_via_dialog_port() {
        let fs = sample_fs();
        let mut dialog = crate::FakeDialog::new();
        dialog.queue_folder_cancel();
        assert!(WorkspaceRoot::open_folder(&mut dialog, &fs)
            .unwrap()
            .is_none());

        dialog.queue_folder("/ws");
        let root = WorkspaceRoot::open_folder(&mut dialog, &fs)
            .unwrap()
            .unwrap();
        assert_eq!(root.as_path(), Path::new("/ws"));

        dialog.queue_folder("/missing");
        assert!(WorkspaceRoot::open_folder(&mut dialog, &fs)
            .unwrap_err()
            .is_not_found());

        let mut fs_file = MemFs::new();
        fs_file.add_file("/ws/only.rs", b"").unwrap();
        dialog.queue_folder("/ws/only.rs");
        assert!(WorkspaceRoot::open_folder(&mut dialog, &fs_file)
            .unwrap_err()
            .is_not_a_directory());
    }

    #[test]
    fn workspace_root_open_file_sets_parent_root() {
        let fs = sample_fs();
        let mut dialog = crate::FakeDialog::new();
        dialog.queue_file_cancel();
        assert!(WorkspaceRoot::open_file(&mut dialog, &fs)
            .unwrap()
            .is_none());

        dialog.queue_file("/ws/src/lib.rs");
        let (root, file) = WorkspaceRoot::open_file(&mut dialog, &fs).unwrap().unwrap();
        assert_eq!(root.as_path(), Path::new("/ws/src"));
        assert_eq!(file, PathBuf::from("/ws/src/lib.rs"));

        dialog.queue_file("/missing.rs");
        assert!(WorkspaceRoot::open_file(&mut dialog, &fs)
            .unwrap_err()
            .is_not_found());
    }

    #[test]
    fn workspace_root_from_file_path_no_parent_is_error() {
        let mut fs = MemFs::new();
        fs.add_dir("/").unwrap();
        let err = WorkspaceRoot::from_file_path(Path::new("/"), &fs).unwrap_err();
        assert!(err.is_no_parent() || err.is_not_a_directory());
    }

    #[test]
    fn file_tree_composite_skips_git_target_node_modules() {
        assert!(FileTree::skips_display_name(".git"));
        assert!(FileTree::skips_display_name("target"));
        assert!(FileTree::skips_display_name("node_modules"));
        assert!(!FileTree::skips_display_name(".github"));
        assert!(!FileTree::skips_display_name("src"));
        assert!(!FileTree::skips_display_name("node_module"));
        assert!(!FileTree::skips_display_name("targets"));
        assert!(!FileTree::skips_display_name(".gitignore"));
        assert!(!FileTree::skips_display_name(""));

        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        assert_eq!(tree.root(), &root);
        let names: Vec<_> = tree
            .children()
            .iter()
            .map(|n| n.name().to_string())
            .collect();
        assert_eq!(names, vec![".github", "README.md", "empty", "src"]);
        assert!(tree.find(Path::new("/ws/.git")).is_none());
        assert!(tree.find(Path::new("/ws/target")).is_none());
        assert!(tree.find(Path::new("/ws/node_modules")).is_none());
        assert!(tree.find(Path::new("/ws/src")).unwrap().is_dir());
        assert!(tree.find(Path::new("/ws/src/lib.rs")).unwrap().is_file());
        assert!(tree
            .find(Path::new("/ws/empty"))
            .unwrap()
            .children()
            .is_empty());
        assert!(tree.find(Path::new("/ws/nope")).is_none());
    }

    #[test]
    fn file_tree_composite_files_are_leaves_dirs_have_children() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        let readme = tree.find(Path::new("/ws/README.md")).unwrap();
        assert_eq!(readme.name(), "README.md");
        assert_eq!(readme.path(), Path::new("/ws/README.md"));
        assert!(readme.is_file());
        assert!(!readme.is_dir());
        assert!(readme.children().is_empty());

        let src = tree.find(Path::new("/ws/src")).unwrap();
        assert!(src.is_dir());
        assert!(!src.is_file());
        assert_eq!(src.children().len(), 2);
        assert_eq!(src.children()[0].name(), "lib.rs");
        assert_eq!(src.children()[1].name(), "main.rs");

        let github = tree
            .find(Path::new("/ws/.github/workflows/ci.yml"))
            .unwrap();
        assert!(github.is_file());
    }

    #[test]
    fn file_tree_composite_load_rejects_file_root() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"").unwrap();
        let file_root = WorkspaceRoot::from_canonical("/ws/a.rs").unwrap();
        assert!(FileTree::load(&file_root, &fs)
            .unwrap_err()
            .is_not_a_directory());
        let missing = WorkspaceRoot::from_canonical("/nope").unwrap();
        assert!(FileTree::load(&missing, &fs)
            .unwrap_err()
            .is_not_a_directory());
    }

    #[test]
    fn file_tree_does_not_skip_file_named_like_filter() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/target", b"not a dir").unwrap();
        fs.add_file("/ws/src/a.rs", b"").unwrap();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        assert!(tree.find(Path::new("/ws/target")).unwrap().is_file());
        assert!(tree.find(Path::new("/ws/src/a.rs")).is_some());
    }
}
