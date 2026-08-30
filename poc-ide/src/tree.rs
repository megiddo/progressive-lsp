//! `WorkspaceRoot` value object, `FileTree` / `TreeNode` Composite, and
//! [`TreeExpansion`] (collapsed by default).

use std::collections::BTreeSet;
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
///
/// A directory's `children` is `None` until [`TreeNode::load_children`] /
/// [`FileTree::expand`] lists that one level. `Some(vec![])` is an empty
/// loaded folder — not the same as unloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeNode {
    File {
        name: String,
        path: PathBuf,
    },
    Directory {
        name: String,
        path: PathBuf,
        children: Option<Vec<TreeNode>>,
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
            Self::Directory {
                children: Some(children),
                ..
            } => children,
            Self::Directory { children: None, .. } | Self::File { .. } => &[],
        }
    }

    fn children_mut(&mut self) -> &mut [TreeNode] {
        match self {
            Self::Directory {
                children: Some(children),
                ..
            } => children,
            Self::Directory { children: None, .. } | Self::File { .. } => &mut [],
        }
    }

    /// Files are leaves (always loaded). Directories are loaded after
    /// [`TreeNode::load_children`] / [`FileTree::expand`].
    pub fn is_loaded(&self) -> bool {
        match self {
            Self::File { .. } => true,
            Self::Directory { children, .. } => children.is_some(),
        }
    }

    /// Command: list this directory's **immediate** children. Already-loaded
    /// directories are a no-op (do not re-walk). Files are [`IdeError::NotADirectory`].
    pub fn load_children(&mut self, fs: &(impl FsPort + ?Sized)) -> Result<(), IdeError> {
        match self {
            Self::File { path, .. } => Err(IdeError::NotADirectory(path.clone())),
            Self::Directory {
                children: Some(_), ..
            } => Ok(()),
            Self::Directory { path, children, .. } => {
                *children = Some(list_immediate(path, fs)?);
                Ok(())
            }
        }
    }

    fn from_entry(entry: DirEntry) -> Self {
        if entry.is_dir {
            Self::Directory {
                name: entry.name,
                path: entry.path,
                children: None,
            }
        } else {
            Self::File {
                name: entry.name,
                path: entry.path,
            }
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
        let children = list_immediate(root.as_path(), fs)?;
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

    /// Command: load one directory's immediate children via [`FsPort::read_dir`].
    /// The workspace root is already listed by [`FileTree::load`] — expanding it
    /// is a no-op. A path not yet in the tree is [`IdeError::NotFound`]. A file
    /// path is [`IdeError::NotADirectory`]. Expanding twice is idempotent.
    pub fn expand(&mut self, path: &Path, fs: &(impl FsPort + ?Sized)) -> Result<(), IdeError> {
        if path == self.root.as_path() {
            return Ok(());
        }
        match self.find_mut(path) {
            None => Err(IdeError::NotFound(path.to_path_buf())),
            Some(node) => node.load_children(fs),
        }
    }

    pub fn find(&self, path: &Path) -> Option<&TreeNode> {
        find_node(&self.children, path)
    }

    pub fn find_mut(&mut self, path: &Path) -> Option<&mut TreeNode> {
        find_node_mut(&mut self.children, path)
    }
}

/// Paths the user has explicitly expanded. A path is expanded iff it is in this
/// collection; **default is collapsed at every level**. Opening a new
/// [`FileTree`] / [`TreeExpansion::for_root`] starts empty.
///
/// [`TreeExpansion::expand`] / [`TreeExpansion::collapse`] are Commands.
/// Expanding a parent does not expand children. Collapse of a missing path is a
/// no-op. Expanding a file is a no-op.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeExpansion {
    expanded: BTreeSet<PathBuf>,
}

impl TreeExpansion {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty expansion for a newly opened workspace root.
    pub fn for_root(_root: &WorkspaceRoot) -> Self {
        Self::new()
    }

    pub fn is_empty(&self) -> bool {
        self.expanded.is_empty()
    }

    pub fn len(&self) -> usize {
        self.expanded.len()
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub fn expanded_paths(&self) -> impl Iterator<Item = &Path> {
        self.expanded.iter().map(PathBuf::as_path)
    }

    /// Command: mark `path` expanded when it is a directory in `tree`.
    /// The workspace root is always a directory. Expanding a file is a no-op.
    /// A path not in the tree is [`IdeError::NotFound`]. Idempotent.
    pub fn expand(&mut self, path: &Path, tree: &FileTree) -> Result<(), IdeError> {
        if path == tree.root().as_path() {
            self.expanded.insert(path.to_path_buf());
            return Ok(());
        }
        match tree.find(path) {
            None => Err(IdeError::NotFound(path.to_path_buf())),
            Some(node) if node.is_file() => Ok(()),
            Some(_) => {
                self.expanded.insert(path.to_path_buf());
                Ok(())
            }
        }
    }

    /// Command: collapse `path`. A path that is not expanded is a no-op.
    pub fn collapse(&mut self, path: &Path) {
        self.expanded.remove(path);
    }

    /// Drop every expanded path (new workspace root).
    pub fn clear(&mut self) {
        self.expanded.clear();
    }
}

/// One-level listing Command used by [`FileTree::load`] and [`TreeNode::load_children`].
fn list_immediate(dir: &Path, fs: &(impl FsPort + ?Sized)) -> Result<Vec<TreeNode>, IdeError> {
    let mut entries = fs.read_dir(dir)?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut nodes = Vec::new();
    for entry in entries {
        if entry.is_dir && FileTree::skips_display_name(&entry.name) {
            continue;
        }
        nodes.push(TreeNode::from_entry(entry));
    }
    Ok(nodes)
}

fn find_node<'a>(nodes: &'a [TreeNode], path: &Path) -> Option<&'a TreeNode> {
    for node in nodes {
        if node.path() == path {
            return Some(node);
        }
        if node.is_loaded() {
            if let Some(hit) = find_node(node.children(), path) {
                return Some(hit);
            }
        }
    }
    None
}

fn find_node_mut<'a>(nodes: &'a mut [TreeNode], path: &Path) -> Option<&'a mut TreeNode> {
    if let Some(i) = nodes.iter().position(|node| node.path() == path) {
        return Some(&mut nodes[i]);
    }
    for node in nodes {
        if node.is_loaded() {
            if let Some(hit) = find_node_mut(node.children_mut(), path) {
                return Some(hit);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{DirEntry, FsPort, MemFs};
    use std::cell::{Cell, RefCell};

    /// Decorator / test double: records `read_dir` paths on an inner [`FsPort`].
    struct CountingFs {
        inner: MemFs,
        read_dirs: RefCell<Vec<PathBuf>>,
        fail_next: Cell<bool>,
    }

    impl CountingFs {
        fn wrap(inner: MemFs) -> Self {
            Self {
                inner,
                read_dirs: RefCell::new(Vec::new()),
                fail_next: Cell::new(false),
            }
        }

        fn read_dir_paths(&self) -> Vec<PathBuf> {
            self.read_dirs.borrow().clone()
        }

        fn read_dir_called(&self, path: &Path) -> bool {
            self.read_dirs.borrow().iter().any(|p| p == path)
        }

        fn read_dir_count(&self) -> usize {
            self.read_dirs.borrow().len()
        }

        fn fail_next_read_dir(&self) {
            self.fail_next.set(true);
        }
    }

    impl FsPort for CountingFs {
        fn canonicalize(&self, path: &Path) -> Result<PathBuf, IdeError> {
            self.inner.canonicalize(path)
        }

        fn is_dir(&self, path: &Path) -> bool {
            self.inner.is_dir(path)
        }

        fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, IdeError> {
            self.read_dirs.borrow_mut().push(path.to_path_buf());
            if self.fail_next.replace(false) {
                return Err(IdeError::Io(std::io::Error::other("spy fail")));
            }
            self.inner.read_dir(path)
        }

        fn read(&self, path: &Path) -> Result<Vec<u8>, IdeError> {
            self.inner.read(path)
        }

        fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), IdeError> {
            self.inner.write(path, bytes)
        }
    }

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
        let mut tree = FileTree::load(&root, &fs).unwrap();
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
        tree.expand(Path::new("/ws/src"), &fs).unwrap();
        assert!(tree.find(Path::new("/ws/src/lib.rs")).unwrap().is_file());
        tree.expand(Path::new("/ws/empty"), &fs).unwrap();
        let empty = tree.find(Path::new("/ws/empty")).unwrap();
        assert!(empty.is_loaded());
        assert!(empty.children().is_empty());
        assert!(tree.find(Path::new("/ws/nope")).is_none());
    }

    #[test]
    fn file_tree_composite_files_are_leaves_dirs_have_children() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        let readme = tree.find(Path::new("/ws/README.md")).unwrap();
        assert_eq!(readme.name(), "README.md");
        assert_eq!(readme.path(), Path::new("/ws/README.md"));
        assert!(readme.is_file());
        assert!(!readme.is_dir());
        assert!(readme.is_loaded());
        assert!(readme.children().is_empty());

        tree.expand(Path::new("/ws/src"), &fs).unwrap();
        let src = tree.find(Path::new("/ws/src")).unwrap();
        assert!(src.is_dir());
        assert!(!src.is_file());
        assert!(src.is_loaded());
        assert_eq!(src.children().len(), 2);
        assert_eq!(src.children()[0].name(), "lib.rs");
        assert_eq!(src.children()[1].name(), "main.rs");

        tree.expand(Path::new("/ws/.github"), &fs).unwrap();
        tree.expand(Path::new("/ws/.github/workflows"), &fs)
            .unwrap();
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
        let mut tree = FileTree::load(&root, &fs).unwrap();
        assert!(tree.find(Path::new("/ws/target")).unwrap().is_file());
        tree.expand(Path::new("/ws/src"), &fs).unwrap();
        assert!(tree.find(Path::new("/ws/src/a.rs")).is_some());
    }

    #[test]
    fn file_tree_load_is_shallow_and_does_not_read_grandchildren() {
        let spy = CountingFs::wrap(sample_fs());
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &spy).unwrap();
        assert_eq!(spy.read_dir_paths(), vec![PathBuf::from("/ws")]);
        assert!(spy.read_dir_called(Path::new("/ws")));
        assert!(!spy.read_dir_called(Path::new("/ws/src")));
        assert!(!spy.read_dir_called(Path::new("/ws/.github")));
        assert!(!spy.read_dir_called(Path::new("/ws/empty")));
        assert!(!spy.read_dir_called(Path::new("/ws/.git")));
        assert!(tree.find(Path::new("/ws/src/lib.rs")).is_none());
        let src = tree.find(Path::new("/ws/src")).unwrap();
        assert!(src.is_dir());
        assert!(!src.is_loaded());
        assert!(src.children().is_empty());
        let empty = tree.find(Path::new("/ws/empty")).unwrap();
        assert!(!empty.is_loaded());
        assert!(empty.children().is_empty());
    }

    #[test]
    fn file_tree_expand_loads_one_level_and_distinguishes_empty() {
        let spy = CountingFs::wrap(sample_fs());
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &spy).unwrap();
        assert_eq!(spy.read_dir_count(), 1);

        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        assert!(spy.read_dir_called(Path::new("/ws/src")));
        assert!(!spy.read_dir_called(Path::new("/ws/src/lib.rs")));
        let src = tree.find(Path::new("/ws/src")).unwrap();
        assert!(src.is_loaded());
        assert_eq!(src.children().len(), 2);
        assert!(tree.find(Path::new("/ws/src/lib.rs")).unwrap().is_file());
        assert!(tree.find(Path::new("/ws/src/main.rs")).unwrap().is_file());

        tree.expand(Path::new("/ws/empty"), &spy).unwrap();
        let empty = tree.find(Path::new("/ws/empty")).unwrap();
        assert!(empty.is_loaded());
        assert!(empty.children().is_empty());
        assert_ne!(
            tree.find(Path::new("/ws/src")).unwrap().children().len(),
            tree.find(Path::new("/ws/empty")).unwrap().children().len()
        );
    }

    #[test]
    fn file_tree_expand_skip_filter_applies_on_each_level() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/src/lib.rs", b"").unwrap();
        fs.add_file("/ws/src/target/debug/x", b"").unwrap();
        fs.add_file("/ws/src/.git/HEAD", b"").unwrap();
        fs.add_file("/ws/src/node_modules/pkg/index.js", b"")
            .unwrap();
        fs.add_file("/ws/nested/target", b"file named target")
            .unwrap();
        let spy = CountingFs::wrap(fs);
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &spy).unwrap();
        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        assert!(tree.find(Path::new("/ws/src/lib.rs")).is_some());
        assert!(tree.find(Path::new("/ws/src/target")).is_none());
        assert!(tree.find(Path::new("/ws/src/.git")).is_none());
        assert!(tree.find(Path::new("/ws/src/node_modules")).is_none());
        tree.expand(Path::new("/ws/nested"), &spy).unwrap();
        assert!(tree.find(Path::new("/ws/nested/target")).unwrap().is_file());
    }

    #[test]
    fn file_tree_expand_missing_and_file_path_are_domain_errors() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        assert!(tree
            .expand(Path::new("/ws/nope"), &fs)
            .unwrap_err()
            .is_not_found());
        assert!(tree
            .expand(Path::new("/ws/src/lib.rs"), &fs)
            .unwrap_err()
            .is_not_found());
        tree.expand(Path::new("/ws/src"), &fs).unwrap();
        assert!(tree
            .expand(Path::new("/ws/src/lib.rs"), &fs)
            .unwrap_err()
            .is_not_a_directory());
        assert!(tree
            .find_mut(Path::new("/ws/README.md"))
            .unwrap()
            .load_children(&fs)
            .unwrap_err()
            .is_not_a_directory());
        assert!(tree
            .expand(Path::new("/ws/.github/workflows"), &fs)
            .unwrap_err()
            .is_not_found());
    }

    #[test]
    fn file_tree_expand_is_idempotent_and_root_is_noop() {
        let spy = CountingFs::wrap(sample_fs());
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &spy).unwrap();
        tree.expand(Path::new("/ws"), &spy).unwrap();
        assert_eq!(spy.read_dir_count(), 1);
        assert_eq!(spy.read_dir_paths(), vec![PathBuf::from("/ws")]);

        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        let after_first = spy.read_dir_count();
        assert_eq!(after_first, 2);
        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        assert_eq!(spy.read_dir_count(), after_first);
        assert_eq!(tree.find(Path::new("/ws/src")).unwrap().children().len(), 2);
        tree.find_mut(Path::new("/ws/src"))
            .unwrap()
            .load_children(&spy)
            .unwrap();
        assert_eq!(spy.read_dir_count(), after_first);
    }

    #[test]
    fn file_tree_expand_read_dir_failure_leaves_dir_unloaded() {
        let spy = CountingFs::wrap(sample_fs());
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &spy).unwrap();
        spy.fail_next_read_dir();
        let err = tree.expand(Path::new("/ws/src"), &spy).unwrap_err();
        assert!(err.is_io());
        assert!(!tree.find(Path::new("/ws/src")).unwrap().is_loaded());
        tree.expand(Path::new("/ws/src"), &spy).unwrap();
        assert!(tree.find(Path::new("/ws/src")).unwrap().is_loaded());
    }

    #[test]
    fn tree_expansion_value_object_new_tree_has_no_expanded_paths() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        let expansion = TreeExpansion::new();
        assert!(expansion.is_empty());
        assert_eq!(expansion.len(), 0);
        assert!(!expansion.is_expanded(root.as_path()));
        for node in tree.children() {
            assert!(
                !expansion.is_expanded(node.path()),
                "{}",
                node.path().display()
            );
        }
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
        assert!(!expansion.is_expanded(Path::new("/ws/.github")));
        assert!(!expansion.is_expanded(Path::new("/ws/README.md")));
        assert!(!expansion.is_expanded(Path::new("/ws/nope")));
        assert_eq!(expansion, TreeExpansion::for_root(&root));
        assert_eq!(expansion.expanded_paths().count(), 0);
    }

    #[test]
    fn tree_expansion_expand_collapse_cycle_ends_collapsed() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        let mut expansion = TreeExpansion::new();
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        assert!(expansion.is_expanded(Path::new("/ws/src")));
        assert_eq!(expansion.len(), 1);
        let paths: Vec<_> = expansion.expanded_paths().collect();
        assert_eq!(paths, vec![Path::new("/ws/src")]);
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        assert_eq!(expansion.len(), 1);
        expansion.collapse(Path::new("/ws/src"));
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
        assert!(expansion.is_empty());
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        expansion.collapse(Path::new("/ws/src"));
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
    }

    #[test]
    fn tree_expansion_expanding_parent_does_not_auto_expand_children() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        let mut expansion = TreeExpansion::new();
        expansion.expand(Path::new("/ws/.github"), &tree).unwrap();
        tree.expand(Path::new("/ws/.github"), &fs).unwrap();
        assert!(expansion.is_expanded(Path::new("/ws/.github")));
        assert!(!expansion.is_expanded(Path::new("/ws/.github/workflows")));
        tree.expand(Path::new("/ws/.github/workflows"), &fs)
            .unwrap();
        assert!(!expansion.is_expanded(Path::new("/ws/.github/workflows")));
        assert!(!expansion.is_expanded(Path::new("/ws/.github/workflows/ci.yml")));
        expansion
            .expand(Path::new("/ws/.github/workflows"), &tree)
            .unwrap();
        assert!(expansion.is_expanded(Path::new("/ws/.github")));
        assert!(expansion.is_expanded(Path::new("/ws/.github/workflows")));
        assert!(!expansion.is_expanded(Path::new("/ws/.github/workflows/ci.yml")));
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
    }

    #[test]
    fn tree_expansion_setting_workspace_root_clears() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        let mut expansion = TreeExpansion::for_root(&root);
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        expansion.expand(Path::new("/ws/empty"), &tree).unwrap();
        assert_eq!(expansion.len(), 2);
        let other = WorkspaceRoot::from_canonical("/other").unwrap();
        expansion = TreeExpansion::for_root(&other);
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
        assert!(!expansion.is_expanded(Path::new("/ws/empty")));
        assert!(expansion.is_empty());

        let mut expansion = TreeExpansion::for_root(&root);
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        expansion.clear();
        assert!(expansion.is_empty());
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
    }

    #[test]
    fn tree_expansion_collapse_missing_is_noop() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let tree = FileTree::load(&root, &fs).unwrap();
        let mut expansion = TreeExpansion::new();
        expansion.collapse(Path::new("/missing"));
        expansion.collapse(Path::new("/ws/src"));
        assert!(expansion.is_empty());
        expansion.expand(Path::new("/ws/src"), &tree).unwrap();
        expansion.collapse(Path::new("/missing"));
        expansion.collapse(Path::new("/ws/nope"));
        assert!(expansion.is_expanded(Path::new("/ws/src")));
        assert_eq!(expansion.len(), 1);
    }

    #[test]
    fn tree_expansion_expand_file_is_noop() {
        let fs = sample_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        let mut expansion = TreeExpansion::new();
        expansion.expand(Path::new("/ws/README.md"), &tree).unwrap();
        assert!(!expansion.is_expanded(Path::new("/ws/README.md")));
        assert!(expansion.is_empty());
        assert!(expansion
            .expand(Path::new("/ws/nope"), &tree)
            .unwrap_err()
            .is_not_found());
        expansion.expand(root.as_path(), &tree).unwrap();
        assert!(expansion.is_expanded(root.as_path()));
        tree.expand(Path::new("/ws/src"), &fs).unwrap();
        expansion
            .expand(Path::new("/ws/src/lib.rs"), &tree)
            .unwrap();
        assert!(!expansion.is_expanded(Path::new("/ws/src/lib.rs")));
        assert!(!expansion.is_expanded(Path::new("/ws/src")));
    }
}
