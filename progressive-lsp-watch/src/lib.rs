//! Watch coalescer, backend port, filters, and FilesSince journal.

pub mod backend;
pub mod coalescer;
pub mod filter;
pub mod journal;

pub use backend::{FakeWatcher, NotifyWatcher, RawWatchEvent, WatchBackend, WatchKind};
pub use coalescer::WatchCoalescer;
pub use coalescer::{
    SharedCoalescer, DEFAULT_FILES_SINCE_LIMIT, DEFAULT_OVERFLOW_LIMIT, DEFAULT_WINDOW_MS,
};
pub use filter::{
    DefaultIgnoreFilter, DenyListFilter, IdentityWatchFilter, WatchFilter, DEFAULT_IGNORE_GLOBS,
    MANIFEST_NAMES,
};
pub use journal::{FilesSinceAnswer, FilesSinceJournal, FilesSinceQuery};

use progressive_lsp_control::WatchBatch as ProtoBatch;
use progressive_lsp_control::WatchEvent as ProtoEvent;

/// Domain watch event. Converts to the protobuf DTO at the control boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchEvent {
    pub path: String,
    pub kind: WatchKind,
}

impl WatchEvent {
    pub fn new(path: impl Into<String>, kind: WatchKind) -> Self {
        Self {
            path: path.into(),
            kind,
        }
    }

    pub fn to_proto(&self) -> ProtoEvent {
        ProtoEvent {
            path: self.path.clone(),
            kind: self.kind.as_str().to_string(),
        }
    }
}

/// Domain batch. Overflow / need_rescan must be set; never silently drop catch-up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchBatch {
    pub events: Vec<WatchEvent>,
    pub overflow: bool,
    pub need_rescan: bool,
    pub generation: u64,
}

impl WatchBatch {
    pub fn empty(generation: u64) -> Self {
        Self {
            events: Vec::new(),
            overflow: false,
            need_rescan: false,
            generation,
        }
    }

    pub fn to_proto(&self) -> ProtoBatch {
        ProtoBatch {
            events: self.events.iter().map(WatchEvent::to_proto).collect(),
            overflow: self.overflow,
            need_rescan: self.need_rescan,
            generation: self.generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_batch_maps_to_proto_without_dropping_flags() {
        let batch = WatchBatch {
            events: vec![WatchEvent::new("src/A.java", WatchKind::Modify)],
            overflow: true,
            need_rescan: true,
            generation: 9,
        };
        let proto = batch.to_proto();
        assert_eq!(proto.events.len(), 1);
        assert_eq!(proto.events[0].path, "src/A.java");
        assert_eq!(proto.events[0].kind, "modify");
        assert!(proto.overflow);
        assert!(proto.need_rescan);
        assert_eq!(proto.generation, 9);
        let empty = WatchBatch::empty(0);
        assert!(!empty.overflow);
        assert!(empty.events.is_empty());
        assert_eq!(empty.to_proto().generation, 0);
    }
}
