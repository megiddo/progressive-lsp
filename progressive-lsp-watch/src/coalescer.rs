//! Observer + Scheduler. N events in a ClockPort window become one `WatchBatch`.

use std::sync::Arc;

use std::sync::Mutex;

use progressive_lsp_control::{
    FilesSincePort, FilesSinceRequest, FilesSinceResponse, WatchBatch as ProtoBatch,
};
use progressive_lsp_core::{ClockPort, LogComponent, LogPort, LogScope, NullLog};

use crate::backend::{RawWatchEvent, WatchBackend, WatchKind};
use crate::journal::{FilesSinceAnswer, FilesSinceJournal, FilesSinceQuery};
use crate::{WatchBatch, WatchEvent};

/// Default coalesce window (milliseconds). Tests advance [`FakeClock`](progressive_lsp_core::FakeClock).
pub const DEFAULT_WINDOW_MS: u64 = 50;
/// Pending events above this set overflow / need_rescan and bump generation.
pub const DEFAULT_OVERFLOW_LIMIT: usize = 16_384;
/// FilesSince path bound.
pub const DEFAULT_FILES_SINCE_LIMIT: usize = 256;

/// Coalesces backend events using an injected clock. Never sleeps.
pub struct WatchCoalescer {
    clock: Arc<dyn ClockPort>,
    window_ms: u64,
    overflow_limit: usize,
    pending: Vec<WatchEvent>,
    window_opened_at_ms: Option<u64>,
    generation: u64,
    journal: FilesSinceJournal,
    last_batch: WatchBatch,
    log: Arc<dyn LogPort>,
}

impl WatchCoalescer {
    pub fn new(clock: Arc<dyn ClockPort>) -> Self {
        Self::with_limits(
            clock,
            DEFAULT_WINDOW_MS,
            DEFAULT_OVERFLOW_LIMIT,
            DEFAULT_FILES_SINCE_LIMIT,
        )
    }

    pub fn with_limits(
        clock: Arc<dyn ClockPort>,
        window_ms: u64,
        overflow_limit: usize,
        files_since_limit: usize,
    ) -> Self {
        Self {
            clock,
            window_ms: window_ms.max(1),
            overflow_limit: overflow_limit.max(1),
            pending: Vec::new(),
            window_opened_at_ms: None,
            generation: 0,
            journal: FilesSinceJournal::new(files_since_limit),
            last_batch: WatchBatch::empty(0),
            log: Arc::new(NullLog),
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    pub fn last_batch(&self) -> &WatchBatch {
        &self.last_batch
    }

    pub fn ingest(&mut self, events: impl IntoIterator<Item = WatchEvent>) {
        for ev in events {
            if self.pending.is_empty() {
                self.window_opened_at_ms = Some(self.clock.unix_ms());
            }
            if self.pending.len() >= self.overflow_limit {
                let _g = LogScope::enter(
                    LogScope::new()
                        .operation("watch")
                        .component(LogComponent::watch()),
                );
                self.log.warn("watch overflow; dropping events");
                self.force_overflow();
                return;
            }
            self.pending.push(ev);
        }
    }

    pub fn ingest_raw(&mut self, events: impl IntoIterator<Item = RawWatchEvent>) {
        self.ingest(events.into_iter().map(|e| WatchEvent::new(e.path, e.kind)));
    }

    pub fn poll_backend(&mut self, backend: &mut dyn WatchBackend) {
        let raw = backend.poll();
        self.ingest_raw(raw);
    }

    /// If the coalesce window has elapsed, emit one batch (all pending events).
    pub fn flush_due(&mut self) -> Option<WatchBatch> {
        let opened = self.window_opened_at_ms?;
        if self.pending.is_empty() {
            return None;
        }
        let elapsed = self.clock.unix_ms().saturating_sub(opened);
        if elapsed < self.window_ms {
            return None;
        }
        Some(self.flush_pending(false))
    }

    pub fn files_since_query(&self, q: Option<FilesSinceQuery>) -> FilesSinceAnswer {
        self.journal.query(q)
    }

    pub fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse {
        self.journal
            .query(FilesSinceQuery::from_request(req))
            .to_proto()
    }

    fn force_overflow(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.journal.mark_overflow(self.generation);
        let batch = WatchBatch {
            events: std::mem::take(&mut self.pending),
            overflow: true,
            need_rescan: true,
            generation: self.generation,
        };
        self.window_opened_at_ms = None;
        self.last_batch = batch;
    }

    fn flush_pending(&mut self, overflow: bool) -> WatchBatch {
        self.generation = self.generation.saturating_add(1);
        let now = self.clock.unix_ms();
        let events = std::mem::take(&mut self.pending);
        for ev in &events {
            self.journal.record(ev.path.clone(), self.generation, now);
        }
        if overflow {
            self.journal.mark_overflow(self.generation);
        }
        let batch = WatchBatch {
            events,
            overflow,
            need_rescan: overflow,
            generation: self.generation,
        };
        self.window_opened_at_ms = None;
        self.last_batch = batch.clone();
        batch
    }
}

/// Convenience: inject many FakeWatcher events and coalesce after advancing the clock.
pub fn coalesce_injected(
    coalescer: &mut WatchCoalescer,
    backend: &mut dyn WatchBackend,
) -> Option<WatchBatch> {
    coalescer.poll_backend(backend);
    coalescer.flush_due()
}

pub fn kind_from_str(s: &str) -> WatchKind {
    WatchKind::parse(s).unwrap_or(WatchKind::Modify)
}

/// Shared coalescer for the control port. Interior mutability; no sleep.
pub struct SharedCoalescer {
    inner: Mutex<WatchCoalescer>,
}

impl SharedCoalescer {
    pub fn new(coalescer: WatchCoalescer) -> Self {
        Self {
            inner: Mutex::new(coalescer),
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, WatchCoalescer> {
        self.inner.lock().expect("watch lock")
    }
}

impl FilesSincePort for SharedCoalescer {
    fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse {
        self.lock().files_since(req)
    }

    fn last_watch_batch(&self) -> ProtoBatch {
        self.lock().last_batch().to_proto()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FakeWatcher, WatchKind};
    use progressive_lsp_control::files_since_request;
    use progressive_lsp_core::FakeClock;
    use std::sync::Arc;

    fn setup(window: u64, overflow: usize) -> (Arc<FakeClock>, WatchCoalescer, FakeWatcher) {
        let clock = Arc::new(FakeClock::at_unix_ms(1_000));
        let c = WatchCoalescer::with_limits(clock.clone(), window, overflow, 64);
        (clock, c, FakeWatcher::new())
    }

    #[test]
    fn ten_thousand_events_become_one_batch() {
        let (clock, mut c, mut fake) = setup(50, 20_000);
        fake.start().unwrap();
        let started = std::time::Instant::now();
        for i in 0..10_000 {
            fake.inject_one(format!("f{i}.java"), WatchKind::Modify);
        }
        c.poll_backend(&mut fake);
        assert_eq!(c.pending_len(), 10_000);
        assert!(c.flush_due().is_none());
        clock.advance_ms(49);
        assert!(c.flush_due().is_none());
        clock.advance_ms(1);
        let batch = c.flush_due().expect("window opened");
        assert_eq!(batch.events.len(), 10_000);
        assert!(!batch.overflow);
        assert!(!batch.need_rescan);
        assert_eq!(batch.generation, 1);
        assert_eq!(c.generation(), 1);
        assert!(c.flush_due().is_none());
        assert_eq!(c.last_batch().events.len(), 10_000);
        assert_eq!(c.window_ms(), 50);
        let elapsed_us = started.elapsed().as_micros();
        assert!(
            elapsed_us < 2_000_000,
            "10k coalesce {elapsed_us}µs exceeds Darwin sample gate (2s)"
        );
    }

    #[test]
    fn second_window_is_a_new_generation() {
        let (clock, mut c, _) = setup(10, 100);
        c.ingest([WatchEvent::new("a", WatchKind::Create)]);
        clock.advance_ms(10);
        let first = c.flush_due().unwrap();
        assert_eq!(first.generation, 1);
        c.ingest([WatchEvent::new("b", WatchKind::Modify)]);
        clock.advance_ms(10);
        let second = c.flush_due().unwrap();
        assert_eq!(second.generation, 2);
        assert_eq!(second.events[0].path, "b");
        assert_eq!(c.generation(), 2);
    }

    #[test]
    fn overflow_bumps_generation_and_files_since_truncated() {
        let (clock, mut c, _) = setup(50, 2);
        c.ingest([
            WatchEvent::new("a", WatchKind::Create),
            WatchEvent::new("b", WatchKind::Modify),
            WatchEvent::new("c", WatchKind::Delete),
        ]);
        assert!(c.last_batch().overflow);
        assert!(c.last_batch().need_rescan);
        assert_eq!(c.generation(), 1);
        clock.advance_ms(50);
        assert!(c.flush_due().is_none());
        let fs = c.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(0)),
        });
        assert!(fs.truncated);
        assert_eq!(fs.generation, 1);
        let ans = c.files_since_query(Some(FilesSinceQuery::SinceGeneration(0)));
        assert!(ans.truncated);
        assert!(ans.overflow.is_some());
    }

    #[test]
    fn overflow_emits_log_scope_context_object() {
        let log = progressive_lsp_core::FakeLog::new();
        let clock = Arc::new(FakeClock::at_unix_ms(1_000));
        let mut c = WatchCoalescer::with_limits(clock, 50, 2, 64).with_log(Arc::new(log.clone()));
        c.ingest([
            WatchEvent::new("a", WatchKind::Create),
            WatchEvent::new("b", WatchKind::Modify),
            WatchEvent::new("c", WatchKind::Delete),
        ]);
        assert!(c.last_batch().overflow);
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("watch")
                    && r.message.contains("overflow")),
            "{:?}",
            log.records()
        );
    }

    #[test]
    fn files_since_after_successful_batch() {
        let (clock, mut c, _) = setup(5, 100);
        c.ingest([
            WatchEvent::new("a.java", WatchKind::Modify),
            WatchEvent::new("b.java", WatchKind::Create),
        ]);
        clock.advance_ms(5);
        let _ = c.flush_due();
        let fs = c.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(0)),
        });
        assert!(!fs.truncated);
        assert_eq!(fs.paths.len(), 2);
        assert!(fs.paths.contains(&"a.java".into()));
        assert_eq!(fs.generation, 1);
        let none = c.files_since(&FilesSinceRequest { since: None });
        assert_eq!(none.paths.len(), 2);
        let later = c.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceUnixMs(clock.unix_ms())),
        });
        assert!(later.paths.is_empty());
    }

    #[test]
    fn new_defaults_and_coalesce_injected() {
        let clock = Arc::new(FakeClock::at_unix_ms(0));
        let mut c = WatchCoalescer::new(clock.clone());
        assert_eq!(c.window_ms(), DEFAULT_WINDOW_MS);
        assert_eq!(c.generation(), 0);
        let mut fake = FakeWatcher::new();
        fake.inject_one("x", WatchKind::Modify);
        clock.advance_ms(DEFAULT_WINDOW_MS);
        // events ingested after advance still need a window open then another advance
        let none = coalesce_injected(&mut c, &mut fake);
        assert!(none.is_none());
        clock.advance_ms(DEFAULT_WINDOW_MS);
        let batch = c.flush_due().unwrap();
        assert_eq!(batch.events[0].path, "x");
        assert_eq!(kind_from_str("create"), WatchKind::Create);
        assert_eq!(kind_from_str("nope"), WatchKind::Modify);
    }

    #[test]
    fn limits_are_at_least_one() {
        let clock = Arc::new(FakeClock::at_unix_ms(0));
        let c = WatchCoalescer::with_limits(clock, 0, 0, 0);
        assert_eq!(c.window_ms(), 1);
    }

    #[test]
    fn empty_pending_does_not_flush() {
        let clock = Arc::new(FakeClock::at_unix_ms(0));
        let mut c = WatchCoalescer::new(clock);
        assert!(c.flush_due().is_none());
    }

    #[test]
    fn burst_10k_overflow_then_files_since_catch_up() {
        let (clock, mut c, mut fake) = setup(50, 4_096);
        fake.start().unwrap();
        for i in 0..10_000 {
            fake.inject_one(format!("burst{i}.java"), WatchKind::Modify);
        }
        let started = std::time::Instant::now();
        c.poll_backend(&mut fake);
        assert!(c.last_batch().overflow);
        assert!(c.last_batch().need_rescan);
        let fs = c.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(0)),
        });
        assert!(fs.truncated, "overflow must set FilesSince truncated");
        assert_eq!(fs.generation, 1);
        c.ingest([WatchEvent::new("caught-up.java", WatchKind::Modify)]);
        clock.advance_ms(50);
        let batch = c.flush_due().expect("catch-up window");
        assert!(!batch.overflow);
        assert_eq!(batch.events[0].path, "caught-up.java");
        let after = c.files_since(&FilesSinceRequest {
            since: Some(files_since_request::Since::SinceGeneration(
                batch.generation,
            )),
        });
        assert!(!after.truncated);
        assert!(after.paths.is_empty());
        let remaining = c.files_since(&FilesSinceRequest { since: None });
        assert!(remaining.paths.contains(&"caught-up.java".into()));
        let elapsed_us = started.elapsed().as_micros();
        assert!(
            elapsed_us < 5_000_000,
            "10k burst+catch-up {elapsed_us}µs exceeds Darwin sample gate (5s)"
        );
    }

    #[test]
    fn shared_coalescer_is_files_since_port() {
        use progressive_lsp_control::FilesSincePort;
        let clock = Arc::new(FakeClock::at_unix_ms(0));
        let mut c = WatchCoalescer::new(clock.clone());
        c.ingest([WatchEvent::new("z.java", WatchKind::Modify)]);
        clock.advance_ms(DEFAULT_WINDOW_MS);
        let _ = c.flush_due();
        let shared = SharedCoalescer::new(c);
        let fs = FilesSincePort::files_since(&shared, &FilesSinceRequest { since: None });
        assert!(fs.paths.contains(&"z.java".into()));
        assert_eq!(shared.last_watch_batch().events.len(), 1);
    }
}
