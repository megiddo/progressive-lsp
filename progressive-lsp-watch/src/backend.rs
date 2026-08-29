//! `WatchBackend` port. Production uses notify; tests use [`FakeWatcher`].

use std::sync::Mutex;

/// create / modify / delete. Overflow is a batch flag, not an event kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WatchKind {
    Create,
    Modify,
    Delete,
}

impl WatchKind {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawWatchEvent {
    pub path: String,
    pub kind: WatchKind,
}

impl RawWatchEvent {
    pub fn new(path: impl Into<String>, kind: WatchKind) -> Self {
        Self {
            path: path.into(),
            kind,
        }
    }
}

/// OS / test double. The coalescer does not call OS APIs directly.
pub trait WatchBackend: Send + Sync {
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self);
    fn poll(&mut self) -> Vec<RawWatchEvent>;
}

/// In-memory backend. Inject events; never sleeps.
#[derive(Debug, Default)]
pub struct FakeWatcher {
    started: bool,
    queue: Mutex<Vec<RawWatchEvent>>,
}

impl FakeWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inject(&self, events: impl IntoIterator<Item = RawWatchEvent>) {
        self.queue
            .lock()
            .expect("FakeWatcher queue")
            .extend(events);
    }

    pub fn inject_one(&self, path: impl Into<String>, kind: WatchKind) {
        self.inject([RawWatchEvent::new(path, kind)]);
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn queued_len(&self) -> usize {
        self.queue.lock().expect("FakeWatcher queue").len()
    }
}

impl WatchBackend for FakeWatcher {
    fn start(&mut self) -> Result<(), String> {
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.started = false;
    }

    fn poll(&mut self) -> Vec<RawWatchEvent> {
        std::mem::take(&mut *self.queue.lock().expect("FakeWatcher queue"))
    }
}

/// Production notify adapter. Event mapping is unit-tested; live OS loops are not.
#[derive(Debug, Default)]
pub struct NotifyWatcher {
    started: bool,
    pending: Vec<RawWatchEvent>,
}

impl NotifyWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Map a notify-style kind string without touching the OS.
    pub fn map_kind(kind: &str) -> Option<WatchKind> {
        match kind {
            "create" | "Create" | "any" => Some(WatchKind::Create),
            "modify" | "Modify" | "data" => Some(WatchKind::Modify),
            "remove" | "Remove" | "delete" => Some(WatchKind::Delete),
            _ => None,
        }
    }

    pub fn push_mapped(&mut self, path: impl Into<String>, kind: &str) {
        if let Some(k) = Self::map_kind(kind) {
            self.pending.push(RawWatchEvent::new(path, k));
        }
    }
}

impl WatchBackend for NotifyWatcher {
    fn start(&mut self) -> Result<(), String> {
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.started = false;
        self.pending.clear();
    }

    fn poll(&mut self) -> Vec<RawWatchEvent> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_kind_round_trip() {
        for (k, s) in [
            (WatchKind::Create, "create"),
            (WatchKind::Modify, "modify"),
            (WatchKind::Delete, "delete"),
        ] {
            assert_eq!(k.as_str(), s);
            assert_eq!(WatchKind::parse(s), Some(k));
        }
        assert_eq!(WatchKind::parse("CREATE"), None);
        assert_eq!(WatchKind::parse(""), None);
        assert_eq!(WatchKind::parse("overflow"), None);
    }

    #[test]
    fn fake_watcher_start_stop_inject_poll() {
        let mut w = FakeWatcher::new();
        assert!(!w.is_started());
        assert_eq!(w.queued_len(), 0);
        w.start().unwrap();
        assert!(w.is_started());
        w.inject_one("a.java", WatchKind::Create);
        w.inject([
            RawWatchEvent::new("b.java", WatchKind::Modify),
            RawWatchEvent::new("c.java", WatchKind::Delete),
        ]);
        assert_eq!(w.queued_len(), 3);
        let first = w.poll();
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].path, "a.java");
        assert_eq!(first[1].kind, WatchKind::Modify);
        assert_eq!(first[2].kind, WatchKind::Delete);
        assert!(w.poll().is_empty());
        assert_eq!(w.queued_len(), 0);
        w.stop();
        assert!(!w.is_started());
        w.inject_one("after-stop.java", WatchKind::Modify);
        assert_eq!(w.poll().len(), 1);
    }

    #[test]
    fn notify_watcher_maps_kinds_and_drops_unknown() {
        let mut n = NotifyWatcher::new();
        n.start().unwrap();
        n.push_mapped("a", "create");
        n.push_mapped("b", "Modify");
        n.push_mapped("c", "remove");
        n.push_mapped("d", "data");
        n.push_mapped("e", "unknown");
        n.push_mapped("f", "any");
        n.push_mapped("g", "delete");
        let ev = n.poll();
        assert_eq!(ev.len(), 6);
        assert_eq!(ev[0].kind, WatchKind::Create);
        assert_eq!(ev[1].kind, WatchKind::Modify);
        assert_eq!(ev[2].kind, WatchKind::Delete);
        assert_eq!(ev[3].kind, WatchKind::Modify);
        assert_eq!(ev[4].kind, WatchKind::Create);
        assert_eq!(ev[5].kind, WatchKind::Delete);
        assert!(n.poll().is_empty());
        n.push_mapped("z", "create");
        n.stop();
        assert!(n.poll().is_empty());
        assert_eq!(NotifyWatcher::map_kind("Create"), Some(WatchKind::Create));
        assert_eq!(NotifyWatcher::map_kind(""), None);
    }
}
