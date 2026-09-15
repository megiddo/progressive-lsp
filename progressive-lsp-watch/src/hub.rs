//! ADR 003: coalescing file-event hub on a dedicated thread.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use progressive_lsp_core::{ClockPort, SystemClock};

use crate::backend::{WatchBackend, WatchKind};
use crate::coalescer::WatchCoalescer;
use crate::{WatchBatch, WatchEvent};

/// Commands into the hub thread (disk backend + LSP buffer edits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubCommand {
    Publish(WatchEvent),
    Stop,
}

/// Receives coalesced [`WatchBatch`] deliveries.
pub trait HubSubscriber: Send + Sync {
    fn on_batch(&self, batch: &WatchBatch);
}

struct SubscriberList {
    subs: Vec<Arc<dyn HubSubscriber>>,
}

impl SubscriberList {
    fn dispatch(&self, batch: &WatchBatch) {
        for sub in &self.subs {
            sub.on_batch(batch);
        }
    }
}

/// Client handle: publish buffer edits and stop the hub.
#[derive(Clone, Debug)]
pub struct HubHandle {
    tx: SyncSender<HubCommand>,
}

impl HubHandle {
    pub fn publish(&self, event: WatchEvent) {
        let _ = self.tx.send(HubCommand::Publish(event));
    }

    pub fn publish_buffer_modify(&self, path: impl Into<String>) {
        self.publish(WatchEvent::buffer_modify(path));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(HubCommand::Stop);
    }
}

/// Coalescing pub/sub hub (one OS watch backend, many subscribers).
pub struct FileEventHub;

impl FileEventHub {
    /// Spawns the hub thread. Uses [`SystemClock`] and a real coalesce window (I/O scheduling only).
    pub fn start_system(
        mut backend: Box<dyn WatchBackend>,
        window_ms: u64,
        subscribers: Vec<Arc<dyn HubSubscriber>>,
    ) -> Result<(HubHandle, JoinHandle<()>), String> {
        backend.start()?;
        let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
        let (tx, rx) = mpsc::sync_channel::<HubCommand>(4096);
        let handle = HubHandle { tx: tx.clone() };
        let subs = Arc::new(SubscriberList { subs: subscribers });
        let join = thread::Builder::new()
            .name("file-event-hub".into())
            .spawn(move || {
                run_system_loop(&mut *backend, clock, window_ms, rx, subs);
                backend.stop();
            })
            .map_err(|e| e.to_string())?;
        Ok((handle, join))
    }

    /// Test / deterministic: advances [`FakeClock`] each tick — never sleeps.
    pub fn start_with_fake_clock(
        mut backend: Box<dyn WatchBackend>,
        clock: Arc<progressive_lsp_core::FakeClock>,
        window_ms: u64,
        subscribers: Vec<Arc<dyn HubSubscriber>>,
    ) -> Result<(HubHandle, JoinHandle<()>), String> {
        backend.start()?;
        let (tx, rx) = mpsc::sync_channel::<HubCommand>(4096);
        let handle = HubHandle { tx: tx.clone() };
        let subs = Arc::new(SubscriberList { subs: subscribers });
        let join = thread::Builder::new()
            .name("file-event-hub".into())
            .spawn(move || {
                run_fake_clock_loop(&mut *backend, clock, window_ms, rx, subs);
                backend.stop();
            })
            .map_err(|e| e.to_string())?;
        Ok((handle, join))
    }

    /// Run one hub iteration synchronously (unit tests with [`FakeClock`](progressive_lsp_core::FakeClock)).
    pub fn drain_once(
        backend: &mut dyn WatchBackend,
        coalescer: &mut WatchCoalescer,
        rx: &Receiver<HubCommand>,
        subs: &SubscriberList,
    ) {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                HubCommand::Publish(ev) => coalescer.ingest([ev]),
                HubCommand::Stop => return,
            }
        }
        coalescer.poll_backend(backend);
        if let Some(batch) = coalescer.flush_now() {
            subs.dispatch(&batch);
        }
    }
}

fn run_system_loop(
    backend: &mut dyn WatchBackend,
    clock: Arc<dyn ClockPort>,
    window_ms: u64,
    rx: Receiver<HubCommand>,
    subs: Arc<SubscriberList>,
) {
    let mut coalescer = WatchCoalescer::with_limits(
        clock,
        window_ms.max(1),
        crate::coalescer::DEFAULT_OVERFLOW_LIMIT,
        crate::coalescer::DEFAULT_FILES_SINCE_LIMIT,
    );
    let wait = Duration::from_millis(window_ms.max(1));
    loop {
        match rx.recv_timeout(wait) {
            Ok(HubCommand::Publish(ev)) => coalescer.ingest([ev]),
            Ok(HubCommand::Stop) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                HubCommand::Publish(ev) => coalescer.ingest([ev]),
                HubCommand::Stop => return,
            }
        }
        coalescer.poll_backend(backend);
        if let Some(batch) = coalescer
            .flush_due()
            .or_else(|| coalescer.flush_now())
        {
            subs.dispatch(&batch);
        }
    }
}

fn run_fake_clock_loop(
    backend: &mut dyn WatchBackend,
    clock: Arc<progressive_lsp_core::FakeClock>,
    window_ms: u64,
    rx: Receiver<HubCommand>,
    subs: Arc<SubscriberList>,
) {
    let mut coalescer = WatchCoalescer::with_limits(
        clock.clone(),
        window_ms.max(1),
        crate::coalescer::DEFAULT_OVERFLOW_LIMIT,
        crate::coalescer::DEFAULT_FILES_SINCE_LIMIT,
    );
    loop {
        let mut stop = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                HubCommand::Publish(ev) => coalescer.ingest([ev]),
                HubCommand::Stop => stop = true,
            }
        }
        coalescer.poll_backend(backend);
        clock.advance_ms(window_ms.max(1));
        if let Some(batch) = coalescer
            .flush_due()
            .or_else(|| coalescer.flush_now())
        {
            subs.dispatch(&batch);
        }
        if stop {
            break;
        }
        if rx.try_recv().is_err() && coalescer.pending_len() == 0 {
            thread::yield_now();
        }
    }
}

/// Wrap a closure as a subscriber.
pub struct CallbackSubscriber {
    f: Arc<dyn Fn(&WatchBatch) + Send + Sync>,
}

impl CallbackSubscriber {
    pub fn new(f: impl Fn(&WatchBatch) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { f: Arc::new(f) })
    }
}

impl HubSubscriber for CallbackSubscriber {
    fn on_batch(&self, batch: &WatchBatch) {
        (self.f)(batch);
    }
}

/// Records batches for tests.
#[derive(Debug, Default)]
pub struct RecordingSubscriber {
    batches: Mutex<Vec<WatchBatch>>,
}

impl RecordingSubscriber {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_batches(&self) -> Vec<WatchBatch> {
        std::mem::take(&mut *self.batches.lock().expect("batches"))
    }
}

impl HubSubscriber for RecordingSubscriber {
    fn on_batch(&self, batch: &WatchBatch) {
        self.batches.lock().expect("batches").push(batch.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FakeWatcher, RawWatchEvent, WatchKind};
    use progressive_lsp_core::FakeClock;

    #[test]
    fn fake_watcher_produces_one_batch_and_generation_bump() {
        let clock = Arc::new(FakeClock::at_unix_ms(1_000));
        let fake = FakeWatcher::new();
        fake.inject_one("src/A.java", WatchKind::Modify);
        let rec = Arc::new(RecordingSubscriber::new());
        let (handle, join) = FileEventHub::start_with_fake_clock(
            Box::new(fake),
            clock.clone(),
            10,
            vec![rec.clone()],
        )
        .unwrap();
        assert!(handle.tx.send(HubCommand::Stop).is_ok());
        join.join().unwrap();
        let batches = rec.take_batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].generation, 1);
        assert_eq!(batches[0].events.len(), 1);
        assert_eq!(batches[0].events[0].path, "src/A.java");
    }

    #[test]
    fn buffer_and_disk_coalesce_in_one_batch() {
        let clock = Arc::new(FakeClock::at_unix_ms(0));
        let mut fake = FakeWatcher::new();
        fake.start().unwrap();
        fake.inject([RawWatchEvent::new("a.java", WatchKind::Modify)]);
        let rec = Arc::new(RecordingSubscriber::new());
        let (tx, rx) = mpsc::channel();
        let subs = Arc::new(SubscriberList {
            subs: vec![rec.clone()],
        });
        let mut coalescer = WatchCoalescer::with_limits(clock.clone(), 10, 100, 64);
        tx.send(HubCommand::Publish(WatchEvent::buffer_modify("b.java")))
            .unwrap();
        FileEventHub::drain_once(&mut fake, &mut coalescer, &rx, &subs);
        clock.advance_ms(10);
        FileEventHub::drain_once(&mut fake, &mut coalescer, &rx, &subs);
        let batches = rec.take_batches();
        assert!(
            batches.iter().any(|b| b.events.len() >= 2),
            "{batches:?}"
        );
        assert!(batches.iter().any(|b| b.generation >= 1));
    }
}
