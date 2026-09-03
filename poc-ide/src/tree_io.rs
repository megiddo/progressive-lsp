//! Tree expand Command + Event mailbox. The UI submits [`TreeIoRequest`] and
//! polls [`TreeIoEvent`]. The worker (or a test pump) owns [`FsPort::read_dir`].
//! A UI-facing apply path never calls `read_dir`.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::error::IdeError;
use crate::ports::{FsPort, StdFs};
use crate::tree::{CompactChainListing, ExpandChainCommand};

/// Command the UI sends to the tree worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeIoRequest {
    Expand { path: PathBuf },
}

impl TreeIoRequest {
    pub fn expand(path: impl AsRef<Path>) -> Self {
        Self::Expand {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Expand { path } => path,
        }
    }

    pub fn is_expand(&self) -> bool {
        matches!(self, Self::Expand { .. })
    }
}

/// Event the worker yields after listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeIoEvent {
    Expanded { listing: CompactChainListing },
    Failed { path: PathBuf, error: String },
}

impl TreeIoEvent {
    pub fn is_expanded(&self) -> bool {
        matches!(self, Self::Expanded { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Expanded { listing } => listing.path(),
            Self::Failed { path, .. } => path,
        }
    }
}

/// Paths whose compact-chain listing is in flight. Value object, not a Manager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeExpandFlight {
    pending: BTreeSet<PathBuf>,
}

impl TreeExpandFlight {
    pub fn idle() -> Self {
        Self::default()
    }

    pub fn begin(&mut self, path: impl AsRef<Path>) -> bool {
        self.pending.insert(path.as_ref().to_path_buf())
    }

    pub fn finish(&mut self, path: &Path) {
        self.pending.remove(path);
    }

    pub fn is_loading(&self, path: &Path) -> bool {
        self.pending.contains(path)
    }

    pub fn can_expand(&self, path: &Path) -> bool {
        !self.pending.contains(path)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn loading_label() -> &'static str {
        "loading…"
    }
}

/// Command queue + Event inbox. `submit` / `poll` never call [`FsPort::read_dir`].
#[derive(Debug, Default)]
pub struct TreeIoMailbox {
    requests: VecDeque<TreeIoRequest>,
    events: VecDeque<TreeIoEvent>,
}

impl TreeIoMailbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&mut self, req: TreeIoRequest) {
        self.requests.push_back(req);
    }

    pub fn take_requests(&mut self) -> Vec<TreeIoRequest> {
        self.requests.drain(..).collect()
    }

    pub fn pending_len(&self) -> usize {
        self.requests.len()
    }

    pub fn push_event(&mut self, ev: TreeIoEvent) {
        self.events.push_back(ev);
    }

    pub fn poll(&mut self) -> Vec<TreeIoEvent> {
        self.events.drain(..).collect()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty() && self.events.is_empty()
    }
}

/// UI-facing channel ends. `submit` / `poll` never call [`FsPort::read_dir`].
#[derive(Debug)]
pub struct TreeIoHandle {
    tx: mpsc::Sender<TreeIoRequest>,
    rx: mpsc::Receiver<TreeIoEvent>,
}

impl TreeIoHandle {
    pub fn pair() -> (
        Self,
        mpsc::Receiver<TreeIoRequest>,
        mpsc::Sender<TreeIoEvent>,
    ) {
        let (req_tx, req_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        (
            Self {
                tx: req_tx,
                rx: ev_rx,
            },
            req_rx,
            ev_tx,
        )
    }

    pub fn submit(&self, req: TreeIoRequest) -> Result<(), IdeError> {
        self.tx
            .send(req)
            .map_err(|_| IdeError::Io(std::io::Error::other("tree io thread ended")))
    }

    pub fn poll(&self) -> Vec<TreeIoEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

/// Worker apply. Tests drive this with [`crate::ports::MemFs`]. This path may call `read_dir`.
pub fn dispatch_tree_io<T: FsPort + ?Sized>(fs: &T, req: TreeIoRequest) -> TreeIoEvent {
    match req {
        TreeIoRequest::Expand { path } => match ExpandChainCommand::new(&path).apply(fs) {
            Ok(listing) => TreeIoEvent::Expanded { listing },
            Err(e) => TreeIoEvent::Failed {
                path,
                error: e.to_string(),
            },
        },
    }
}

pub fn pump_tree_io<T: FsPort + ?Sized>(mailbox: &mut TreeIoMailbox, fs: &T) {
    for req in mailbox.take_requests() {
        mailbox.push_event(dispatch_tree_io(fs, req));
    }
}

/// One named thread owns [`StdFs::read_dir`] for compact-chain expand.
pub fn spawn_tree_io() -> TreeIoHandle {
    let (handle, req_rx, ev_tx) = TreeIoHandle::pair();
    let _ = std::thread::Builder::new()
        .name("poc-ide-tree".into())
        .spawn(move || run_tree_io(req_rx, ev_tx));
    handle
}

fn run_tree_io(req_rx: mpsc::Receiver<TreeIoRequest>, ev_tx: mpsc::Sender<TreeIoEvent>) {
    let fs = StdFs;
    while let Ok(req) = req_rx.recv() {
        if ev_tx.send(dispatch_tree_io(&fs, req)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeClock, MemFs};
    use crate::tree::{FileTree, WorkspaceRoot};
    use std::path::Path;

    fn chain_fs() -> MemFs {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a/b/c/file.rs", b"").unwrap();
        fs.add_file("/ws/README.md", b"hi").unwrap();
        fs
    }

    #[test]
    fn tree_io_request_command_expand_path() {
        let req = TreeIoRequest::expand("/ws/a");
        assert!(req.is_expand());
        assert_eq!(req.path(), Path::new("/ws/a"));
        assert_eq!(
            req,
            TreeIoRequest::Expand {
                path: "/ws/a".into()
            }
        );
        assert_ne!(req, TreeIoRequest::expand("/ws/b"));
    }

    #[test]
    fn tree_expand_flight_value_object_disables_until_ready() {
        let mut flight = TreeExpandFlight::idle();
        assert!(flight.is_empty());
        assert_eq!(flight.len(), 0);
        assert!(flight.can_expand(Path::new("/ws/a")));
        assert!(!flight.is_loading(Path::new("/ws/a")));
        assert_eq!(TreeExpandFlight::loading_label(), "loading…");
        assert!(flight.begin("/ws/a"));
        assert!(!flight.begin("/ws/a"));
        assert!(flight.is_loading(Path::new("/ws/a")));
        assert!(!flight.can_expand(Path::new("/ws/a")));
        assert!(flight.can_expand(Path::new("/ws/b")));
        assert_eq!(flight.len(), 1);
        assert!(!flight.is_empty());
        flight.finish(Path::new("/ws/a"));
        assert!(flight.can_expand(Path::new("/ws/a")));
        assert!(flight.is_empty());
        assert_eq!(TreeExpandFlight::default(), TreeExpandFlight::idle());
    }

    #[test]
    fn tree_io_mailbox_submit_poll_never_calls_read_dir() {
        let mut mailbox = TreeIoMailbox::new();
        assert!(mailbox.is_empty());
        assert_eq!(mailbox.pending_len(), 0);
        assert_eq!(mailbox.event_len(), 0);
        mailbox.submit(TreeIoRequest::expand("/ws/a"));
        assert_eq!(mailbox.pending_len(), 1);
        assert!(!mailbox.is_empty());
        let reqs = mailbox.take_requests();
        assert_eq!(reqs.len(), 1);
        assert!(mailbox.take_requests().is_empty());
        mailbox.push_event(TreeIoEvent::Failed {
            path: "/ws/a".into(),
            error: "x".into(),
        });
        assert_eq!(mailbox.event_len(), 1);
        let evs = mailbox.poll();
        assert_eq!(evs.len(), 1);
        assert!(evs[0].is_failed());
        assert_eq!(evs[0].path(), Path::new("/ws/a"));
        assert!(mailbox.poll().is_empty());
        assert!(mailbox.is_empty());
    }

    #[test]
    fn expand_chain_command_and_apply_listing_use_memfs() {
        let fs = chain_fs();
        let _ = FakeClock::at_unix_ms(1);
        let listing = ExpandChainCommand::new("/ws/a").apply(&fs).unwrap();
        assert_eq!(listing.path(), Path::new("/ws/a"));
        assert!(!listing.is_empty());
        assert_eq!(listing.children_count(), 1);
        assert_eq!(listing.levels().len(), 3);
        assert_eq!(listing.levels()[0].0, PathBuf::from("/ws/a"));
        assert_eq!(listing.levels()[2].1[0].name(), "file.rs");

        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        assert!(tree.needs_compact_listing(Path::new("/ws/a")));
        tree.apply_listing(&listing).unwrap();
        assert!(!tree.needs_compact_listing(Path::new("/ws/a")));
        assert!(tree.find(Path::new("/ws/a/b/c")).unwrap().is_loaded());
        assert_eq!(
            tree.find(Path::new("/ws/a/b/c")).unwrap().children()[0].name(),
            "file.rs"
        );
        tree.apply_listing(&listing).unwrap();
        assert_eq!(ExpandChainCommand::new("/ws/a").path(), Path::new("/ws/a"));
    }

    #[test]
    fn pump_tree_io_lists_on_worker_then_ui_applies() {
        let fs = chain_fs();
        let _ = FakeClock::at_unix_ms(2);
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        let mut mailbox = TreeIoMailbox::new();
        mailbox.submit(TreeIoRequest::expand("/ws/a"));
        pump_tree_io(&mut mailbox, &fs);
        let events = mailbox.poll();
        assert_eq!(events.len(), 1);
        assert!(events[0].is_expanded());
        assert!(!events[0].is_failed());
        match &events[0] {
            TreeIoEvent::Expanded { listing } => {
                tree.apply_listing(listing).unwrap();
            }
            TreeIoEvent::Failed { .. } => panic!("expected expanded"),
        }
        assert!(tree.find(Path::new("/ws/a/b/c")).unwrap().is_loaded());

        mailbox.submit(TreeIoRequest::expand("/ws/missing"));
        pump_tree_io(&mut mailbox, &fs);
        let failed = mailbox.poll();
        assert!(failed[0].is_failed());
        assert!(!failed[0].is_expanded());
        assert_eq!(failed[0].path(), Path::new("/ws/missing"));
        match &failed[0] {
            TreeIoEvent::Failed { error, .. } => assert!(error.contains("not found")),
            TreeIoEvent::Expanded { .. } => panic!("expected failed"),
        }
    }

    #[test]
    fn tree_io_handle_submit_poll_never_calls_read_dir() {
        let (handle, req_rx, ev_tx) = TreeIoHandle::pair();
        handle.submit(TreeIoRequest::expand("/ws/a")).unwrap();
        let req = req_rx.recv().unwrap();
        assert!(req.is_expand());
        ev_tx
            .send(TreeIoEvent::Failed {
                path: "/ws/a".into(),
                error: "no".into(),
            })
            .unwrap();
        let evs = handle.poll();
        assert_eq!(evs.len(), 1);
        assert!(evs[0].is_failed());
        assert!(handle.poll().is_empty());
        drop(req_rx);
        assert!(handle.submit(TreeIoRequest::expand("/ws/b")).is_err());
        let _ = spawn_tree_io;
        let _ = dispatch_tree_io::<MemFs>;
        let _ = pump_tree_io::<MemFs>;
    }

    #[test]
    fn apply_listing_root_is_noop_and_missing_is_not_found() {
        let fs = chain_fs();
        let root = WorkspaceRoot::from_canonical("/ws").unwrap();
        let mut tree = FileTree::load(&root, &fs).unwrap();
        tree.apply_listing(&CompactChainListing::empty("/ws"))
            .unwrap();
        let listing = ExpandChainCommand::new("/ws/a").apply(&fs).unwrap();
        let mut other = FileTree::load(&root, &fs).unwrap();
        assert!(other.apply_listing(&listing).is_ok());
        let mut missing = FileTree::load(&root, &fs).unwrap();
        let orphan = ExpandChainCommand::new("/ws/a/b").apply(&fs).unwrap();
        assert!(missing.apply_listing(&orphan).unwrap_err().is_not_found());
        assert!(tree.needs_compact_listing(Path::new("/ws/missing")));
        assert!(!tree.needs_compact_listing(Path::new("/ws/README.md")));
        assert!(CompactChainListing::empty("/ws").is_empty());
        assert_eq!(CompactChainListing::empty("/ws").children_count(), 0);
    }
}
