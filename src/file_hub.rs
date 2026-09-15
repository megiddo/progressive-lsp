//! ADR 003 hub slot on [`crate::ServeHost`] (wire deliver from `lib.rs` after `Arc`).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use progressive_lsp_watch::{
    CallbackSubscriber, FileEventHub, HubHandle, NotifyWatcher, WatchBatch as DomainWatchBatch,
};

const HUB_COALESCE_MS: u64 = 50;

pub struct FileHubSlot {
    inner: Mutex<FileHubState>,
}

struct FileHubState {
    handle: Option<HubHandle>,
    join: Option<JoinHandle<()>>,
    deliver: Option<Arc<dyn Fn(DomainWatchBatch) + Send + Sync>>,
    need_rescan: bool,
}

impl Default for FileHubSlot {
    fn default() -> Self {
        Self {
            inner: Mutex::new(FileHubState {
                handle: None,
                join: None,
                deliver: None,
                need_rescan: false,
            }),
        }
    }
}

impl FileHubSlot {
    pub fn wire_deliver(&self, deliver: Arc<dyn Fn(DomainWatchBatch) + Send + Sync>) {
        self.inner.lock().expect("file hub").deliver = Some(deliver);
    }

    pub fn is_running(&self) -> bool {
        self.inner
            .lock()
            .expect("file hub")
            .handle
            .is_some()
    }

    pub fn need_rescan(&self) -> bool {
        self.inner.lock().expect("file hub").need_rescan
    }

    pub fn clear_need_rescan(&self) {
        self.inner.lock().expect("file hub").need_rescan = false;
    }

    pub fn note_batch_flags(&self, batch: &DomainWatchBatch) {
        if batch.need_rescan || batch.overflow {
            self.inner.lock().expect("file hub").need_rescan = true;
        }
    }

    /// Start once per workspace. `NotifyWatcher` is the production adapter (Linux dogfood);
    /// Darwin CI relies on unit fakes — full inotify parity is platform-scoped (ABS-2.8).
    pub fn try_start(&self, _root: &Path) -> Result<(), String> {
        let mut st = self.inner.lock().expect("file hub");
        if st.handle.is_some() {
            return Ok(());
        }
        let deliver = st
            .deliver
            .clone()
            .ok_or_else(|| "file hub deliver not wired".to_string())?;
        let sub = CallbackSubscriber::new(move |batch| deliver(batch.clone()));
        let (handle, join) =
            FileEventHub::start_system(Box::new(NotifyWatcher::new()), HUB_COALESCE_MS, vec![sub])?;
        st.handle = Some(handle);
        st.join = Some(join);
        Ok(())
    }

    pub fn publish_buffer_modify(&self, path: &str) {
        if let Some(h) = self.inner.lock().expect("file hub").handle.as_ref() {
            h.publish_buffer_modify(path.to_string());
        }
    }

    pub fn stop(&self) {
        let mut st = self.inner.lock().expect("file hub");
        if let Some(h) = st.handle.take() {
            h.stop();
        }
        if let Some(j) = st.join.take() {
            let _ = j.join();
        }
    }
}
