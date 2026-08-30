//! `DialogPort` / `ClipboardPort` / `FsPort` / `WatchPort` / `ClockPort` /
//! `LspTransport` plus adapters and test doubles. Tests never call `rfd`, OS
//! clipboard, `notify` OS APIs, or host disk except through `StdFs` tests.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// In-memory clipboard. Cut/copy/paste never call the OS clipboard in tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeClipboard {
    text: String,
    fail_get: bool,
    fail_set: bool,
}

impl FakeClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contents(&self) -> &str {
        &self.text
    }

    pub fn fail_next_get(&mut self) {
        self.fail_get = true;
    }

    pub fn fail_next_set(&mut self) {
        self.fail_set = true;
    }
}

/// Cut/copy/paste go through this Port. Production adapter lives in the bin.
pub trait ClipboardPort {
    fn get_text(&mut self) -> Result<String, IdeError>;
    fn set_text(&mut self, text: &str) -> Result<(), IdeError>;
}

impl ClipboardPort for FakeClipboard {
    fn get_text(&mut self) -> Result<String, IdeError> {
        if self.fail_get {
            self.fail_get = false;
            return Err(IdeError::clipboard("fake get"));
        }
        Ok(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), IdeError> {
        if self.fail_set {
            self.fail_set = false;
            return Err(IdeError::clipboard("fake set"));
        }
        self.text = text.to_string();
        Ok(())
    }
}

/// Tree / canonicalize / bytes go through this Port. Tests use [`MemFs`].
pub trait FsPort {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, IdeError>;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, IdeError>;
    fn read(&self, path: &Path) -> Result<Vec<u8>, IdeError>;
    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), IdeError>;
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

    fn read(&self, path: &Path) -> Result<Vec<u8>, IdeError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(IdeError::NotFound(path.to_path_buf()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => {
                Err(IdeError::IsDirectory(path.to_path_buf()))
            }
            Err(e) => Err(IdeError::Io(e)),
        }
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), IdeError> {
        if path.is_dir() {
            return Err(IdeError::IsDirectory(path.to_path_buf()));
        }
        match std::fs::write(path, bytes) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(IdeError::NotFound(path.to_path_buf()))
            }
            Err(e) => Err(IdeError::Io(e)),
        }
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

    fn read(&self, path: &Path) -> Result<Vec<u8>, IdeError> {
        let path = require_absolute(path)?;
        if self.dirs.contains(&path) && !self.files.contains_key(&path) {
            return Err(IdeError::IsDirectory(path));
        }
        self.files
            .get(&path)
            .cloned()
            .ok_or(IdeError::NotFound(path))
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), IdeError> {
        let path = require_absolute(path)?;
        if self.dirs.contains(&path) {
            return Err(IdeError::IsDirectory(path));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.add_dir(parent)?;
            }
        }
        self.files.insert(path, bytes.to_vec());
        Ok(())
    }
}

/// create / modify / delete on disk. Event / DTO for [`WatchPort`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiskEventKind {
    Create,
    Modify,
    Delete,
}

impl DiskEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Modify => "modify",
            Self::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "modify" => Some(Self::Modify),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// One disk change. `mtime` is the event generation used to ignore KeepMemory repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskEvent {
    path: PathBuf,
    kind: DiskEventKind,
    mtime: u64,
}

impl DiskEvent {
    pub fn new(path: impl Into<PathBuf>, kind: DiskEventKind, mtime: u64) -> Self {
        Self {
            path: path.into(),
            kind,
            mtime,
        }
    }

    pub fn modify(path: impl Into<PathBuf>, mtime: u64) -> Self {
        Self::new(path, DiskEventKind::Modify, mtime)
    }

    pub fn create(path: impl Into<PathBuf>, mtime: u64) -> Self {
        Self::new(path, DiskEventKind::Create, mtime)
    }

    pub fn delete(path: impl Into<PathBuf>, mtime: u64) -> Self {
        Self::new(path, DiskEventKind::Delete, mtime)
    }

    pub fn at_clock(
        path: impl Into<PathBuf>,
        kind: DiskEventKind,
        clock: &impl ClockPort,
    ) -> Self {
        Self::new(path, kind, clock.unix_ms())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn kind(&self) -> DiskEventKind {
        self.kind
    }

    pub fn mtime(&self) -> u64 {
        self.mtime
    }
}

/// Injected clock. Tests use [`FakeClock`] and never `thread::sleep`.
pub trait ClockPort: Send + Sync {
    fn now(&self) -> Instant;
    fn unix_ms(&self) -> u64;
}

/// Wall clock. Not used in deterministic tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn unix_ms(&self) -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
            Err(_) => 0,
        }
    }
}

/// Deterministic clock. Advance with [`FakeClock::advance_ms`] — never sleep.
pub struct FakeClock {
    origin: Instant,
    offset_ms: AtomicU64,
    unix_ms: AtomicU64,
}

impl FakeClock {
    pub fn at_unix_ms(unix_ms: u64) -> Self {
        Self {
            origin: Instant::now(),
            offset_ms: AtomicU64::new(0),
            unix_ms: AtomicU64::new(unix_ms),
        }
    }

    pub fn advance_ms(&self, ms: u64) {
        self.offset_ms.fetch_add(ms, Ordering::SeqCst);
        self.unix_ms.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn offset_ms(&self) -> u64 {
        self.offset_ms.load(Ordering::SeqCst)
    }
}

impl ClockPort for FakeClock {
    fn now(&self) -> Instant {
        self.origin + Duration::from_millis(self.offset_ms())
    }

    fn unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst)
    }
}

/// Disk events go through this Port. Production [`crate::watch::NotifyWatch`] maps
/// `notify` types; the composition root owns the OS watcher. Tests use [`FakeWatch`].
pub trait WatchPort {
    fn watch(&mut self, path: &Path) -> Result<(), IdeError>;
    fn unwatch(&mut self, path: &Path);
    fn poll(&mut self) -> Vec<DiskEvent>;
}

/// Test double: inject events. Never calls `notify` OS APIs.
#[derive(Clone, Debug, Default)]
pub struct FakeWatch {
    events: VecDeque<DiskEvent>,
    watched: BTreeSet<PathBuf>,
}

impl FakeWatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inject(&mut self, event: DiskEvent) {
        self.events.push_back(event);
    }

    pub fn inject_modify(&mut self, path: impl Into<PathBuf>, mtime: u64) {
        self.inject(DiskEvent::modify(path, mtime));
    }

    pub fn queued_len(&self) -> usize {
        self.events.len()
    }

    pub fn is_watching(&self, path: impl AsRef<Path>) -> bool {
        self.watched.contains(path.as_ref())
    }

    pub fn watched_len(&self) -> usize {
        self.watched.len()
    }
}

/// JSON-RPC request/notify. Production [`crate::lsp::StdioLsp`]; tests use [`FakeLsp`].
pub trait LspTransport {
    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IdeError>;
    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), IdeError>;
}

/// One recorded request or notification on [`FakeLsp`].
#[derive(Clone, Debug, PartialEq)]
pub struct LspCall {
    pub notification: bool,
    pub method: String,
    pub params: serde_json::Value,
}

impl LspCall {
    pub fn request(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            notification: false,
            method: method.into(),
            params,
        }
    }

    pub fn notify(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            notification: true,
            method: method.into(),
            params,
        }
    }

    pub fn is_notification(&self) -> bool {
        self.notification
    }
}

/// Test double: scripted JSON-RPC results. Missing binary is a Result.
#[derive(Debug, Default)]
pub struct FakeLsp {
    by_method: BTreeMap<String, VecDeque<Result<serde_json::Value, IdeError>>>,
    sent: Vec<LspCall>,
    missing_binary: bool,
}

impl FakeLsp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn missing_binary() -> Self {
        Self {
            missing_binary: true,
            ..Self::default()
        }
    }

    pub fn is_missing_binary(&self) -> bool {
        self.missing_binary
    }

    pub fn script(&mut self, method: impl Into<String>, result: serde_json::Value) -> &mut Self {
        self.by_method
            .entry(method.into())
            .or_default()
            .push_back(Ok(result));
        self
    }

    pub fn script_error(&mut self, method: impl Into<String>, err: IdeError) -> &mut Self {
        self.by_method
            .entry(method.into())
            .or_default()
            .push_back(Err(err));
        self
    }

    pub fn script_method_missing(&mut self, method: impl Into<String>) -> &mut Self {
        let method = method.into();
        self.script_error(method.clone(), IdeError::lsp_method_missing(method))
    }

    pub fn sent(&self) -> &[LspCall] {
        &self.sent
    }

    pub fn sent_methods(&self) -> Vec<&str> {
        self.sent.iter().map(|c| c.method.as_str()).collect()
    }

    pub fn queued_len(&self, method: &str) -> usize {
        self.by_method.get(method).map(|q| q.len()).unwrap_or(0)
    }

    fn take_scripted(&mut self, method: &str) -> Result<serde_json::Value, IdeError> {
        if self.missing_binary {
            return Err(IdeError::MissingBinary);
        }
        if let Some(q) = self.by_method.get_mut(method) {
            if let Some(r) = q.pop_front() {
                return r;
            }
        }
        Ok(serde_json::Value::Null)
    }
}

impl LspTransport for FakeLsp {
    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IdeError> {
        self.sent.push(LspCall::request(method, params));
        self.take_scripted(method)
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), IdeError> {
        self.sent.push(LspCall::notify(method, params));
        self.take_scripted(method).map(|_| ())
    }
}

impl WatchPort for FakeWatch {
    fn watch(&mut self, path: &Path) -> Result<(), IdeError> {
        let path = require_absolute(path)?;
        self.watched.insert(path);
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) {
        self.watched.remove(path);
    }

    fn poll(&mut self) -> Vec<DiskEvent> {
        self.events.drain(..).collect()
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
    use std::time::Duration;

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
    fn fake_clipboard_test_double_never_calls_os() {
        let mut clip = FakeClipboard::new();
        assert_eq!(clip.contents(), "");
        assert_eq!(clip.get_text().unwrap(), "");
        clip.set_text("hello").unwrap();
        assert_eq!(clip.contents(), "hello");
        assert_eq!(clip.get_text().unwrap(), "hello");
        clip.set_text("").unwrap();
        assert_eq!(clip.contents(), "");
        assert_eq!(FakeClipboard::default(), FakeClipboard::new());

        clip.fail_next_set();
        assert!(clip.set_text("nope").unwrap_err().is_clipboard());
        assert_eq!(clip.contents(), "");
        clip.set_text("kept").unwrap();
        clip.fail_next_get();
        assert!(clip.get_text().unwrap_err().is_clipboard());
        assert_eq!(clip.get_text().unwrap(), "kept");
    }

    #[test]
    fn mem_fs_read_write_bytes_no_host_disk() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"fn x() {}").unwrap();
        assert_eq!(fs.read(Path::new("/ws/a.rs")).unwrap(), b"fn x() {}");
        fs.write(Path::new("/ws/a.rs"), b"fn y() {}").unwrap();
        assert_eq!(fs.read(Path::new("/ws/a.rs")).unwrap(), b"fn y() {}");
        fs.write(Path::new("/ws/new.rs"), b"new").unwrap();
        assert_eq!(fs.read(Path::new("/ws/new.rs")).unwrap(), b"new");
        fs.write(Path::new("/ws/deep/nested/f.rs"), b"z").unwrap();
        assert!(fs.is_dir(Path::new("/ws/deep")));
        assert!(fs.is_dir(Path::new("/ws/deep/nested")));
        assert_eq!(fs.read(Path::new("/ws/deep/nested/f.rs")).unwrap(), b"z");
        assert!(fs.read(Path::new("/missing")).unwrap_err().is_not_found());
        assert!(fs.read(Path::new("rel")).unwrap_err().is_not_absolute());
        assert!(fs
            .write(Path::new("rel.rs"), b"x")
            .unwrap_err()
            .is_not_absolute());
        assert!(fs.read(Path::new("/ws")).unwrap_err().is_directory());
        assert!(fs
            .write(Path::new("/ws"), b"no")
            .unwrap_err()
            .is_directory());
    }

    #[test]
    fn std_fs_read_write_bytes_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.rs");
        fs::write(&file, b"fn x() {}").unwrap();
        let mut port = StdFs;
        assert_eq!(port.read(&file).unwrap(), b"fn x() {}");
        port.write(&file, b"fn y() {}").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"fn y() {}");
        assert!(port
            .read(&tmp.path().join("missing.rs"))
            .unwrap_err()
            .is_not_found());
        assert!(port.write(tmp.path(), b"no").unwrap_err().is_directory());
        let nested = tmp.path().join("no-parent").join("x.rs");
        let err = port.write(&nested, b"x").unwrap_err();
        assert!(
            err.is_not_found(),
            "write without parent must be NotFound, not a generic Io remap: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn std_fs_read_directory_is_directory_or_io() {
        let tmp = tempfile::tempdir().unwrap();
        let err = StdFs.read(tmp.path()).unwrap_err();
        assert!(
            err.is_directory() || err.is_io(),
            "reading a directory must not panic: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn std_fs_write_permission_denied_is_io() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let locked = tmp.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let err = StdFs.write(&locked.join("new.rs"), b"y").unwrap_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            err.is_io(),
            "unwritable parent must be Io, not NotFound: {err}"
        );
        assert!(!err.is_not_found());
    }

    #[cfg(unix)]
    #[test]
    fn std_fs_read_permission_denied_is_io() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let locked = tmp.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let inner = locked.join("file");
        fs::write(&inner, b"x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let result = StdFs.read(&inner);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.unwrap_err();
        assert!(err.is_io(), "unreadable parent must be Io: {err}");
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

    #[test]
    fn disk_event_dto_path_kind_mtime_round_trip() {
        let ev = DiskEvent::new("/ws/a.rs", DiskEventKind::Modify, 7);
        assert_eq!(ev.path(), Path::new("/ws/a.rs"));
        assert_eq!(ev.kind(), DiskEventKind::Modify);
        assert_eq!(ev.mtime(), 7);
        assert_eq!(DiskEventKind::Create.as_str(), "create");
        assert_eq!(DiskEventKind::Modify.as_str(), "modify");
        assert_eq!(DiskEventKind::Delete.as_str(), "delete");
        assert_eq!(DiskEventKind::parse("create"), Some(DiskEventKind::Create));
        assert_eq!(DiskEventKind::parse("modify"), Some(DiskEventKind::Modify));
        assert_eq!(DiskEventKind::parse("delete"), Some(DiskEventKind::Delete));
        assert_eq!(DiskEventKind::parse("CREATE"), None);
        assert_eq!(DiskEventKind::parse(""), None);
        assert_eq!(DiskEventKind::parse("other"), None);
        let created = DiskEvent::create("/ws/b.rs", 1);
        assert_eq!(created.kind(), DiskEventKind::Create);
        assert_eq!(created.mtime(), 1);
        let deleted = DiskEvent::delete("/ws/c.rs", 2);
        assert_eq!(deleted.kind(), DiskEventKind::Delete);
        assert_eq!(DiskEvent::modify("/ws/a.rs", 7), ev);
    }

    #[test]
    fn clock_port_fake_clock_never_sleeps_advance_is_deterministic() {
        let clock = FakeClock::at_unix_ms(1_700_000_000_000);
        assert_eq!(clock.unix_ms(), 1_700_000_000_000);
        assert_eq!(clock.offset_ms(), 0);
        let a = clock.now();
        let b = clock.now();
        assert_eq!(a, b);
        clock.advance_ms(0);
        assert_eq!(clock.unix_ms(), 1_700_000_000_000);
        clock.advance_ms(250);
        assert_eq!(clock.unix_ms(), 1_700_000_000_250);
        assert_eq!(clock.offset_ms(), 250);
        assert_eq!(clock.now().duration_since(a), Duration::from_millis(250));
        clock.advance_ms(50);
        assert_eq!(clock.unix_ms(), 1_700_000_000_300);
        let stamped = DiskEvent::at_clock("/ws/a.rs", DiskEventKind::Modify, &clock);
        assert_eq!(stamped.mtime(), 1_700_000_000_300);
        assert_eq!(stamped.kind(), DiskEventKind::Modify);
    }

    #[test]
    fn system_clock_reports_sane_unix_ms() {
        let clock = SystemClock;
        let _ = clock.now();
        let ms = clock.unix_ms();
        assert!(ms > 1_577_836_800_000, "unix_ms={ms}");
        assert!(ms < 10_000_000_000_000, "unix_ms={ms}");
        let _ = SystemClock;
        let _ = SystemClock::default();
    }

    #[test]
    fn fake_watch_test_double_injects_events_without_os() {
        let mut watch = FakeWatch::new();
        assert_eq!(watch.queued_len(), 0);
        assert_eq!(watch.watched_len(), 0);
        assert!(!watch.is_watching("/ws"));
        assert!(watch.poll().is_empty());
        assert_eq!(FakeWatch::default().queued_len(), 0);

        assert!(watch.watch(Path::new("rel")).unwrap_err().is_not_absolute());
        watch.watch(Path::new("/ws")).unwrap();
        assert!(watch.is_watching("/ws"));
        assert_eq!(watch.watched_len(), 1);
        watch.watch(Path::new("/ws")).unwrap();
        assert_eq!(watch.watched_len(), 1);
        watch.unwatch(Path::new("/other"));
        assert!(watch.is_watching("/ws"));
        watch.unwatch(Path::new("/ws"));
        assert!(!watch.is_watching("/ws"));
        assert_eq!(watch.watched_len(), 0);

        watch.inject_modify("/ws/a.rs", 11);
        watch.inject(DiskEvent::create("/ws/b.rs", 12));
        watch.inject(DiskEvent::delete("/ws/c.rs", 13));
        assert_eq!(watch.queued_len(), 3);
        let events = watch.poll();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].path(), Path::new("/ws/a.rs"));
        assert_eq!(events[0].kind(), DiskEventKind::Modify);
        assert_eq!(events[0].mtime(), 11);
        assert_eq!(events[1].kind(), DiskEventKind::Create);
        assert_eq!(events[2].kind(), DiskEventKind::Delete);
        assert!(watch.poll().is_empty());
        assert_eq!(watch.queued_len(), 0);
    }

    #[test]
    fn fake_lsp_test_double_scripted_responses() {
        let mut lsp = FakeLsp::new();
        assert!(!lsp.is_missing_binary());
        assert_eq!(lsp.queued_len("initialize"), 0);
        assert_eq!(FakeLsp::default().sent().len(), 0);
        lsp.script("initialize", serde_json::json!({"ok": true}));
        lsp.script("initialize", serde_json::json!({"ok": false}));
        assert_eq!(lsp.queued_len("initialize"), 2);
        let first = lsp
            .request("initialize", serde_json::json!({"root": "/ws"}))
            .unwrap();
        assert_eq!(first, serde_json::json!({"ok": true}));
        assert_eq!(lsp.queued_len("initialize"), 1);
        lsp.notify("initialized", serde_json::json!({})).unwrap();
        let unscripted = lsp
            .request("textDocument/definition", serde_json::json!(null))
            .unwrap();
        assert_eq!(unscripted, serde_json::Value::Null);
        assert_eq!(
            lsp.sent_methods(),
            vec![
                "initialize",
                "initialized",
                "textDocument/definition"
            ]
        );
        assert!(!lsp.sent()[0].is_notification());
        assert!(lsp.sent()[1].is_notification());
        assert_eq!(lsp.sent()[0].params["root"], "/ws");
        let req = LspCall::request("m", serde_json::json!(1));
        assert!(!req.is_notification());
        let note = LspCall::notify("n", serde_json::json!(2));
        assert!(note.is_notification());
        assert_eq!(req.method, "m");
        assert_eq!(note.method, "n");
    }

    #[test]
    fn fake_lsp_test_double_missing_binary_is_result() {
        let mut lsp = FakeLsp::missing_binary();
        assert!(lsp.is_missing_binary());
        assert!(lsp
            .request("initialize", serde_json::json!({}))
            .unwrap_err()
            .is_missing_binary());
        assert!(lsp
            .notify("initialized", serde_json::json!({}))
            .unwrap_err()
            .is_missing_binary());
        assert_eq!(lsp.sent_methods(), vec!["initialize", "initialized"]);
        let mut scripted = FakeLsp::new();
        scripted.script_error("shutdown", IdeError::lsp("gone"));
        scripted.script_method_missing("textDocument/implementation");
        assert!(scripted
            .request("shutdown", serde_json::json!({}))
            .unwrap_err()
            .is_lsp());
        assert!(scripted
            .request("textDocument/implementation", serde_json::json!({}))
            .unwrap_err()
            .is_lsp_method_missing());
        scripted.notify("exit", serde_json::json!({})).unwrap();
        assert!(scripted
            .sent()
            .iter()
            .all(|c| !c.method.contains("filesSince") && !c.method.starts_with("$/")));
    }
}
