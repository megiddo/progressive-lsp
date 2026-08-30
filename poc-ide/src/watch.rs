//! `DiskWatch` Observer and `NotifyWatch` Adapter. Tests inject [`FakeWatch`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use notify::{Event, EventKind};

use crate::buffer::BufferMap;
use crate::conflict::{ConflictChoice, ConflictModal};
use crate::error::IdeError;
use crate::ports::{require_absolute, DiskEvent, DiskEventKind, FsPort, WatchPort};

/// How far a `WatchPort` subscription walks. Production folder open uses
/// [`WatchDepth::Immediate`] so binding a large tree does not block on a
/// recursive OS watch. Nested dirs are subscribed when the user expands them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WatchDepth {
    Immediate,
    Recursive,
}

impl WatchDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Recursive => "recursive",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "immediate" => Some(Self::Immediate),
            "recursive" => Some(Self::Recursive),
            _ => None,
        }
    }

    pub fn is_immediate(self) -> bool {
        matches!(self, Self::Immediate)
    }

    pub fn is_recursive(self) -> bool {
        matches!(self, Self::Recursive)
    }
}

/// Observer: watch events for an **open** path enqueue at most one pending modal.
#[derive(Clone, Debug, Default)]
pub struct DiskWatch {
    pending: Vec<ConflictModal>,
    ignored_mtime: BTreeMap<PathBuf, u64>,
}

impl DiskWatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn pending(&self) -> &[ConflictModal] {
        &self.pending
    }

    pub fn first_pending(&self) -> Option<&ConflictModal> {
        self.pending.first()
    }

    pub fn pending_for(&self, path: impl AsRef<Path>) -> Option<&ConflictModal> {
        let path = path.as_ref();
        self.pending.iter().find(|m| m.path() == path)
    }

    pub fn ignored_mtime(&self, path: impl AsRef<Path>) -> Option<u64> {
        self.ignored_mtime.get(path.as_ref()).copied()
    }

    /// Drain [`WatchPort`]. Always prompt for an open path (including clean buffers).
    pub fn ingest(&mut self, watch: &mut impl WatchPort, buffers: &BufferMap) {
        self.pending.retain(|m| buffers.contains(m.path()));
        for event in watch.poll() {
            let path = event.path();
            if !buffers.contains(path) {
                continue;
            }
            if self.pending.iter().any(|m| m.path() == path) {
                continue;
            }
            if self.ignored_mtime.get(path) == Some(&event.mtime()) {
                continue;
            }
            self.pending
                .push(ConflictModal::new(path.to_path_buf(), event.mtime()));
        }
    }

    pub fn resolve(
        &mut self,
        path: impl AsRef<Path>,
        choice: ConflictChoice,
        buffers: &mut BufferMap,
        fs: &(impl FsPort + ?Sized),
    ) -> Result<(), IdeError> {
        let path = path.as_ref();
        let Some(idx) = self.pending.iter().position(|m| m.path() == path) else {
            return Ok(());
        };
        let modal = self.pending[idx].clone();
        match modal.apply(choice, buffers, fs) {
            Ok(()) => {
                self.pending.remove(idx);
                self.ignored_mtime
                    .insert(modal.path().to_path_buf(), modal.mtime());
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

/// Maps `notify` events onto [`DiskEvent`]. Tests never start an OS watcher.
pub struct NotifyWatch {
    rx: Option<Receiver<notify::Result<Event>>>,
    pending: Vec<DiskEvent>,
    watched: BTreeSet<PathBuf>,
    next_mtime: u64,
}

impl Default for NotifyWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifyWatch {
    pub fn new() -> Self {
        Self {
            rx: None,
            pending: Vec::new(),
            watched: BTreeSet::new(),
            next_mtime: 1,
        }
    }

    /// Composition-root / test channel. Does not create a `RecommendedWatcher`.
    pub fn from_receiver(rx: Receiver<notify::Result<Event>>) -> Self {
        Self {
            rx: Some(rx),
            pending: Vec::new(),
            watched: BTreeSet::new(),
            next_mtime: 1,
        }
    }

    pub fn is_watching(&self, path: impl AsRef<Path>) -> bool {
        self.watched.contains(path.as_ref())
    }

    pub fn watched_len(&self) -> usize {
        self.watched.len()
    }

    pub fn queued_len(&self) -> usize {
        self.pending.len()
    }

    pub fn map_kind(kind: EventKind) -> Option<DiskEventKind> {
        match kind {
            EventKind::Create(_) => Some(DiskEventKind::Create),
            EventKind::Modify(_) => Some(DiskEventKind::Modify),
            EventKind::Remove(_) => Some(DiskEventKind::Delete),
            EventKind::Any => Some(DiskEventKind::Modify),
            EventKind::Access(_) | EventKind::Other => None,
        }
    }

    pub fn map_kind_str(kind: &str) -> Option<DiskEventKind> {
        match kind {
            "create" | "Create" | "any" => Some(DiskEventKind::Create),
            "modify" | "Modify" | "data" => Some(DiskEventKind::Modify),
            "remove" | "Remove" | "delete" => Some(DiskEventKind::Delete),
            _ => None,
        }
    }

    pub fn map_event(event: &Event, mtime: u64) -> Vec<DiskEvent> {
        let Some(kind) = Self::map_kind(event.kind) else {
            return Vec::new();
        };
        event
            .paths
            .iter()
            .map(|path| DiskEvent::new(path.clone(), kind, mtime))
            .collect()
    }

    pub fn push_mapped(&mut self, path: impl Into<PathBuf>, kind: &str, mtime: u64) {
        if let Some(k) = Self::map_kind_str(kind) {
            self.pending.push(DiskEvent::new(path, k, mtime));
        }
    }

    fn take_next_mtime(&mut self) -> u64 {
        let mtime = self.next_mtime;
        self.next_mtime = self.next_mtime.saturating_add(1);
        mtime
    }
}

impl WatchPort for NotifyWatch {
    fn watch(&mut self, path: &Path) -> Result<(), IdeError> {
        let path = require_absolute(path)?;
        self.watched.insert(path);
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) {
        self.watched.remove(path);
    }

    fn poll(&mut self) -> Vec<DiskEvent> {
        let incoming = match &self.rx {
            Some(rx) => {
                let mut batch = Vec::new();
                loop {
                    match rx.try_recv() {
                        Ok(item) => batch.push(item),
                        Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                    }
                }
                batch
            }
            None => Vec::new(),
        };
        for item in incoming {
            match item {
                Ok(event) => {
                    let mtime = self.take_next_mtime();
                    self.pending.extend(Self::map_event(&event, mtime));
                }
                Err(_) => {}
            }
        }
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditCommand;
    use crate::ports::{ClockPort, FakeClipboard, FakeClock, FakeWatch, MemFs};

    #[test]
    fn watch_depth_value_object_immediate_is_not_recursive() {
        assert_eq!(WatchDepth::Immediate.as_str(), "immediate");
        assert_eq!(WatchDepth::Recursive.as_str(), "recursive");
        assert_eq!(WatchDepth::parse("immediate"), Some(WatchDepth::Immediate));
        assert_eq!(WatchDepth::parse("recursive"), Some(WatchDepth::Recursive));
        assert_eq!(WatchDepth::parse("all"), None);
        assert_eq!(WatchDepth::parse(""), None);
        assert!(WatchDepth::Immediate.is_immediate());
        assert!(!WatchDepth::Immediate.is_recursive());
        assert!(WatchDepth::Recursive.is_recursive());
        assert!(!WatchDepth::Recursive.is_immediate());
        assert_ne!(WatchDepth::Immediate, WatchDepth::Recursive);
    }
    use notify::event::{
        AccessKind, CreateKind, DataChange, EventAttributes, ModifyKind, RemoveKind,
    };
    use std::sync::mpsc;

    fn open_sample() -> (MemFs, BufferMap, FakeWatch, FakeClock) {
        let mut fs = MemFs::new();
        fs.add_file("/ws/a.rs", b"fn a() {}\n").unwrap();
        fs.add_file("/ws/b.rs", b"fn b() {}\n").unwrap();
        let mut buffers = BufferMap::new();
        buffers.open("/ws/a.rs", &fs).unwrap();
        (fs, buffers, FakeWatch::new(), FakeClock::at_unix_ms(1_000))
    }

    #[test]
    fn disk_watch_observer_enqueues_at_most_one_pending_conflict_per_path() {
        let (fs, buffers, mut watch, clock) = open_sample();
        let _ = fs;
        let mut disk = DiskWatch::new();
        assert!(disk.is_empty());
        assert_eq!(disk.pending_len(), 0);
        assert!(disk.first_pending().is_none());
        assert!(disk.pending_for("/ws/a.rs").is_none());
        assert_eq!(DiskWatch::default().pending_len(), 0);

        watch.inject(DiskEvent::at_clock(
            "/ws/a.rs",
            DiskEventKind::Modify,
            &clock,
        ));
        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
        assert!(!disk.is_empty());
        let modal = disk.first_pending().unwrap();
        assert_eq!(modal.path(), Path::new("/ws/a.rs"));
        assert_eq!(modal.mtime(), 1_000);
        assert_eq!(disk.pending_for("/ws/a.rs").unwrap().mtime(), 1_000);
        assert_eq!(disk.pending().len(), 1);

        watch.inject_modify("/ws/a.rs", 2_000);
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
        assert_eq!(disk.first_pending().unwrap().mtime(), 1_000);
    }

    #[test]
    fn disk_watch_observer_always_prompts_including_clean_buffer() {
        let (fs, buffers, mut watch, clock) = open_sample();
        let _ = fs;
        assert!(!buffers.get("/ws/a.rs").unwrap().is_dirty());
        let mut disk = DiskWatch::new();
        watch.inject(DiskEvent::at_clock(
            "/ws/a.rs",
            DiskEventKind::Modify,
            &clock,
        ));
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
        assert_eq!(
            disk.pending_for("/ws/a.rs").unwrap().path(),
            Path::new("/ws/a.rs")
        );
    }

    #[test]
    fn disk_watch_observer_ignores_events_for_closed_paths() {
        let (fs, mut buffers, mut watch, clock) = open_sample();
        let _ = fs;
        let mut disk = DiskWatch::new();
        watch.inject_modify("/ws/b.rs", clock.unix_ms());
        watch.inject_modify("/ws/missing.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert!(disk.is_empty());

        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
        buffers.close("/ws/a.rs");
        watch.inject_modify("/ws/a.rs", clock.unix_ms() + 1);
        disk.ingest(&mut watch, &buffers);
        assert!(disk.is_empty());
    }

    #[test]
    fn disk_watch_observer_keep_memory_does_not_requeue_same_generation() {
        let (mut fs, mut buffers, mut watch, clock) = open_sample();
        let mut clip = FakeClipboard::new();
        EditCommand::insert("local")
            .apply(buffers.get_mut("/ws/a.rs").unwrap(), &mut clip)
            .unwrap();
        fs.write(Path::new("/ws/a.rs"), b"other\n").unwrap();

        let mut disk = DiskWatch::new();
        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        disk.resolve("/ws/a.rs", ConflictChoice::KeepMemory, &mut buffers, &fs)
            .unwrap();
        assert!(disk.is_empty());
        assert_eq!(disk.ignored_mtime("/ws/a.rs"), Some(1_000));
        assert_eq!(buffers.get("/ws/a.rs").unwrap().text(), "localfn a() {}\n");
        assert!(buffers.get("/ws/a.rs").unwrap().is_dirty());

        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert!(disk.is_empty());

        clock.advance_ms(5);
        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
        assert_eq!(disk.first_pending().unwrap().mtime(), 1_005);
    }

    #[test]
    fn disk_watch_observer_load_disk_replaces_rope_and_clears_dirty() {
        let (mut fs, mut buffers, mut watch, clock) = open_sample();
        let mut clip = FakeClipboard::new();
        EditCommand::insert("stale")
            .apply(buffers.get_mut("/ws/a.rs").unwrap(), &mut clip)
            .unwrap();
        fs.write(Path::new("/ws/a.rs"), b"fresh\n").unwrap();

        let mut disk = DiskWatch::new();
        watch.inject(DiskEvent::create("/ws/a.rs", clock.unix_ms()));
        disk.ingest(&mut watch, &buffers);
        disk.resolve("/ws/a.rs", ConflictChoice::LoadDisk, &mut buffers, &fs)
            .unwrap();
        assert!(disk.is_empty());
        assert_eq!(disk.ignored_mtime("/ws/a.rs"), Some(1_000));
        let buf = buffers.get("/ws/a.rs").unwrap();
        assert_eq!(buf.text(), "fresh\n");
        assert!(!buf.is_dirty());

        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        assert!(disk.is_empty());
    }

    #[test]
    fn disk_watch_observer_load_disk_error_keeps_pending() {
        let (fs, mut buffers, mut watch, clock) = open_sample();
        let mut disk = DiskWatch::new();
        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        disk.ingest(&mut watch, &buffers);
        let err = disk
            .resolve(
                "/ws/a.rs",
                ConflictChoice::LoadDisk,
                &mut buffers,
                &MemFs::new(),
            )
            .unwrap_err();
        assert!(err.is_not_found());
        assert_eq!(disk.pending_len(), 1);
        assert!(disk.ignored_mtime("/ws/a.rs").is_none());
        disk.resolve("/missing.rs", ConflictChoice::KeepMemory, &mut buffers, &fs)
            .unwrap();
        assert_eq!(disk.pending_len(), 1);
        let _ = fs;
    }

    #[test]
    fn disk_watch_observer_two_open_paths_each_get_one_modal() {
        let (fs, mut buffers, mut watch, clock) = open_sample();
        buffers.open("/ws/b.rs", &fs).unwrap();
        let mut disk = DiskWatch::new();
        watch.inject_modify("/ws/a.rs", clock.unix_ms());
        watch.inject_modify("/ws/b.rs", clock.unix_ms() + 1);
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 2);
        assert_eq!(disk.pending_for("/ws/a.rs").unwrap().mtime(), 1_000);
        assert_eq!(disk.pending_for("/ws/b.rs").unwrap().mtime(), 1_001);
        disk.resolve("/ws/b.rs", ConflictChoice::KeepMemory, &mut buffers, &fs)
            .unwrap();
        assert_eq!(disk.pending_len(), 1);
        assert!(disk.pending_for("/ws/a.rs").is_some());
        assert_eq!(disk.ignored_mtime("/ws/b.rs"), Some(1_001));
    }

    #[test]
    fn disk_watch_observer_create_and_delete_also_prompt() {
        let (fs, buffers, mut watch, clock) = open_sample();
        let _ = fs;
        let mut disk = DiskWatch::new();
        watch.inject(DiskEvent::delete("/ws/a.rs", clock.unix_ms()));
        disk.ingest(&mut watch, &buffers);
        assert_eq!(disk.pending_len(), 1);
    }

    #[test]
    fn notify_watch_adapter_maps_kinds_without_os_api() {
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Create(CreateKind::File)),
            Some(DiskEventKind::Create)
        );
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Modify(ModifyKind::Name(
                notify::event::RenameMode::Any
            ))),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Remove(RemoveKind::File)),
            Some(DiskEventKind::Delete)
        );
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Any),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind(EventKind::Access(AccessKind::Any)),
            None
        );
        assert_eq!(NotifyWatch::map_kind(EventKind::Other), None);
        assert_eq!(
            NotifyWatch::map_kind_str("create"),
            Some(DiskEventKind::Create)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("Create"),
            Some(DiskEventKind::Create)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("any"),
            Some(DiskEventKind::Create)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("modify"),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("Modify"),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("data"),
            Some(DiskEventKind::Modify)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("remove"),
            Some(DiskEventKind::Delete)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("Remove"),
            Some(DiskEventKind::Delete)
        );
        assert_eq!(
            NotifyWatch::map_kind_str("delete"),
            Some(DiskEventKind::Delete)
        );
        assert_eq!(NotifyWatch::map_kind_str("unknown"), None);
        assert_eq!(NotifyWatch::map_kind_str(""), None);

        let event = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/ws/a.rs"))
            .add_path(PathBuf::from("/ws/b.rs"));
        let mapped = NotifyWatch::map_event(&event, 4);
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].path(), Path::new("/ws/a.rs"));
        assert_eq!(mapped[1].kind(), DiskEventKind::Modify);
        assert_eq!(mapped[1].mtime(), 4);

        let ignored = Event {
            kind: EventKind::Access(AccessKind::Any),
            paths: vec![PathBuf::from("/ws/a.rs")],
            attrs: EventAttributes::new(),
        };
        assert!(NotifyWatch::map_event(&ignored, 1).is_empty());
        assert!(NotifyWatch::map_event(&Event::new(EventKind::Other), 1).is_empty());
    }

    #[test]
    fn notify_watch_from_receiver_drains_channel_without_os() {
        let (tx, rx) = mpsc::channel();
        let mut watch = NotifyWatch::from_receiver(rx);
        assert_eq!(watch.queued_len(), 0);
        assert_eq!(watch.watched_len(), 0);
        assert!(watch.watch(Path::new("rel")).unwrap_err().is_not_absolute());
        watch.watch(Path::new("/ws")).unwrap();
        assert!(watch.is_watching("/ws"));
        assert_eq!(watch.watched_len(), 1);
        watch.unwatch(Path::new("/ws"));
        assert!(!watch.is_watching("/ws"));
        assert_eq!(watch.watched_len(), 0);

        tx.send(Ok(
            Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from("/ws/a.rs"))
        ))
        .unwrap();
        tx.send(Ok(Event::new(EventKind::Modify(ModifyKind::Data(
            DataChange::Any,
        )))
        .add_path(PathBuf::from("/ws/b.rs"))))
            .unwrap();
        tx.send(Ok(
            Event::new(EventKind::Remove(RemoveKind::File)).add_path(PathBuf::from("/ws/c.rs"))
        ))
        .unwrap();
        tx.send(Ok(
            Event::new(EventKind::Access(AccessKind::Any)).add_path(PathBuf::from("/ws/skip.rs"))
        ))
        .unwrap();
        tx.send(Err(notify::Error::generic("dropped"))).unwrap();

        let events = watch.poll();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind(), DiskEventKind::Create);
        assert_eq!(events[0].mtime(), 1);
        assert_eq!(events[1].kind(), DiskEventKind::Modify);
        assert_eq!(events[1].mtime(), 2);
        assert_eq!(events[2].kind(), DiskEventKind::Delete);
        assert_eq!(events[2].mtime(), 3);
        assert!(watch.poll().is_empty());

        drop(tx);
        assert!(watch.poll().is_empty());

        let mut mapped = NotifyWatch::new();
        assert_eq!(NotifyWatch::default().queued_len(), 0);
        mapped.push_mapped("/ws/a.rs", "modify", 9);
        mapped.push_mapped("/ws/z.rs", "nope", 1);
        mapped.push_mapped("/ws/c.rs", "delete", 8);
        assert_eq!(mapped.queued_len(), 2);
        let pushed = mapped.poll();
        assert_eq!(pushed.len(), 2);
        assert_eq!(pushed[0].mtime(), 9);
        assert_eq!(pushed[1].kind(), DiskEventKind::Delete);
        assert_eq!(mapped.queued_len(), 0);
    }
}
