//! Container launch Command + Event mailbox. The UI submits a launch and polls
//! journal snapshots. Tests pump [`FakeRuntime`]; they never call Docker.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::error::IdeError;
use crate::runtime::{run_launch, run_launch_reporting, DockerRuntime, LaunchJournal, RuntimePort};

/// Command the UI sends to the runtime worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeIoRequest {
    Launch { workspace: PathBuf },
}

impl RuntimeIoRequest {
    pub fn launch(workspace: impl AsRef<Path>) -> Self {
        Self::Launch {
            workspace: workspace.as_ref().to_path_buf(),
        }
    }

    pub fn workspace(&self) -> &Path {
        match self {
            Self::Launch { workspace } => workspace,
        }
    }

    pub fn is_launch(&self) -> bool {
        matches!(self, Self::Launch { .. })
    }
}

/// Journal snapshot after a launch attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeIoEvent {
    Progress { journal: LaunchJournal },
    Finished { journal: LaunchJournal },
}

impl RuntimeIoEvent {
    pub fn journal(&self) -> &LaunchJournal {
        match self {
            Self::Progress { journal } | Self::Finished { journal } => journal,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished { .. })
    }
}

/// Command queue + Event inbox. `submit` / `poll` never call Docker.
#[derive(Debug, Default)]
pub struct RuntimeIoMailbox {
    requests: VecDeque<RuntimeIoRequest>,
    events: VecDeque<RuntimeIoEvent>,
}

impl RuntimeIoMailbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&mut self, req: RuntimeIoRequest) {
        self.requests.push_back(req);
    }

    pub fn take_requests(&mut self) -> Vec<RuntimeIoRequest> {
        self.requests.drain(..).collect()
    }

    pub fn pending_len(&self) -> usize {
        self.requests.len()
    }

    pub fn push_event(&mut self, ev: RuntimeIoEvent) {
        self.events.push_back(ev);
    }

    pub fn poll(&mut self) -> Vec<RuntimeIoEvent> {
        self.events.drain(..).collect()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty() && self.events.is_empty()
    }
}

/// UI-facing channel ends.
#[derive(Debug)]
pub struct RuntimeIoHandle {
    tx: mpsc::Sender<RuntimeIoRequest>,
    rx: mpsc::Receiver<RuntimeIoEvent>,
}

impl RuntimeIoHandle {
    pub fn pair() -> (
        Self,
        mpsc::Receiver<RuntimeIoRequest>,
        mpsc::Sender<RuntimeIoEvent>,
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

    pub fn submit(&self, req: RuntimeIoRequest) -> Result<(), IdeError> {
        self.tx
            .send(req)
            .map_err(|_| IdeError::Io(std::io::Error::other("runtime io thread ended")))
    }

    pub fn poll(&self) -> Vec<RuntimeIoEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

/// Worker apply. Tests drive this with [`crate::runtime::FakeRuntime`].
pub fn dispatch_runtime_io(runtime: &dyn RuntimePort, req: RuntimeIoRequest) -> RuntimeIoEvent {
    match req {
        RuntimeIoRequest::Launch { workspace } => {
            let mut journal = LaunchJournal::container_plan();
            run_launch(runtime, &workspace, &mut journal);
            RuntimeIoEvent::Finished { journal }
        }
    }
}

pub fn pump_runtime_io(mailbox: &mut RuntimeIoMailbox, runtime: &dyn RuntimePort) {
    for req in mailbox.take_requests() {
        mailbox.push_event(dispatch_runtime_io(runtime, req));
    }
}

/// Named thread owns [`DockerRuntime`]. Tests use [`pump_runtime_io`] instead.
pub fn spawn_runtime_io() -> RuntimeIoHandle {
    let (handle, req_rx, ev_tx) = RuntimeIoHandle::pair();
    let _ = std::thread::Builder::new()
        .name("poc-ide-runtime".into())
        .spawn(move || run_runtime_io(req_rx, ev_tx));
    handle
}

fn run_runtime_io(req_rx: mpsc::Receiver<RuntimeIoRequest>, ev_tx: mpsc::Sender<RuntimeIoEvent>) {
    let runtime = DockerRuntime::new();
    while let Ok(req) = req_rx.recv() {
        match req {
            RuntimeIoRequest::Launch { workspace } => {
                let mut journal = LaunchJournal::container_plan();
                if ev_tx
                    .send(RuntimeIoEvent::Progress {
                        journal: journal.clone(),
                    })
                    .is_err()
                {
                    return;
                }
                run_launch_reporting(&runtime, &workspace, &mut journal, |j| {
                    let _ = ev_tx.send(RuntimeIoEvent::Progress { journal: j.clone() });
                });
                if ev_tx.send(RuntimeIoEvent::Finished { journal }).is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::FakeRuntime;
    use std::path::Path;

    #[test]
    fn runtime_io_mailbox_pumps_fake_without_docker() {
        assert!(RuntimeIoRequest::launch("/ws").is_launch());
        assert_eq!(
            RuntimeIoRequest::launch("/ws").workspace(),
            Path::new("/ws")
        );
        let mut box_ = RuntimeIoMailbox::new();
        assert!(box_.is_empty());
        box_.submit(RuntimeIoRequest::launch("/ws"));
        assert_eq!(box_.pending_len(), 1);
        pump_runtime_io(&mut box_, &FakeRuntime::ready());
        assert_eq!(box_.pending_len(), 0);
        assert_eq!(box_.event_len(), 1);
        let evs = box_.poll();
        assert!(evs[0].is_finished());
        assert!(evs[0].journal().all_ok());
        assert!(!RuntimeIoEvent::Progress {
            journal: LaunchJournal::container_plan(),
        }
        .is_finished());
        assert!(box_.is_empty());
        assert_eq!(RuntimeIoMailbox::default().event_len(), 0);

        let (handle, req_rx, ev_tx) = RuntimeIoHandle::pair();
        handle.submit(RuntimeIoRequest::launch("/proj")).unwrap();
        let req = req_rx.recv().unwrap();
        ev_tx
            .send(dispatch_runtime_io(&FakeRuntime::docker_missing(), req))
            .unwrap();
        let evs = handle.poll();
        assert_eq!(evs.len(), 1);
        assert!(evs[0].journal().is_failed());
        drop(req_rx);
        assert!(handle.submit(RuntimeIoRequest::launch("/x")).is_err());
    }
}
