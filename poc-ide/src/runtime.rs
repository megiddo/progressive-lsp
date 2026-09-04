//! Linux runtime Port for container open. Tests use [`FakeRuntime`]; they never
//! talk to a Docker daemon, registry, or AWS.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::IdeError;
use crate::language::WireTier;
use crate::open_mode::OpenMode;

/// Image the container host would run. Not pulled in unit tests.
pub const RUNTIME_IMAGE: &str = "progressive-lsp-runtime:local";

/// One launch / preflight step. Value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Running,
    Ok,
    Fail,
    Skipped,
}

impl StepState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Fail => "fail",
            Self::Skipped => "skipped",
        }
    }
}

/// Ordered step in a T1/T2/T3 or container-launch journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchStep {
    id: String,
    label: String,
    state: StepState,
    detail: Option<String>,
}

impl LaunchStep {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state: StepState::Pending,
            detail: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn state(&self) -> StepState {
        self.state
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    pub fn with_state(mut self, state: StepState) -> Self {
        self.state = state;
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let d = detail.into();
        if !d.is_empty() {
            self.detail = Some(d);
        }
        self
    }
}

/// Append-only step list for the status modal. Not a Manager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchJournal {
    steps: Vec<LaunchStep>,
}

impl LaunchJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn container_plan() -> Self {
        Self {
            steps: vec![
                LaunchStep::new("docker_probe", "Docker available"),
                LaunchStep::new("platform", "Linux pack platform"),
                LaunchStep::new("image", "Runtime image present"),
                LaunchStep::new("mount", "Workspace mount"),
                LaunchStep::new("start_serve", "Start progressive-lsp in container"),
                LaunchStep::new("t3_preflight", "T3 pack preflight"),
            ],
        }
    }

    pub fn native_t3_skipped() -> Self {
        Self {
            steps: vec![LaunchStep::new("t3_host", "T3 Linux host")
                .with_state(StepState::Skipped)
                .with_detail("Open Folder in Container for T1/T2/T3 on one Linux serve")],
        }
    }

    /// Native Linux T3: one serve, no Docker plan.
    pub fn native_t3_from_wire(
        current: Option<WireTier>,
        ingest: progressive_lsp_control::IngestState,
    ) -> Self {
        use progressive_lsp_control::IngestState;
        let (state, detail) = match current {
            Some(WireTier::Types) => (StepState::Ok, "types"),
            _ if ingest == IngestState::Running => (StepState::Pending, "waiting for T1/T2"),
            _ if ingest == IngestState::Done => (StepState::Skipped, "T3 skipped (stub pack)"),
            _ => (StepState::Pending, "not started"),
        };
        Self {
            steps: vec![LaunchStep::new("t3_engine", "T3 types")
                .with_state(state)
                .with_detail(detail)],
        }
    }

    pub fn t1_from_ingest(ingest: progressive_lsp_control::IngestState) -> Self {
        use progressive_lsp_control::IngestState;
        let (discover, parse) = match ingest {
            IngestState::NotStarted => (StepState::Pending, StepState::Pending),
            IngestState::Running => (StepState::Ok, StepState::Running),
            IngestState::Done => (StepState::Ok, StepState::Ok),
        };
        Self {
            steps: vec![
                LaunchStep::new("discover", "Discover workspace").with_state(discover),
                LaunchStep::new("t1_ingest", "T1 syntax ingest").with_state(parse),
            ],
        }
    }

    pub fn t2_from_ingest(ingest: progressive_lsp_control::IngestState) -> Self {
        use progressive_lsp_control::IngestState;
        let state = match ingest {
            IngestState::NotStarted => StepState::Pending,
            IngestState::Running => StepState::Running,
            IngestState::Done => StepState::Ok,
        };
        Self {
            steps: vec![LaunchStep::new("t2_graph", "T2 graph ingest").with_state(state)],
        }
    }

    pub fn steps(&self) -> &[LaunchStep] {
        &self.steps
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_failed(&self) -> bool {
        self.steps.iter().any(|s| s.state == StepState::Fail)
    }

    pub fn is_running(&self) -> bool {
        self.steps.iter().any(|s| s.state == StepState::Running)
    }

    pub fn all_ok(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.state == StepState::Ok)
    }

    fn set(&mut self, id: &str, state: StepState, detail: Option<String>) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == id) {
            step.state = state;
            if let Some(d) = detail {
                if !d.is_empty() {
                    step.detail = Some(d);
                }
            }
        }
    }

    pub fn start(&mut self, id: &str) {
        self.set(id, StepState::Running, None);
    }

    pub fn ok(&mut self, id: &str, detail: impl Into<String>) {
        self.set(id, StepState::Ok, Some(detail.into()));
    }

    pub fn fail(&mut self, id: &str, detail: impl Into<String>) {
        self.set(id, StepState::Fail, Some(detail.into()));
    }

    pub fn skip(&mut self, id: &str, detail: impl Into<String>) {
        self.set(id, StepState::Skipped, Some(detail.into()));
    }
}

/// Which strip cell opened the status modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusModalKind {
    T1,
    T2,
    T3,
    Container,
}

impl StatusModalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::T3 => "T3",
            Self::Container => "container",
        }
    }
}

/// Open/closed status modal. Value object; `ui.rs` renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusModal {
    kind: Option<StatusModalKind>,
}

impl StatusModal {
    pub fn closed() -> Self {
        Self { kind: None }
    }

    pub fn open(kind: StatusModalKind) -> Self {
        Self { kind: Some(kind) }
    }

    pub fn is_open(&self) -> bool {
        self.kind.is_some()
    }

    pub fn kind(&self) -> Option<StatusModalKind> {
        self.kind
    }

    pub fn close(&mut self) {
        self.kind = None;
    }
}

/// Probe result. Never includes registry URLs in tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInfo {
    available: bool,
    platform: String,
}

impl RuntimeInfo {
    pub fn new(available: bool, platform: impl Into<String>) -> Self {
        Self {
            available,
            platform: platform.into(),
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn is_linux_pack_platform(&self) -> bool {
        matches!(
            self.platform.as_str(),
            "linux/arm64" | "linux/amd64" | "linux/aarch64" | "linux/x86_64"
        )
    }
}

/// Started container session. Tests never hold a live Docker id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSession {
    workspace: PathBuf,
}

impl RuntimeSession {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

/// How poc-ide starts a Linux serve host. Production is Docker; tests inject
/// [`FakeRuntime`].
pub trait RuntimePort {
    fn probe(&self) -> Result<RuntimeInfo, IdeError>;
    fn ensure_image(&self, image: &str) -> Result<(), IdeError>;
    fn start(&self, workspace: &Path) -> Result<RuntimeSession, IdeError>;
    fn preflight_t3(&self) -> Result<(), IdeError>;
}

/// Scripted runtime. No daemon, no network.
#[derive(Clone, Debug)]
pub struct FakeRuntime {
    info: Result<RuntimeInfo, String>,
    image: Result<(), String>,
    start: Result<(), String>,
    preflight: Result<(), String>,
}

impl FakeRuntime {
    pub fn ready() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(true, "linux/arm64")),
            image: Ok(()),
            start: Ok(()),
            preflight: Ok(()),
        }
    }

    pub fn docker_missing() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(false, "")),
            image: Err("no image".into()),
            start: Err("no start".into()),
            preflight: Err("no preflight".into()),
        }
    }

    pub fn probe_err(msg: impl Into<String>) -> Self {
        Self {
            info: Err(msg.into()),
            image: Err("skipped".into()),
            start: Err("skipped".into()),
            preflight: Err("skipped".into()),
        }
    }

    pub fn bad_platform() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(true, "darwin/arm64")),
            image: Ok(()),
            start: Ok(()),
            preflight: Ok(()),
        }
    }

    pub fn image_missing() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(true, "linux/amd64")),
            image: Err("image not present".into()),
            start: Err("skipped".into()),
            preflight: Err("skipped".into()),
        }
    }

    pub fn start_fails() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(true, "linux/arm64")),
            image: Ok(()),
            start: Err("container serve attach not wired".into()),
            preflight: Err("skipped".into()),
        }
    }

    pub fn preflight_fails() -> Self {
        Self {
            info: Ok(RuntimeInfo::new(true, "linux/arm64")),
            image: Ok(()),
            start: Ok(()),
            preflight: Err("stub pack".into()),
        }
    }
}

impl RuntimePort for FakeRuntime {
    fn probe(&self) -> Result<RuntimeInfo, IdeError> {
        self.info.clone().map_err(IdeError::runtime)
    }

    fn ensure_image(&self, _image: &str) -> Result<(), IdeError> {
        self.image.clone().map_err(IdeError::runtime)
    }

    fn start(&self, workspace: &Path) -> Result<RuntimeSession, IdeError> {
        self.start
            .clone()
            .map(|()| RuntimeSession::new(workspace))
            .map_err(IdeError::runtime)
    }

    fn preflight_t3(&self) -> Result<(), IdeError> {
        self.preflight.clone().map_err(IdeError::runtime)
    }
}

/// `docker` CLI Adapter. Tests construct with a missing binary; they do not
/// start a daemon.
pub struct DockerRuntime {
    docker: PathBuf,
}

impl DockerRuntime {
    pub fn new() -> Self {
        Self {
            docker: PathBuf::from("docker"),
        }
    }

    pub fn from_binary(docker: impl Into<PathBuf>) -> Self {
        Self {
            docker: docker.into(),
        }
    }
}

impl Default for DockerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimePort for DockerRuntime {
    fn probe(&self) -> Result<RuntimeInfo, IdeError> {
        let out = Command::new(&self.docker)
            .args(["version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"])
            .output()
            .map_err(|e| IdeError::runtime(format!("docker probe: {e}")))?;
        if !out.status.success() {
            return Ok(RuntimeInfo::new(false, ""));
        }
        let platform = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if platform.is_empty() {
            return Ok(RuntimeInfo::new(false, ""));
        }
        Ok(RuntimeInfo::new(true, platform))
    }

    fn ensure_image(&self, image: &str) -> Result<(), IdeError> {
        let out = Command::new(&self.docker)
            .args(["image", "inspect", image])
            .output()
            .map_err(|e| IdeError::runtime(format!("docker image inspect: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(IdeError::runtime(format!(
                "{image} not present (not pulled in tests)"
            )))
        }
    }

    fn start(&self, _workspace: &Path) -> Result<RuntimeSession, IdeError> {
        Err(IdeError::runtime(
            "container serve attach not wired; one Linux serve per workspace",
        ))
    }

    fn preflight_t3(&self) -> Result<(), IdeError> {
        Err(IdeError::runtime("T3 preflight waits for container serve"))
    }
}

/// Run the container plan against a [`RuntimePort`]. Tests pass [`FakeRuntime`].
pub fn run_launch(runtime: &dyn RuntimePort, workspace: &Path, journal: &mut LaunchJournal) {
    run_launch_reporting(runtime, workspace, journal, |_| {});
}

/// Same as [`run_launch`], but reports the journal after each step so the
/// status modal can paint `running` while Docker work is in flight.
pub fn run_launch_reporting(
    runtime: &dyn RuntimePort,
    workspace: &Path,
    journal: &mut LaunchJournal,
    mut report: impl FnMut(&LaunchJournal),
) {
    if journal.steps().is_empty() {
        *journal = LaunchJournal::container_plan();
        report(journal);
    }
    journal.start("docker_probe");
    report(journal);
    let info = match runtime.probe() {
        Ok(info) => info,
        Err(e) => {
            journal.fail("docker_probe", e.to_string());
            skip_rest(journal, "docker_probe");
            report(journal);
            return;
        }
    };
    if !info.available() {
        journal.fail("docker_probe", "Docker not available");
        skip_rest(journal, "docker_probe");
        report(journal);
        return;
    }
    journal.ok("docker_probe", info.platform());
    report(journal);

    journal.start("platform");
    report(journal);
    if !info.is_linux_pack_platform() {
        journal.fail(
            "platform",
            format!("need linux/arm64 or linux/amd64, got {}", info.platform()),
        );
        skip_rest(journal, "platform");
        report(journal);
        return;
    }
    journal.ok("platform", info.platform());
    report(journal);

    journal.start("image");
    report(journal);
    match runtime.ensure_image(RUNTIME_IMAGE) {
        Ok(()) => {
            journal.ok("image", RUNTIME_IMAGE);
            report(journal);
        }
        Err(e) => {
            journal.fail("image", e.to_string());
            skip_rest(journal, "image");
            report(journal);
            return;
        }
    }

    journal.start("mount");
    report(journal);
    journal.ok("mount", workspace.display().to_string());
    report(journal);

    journal.start("start_serve");
    report(journal);
    match runtime.start(workspace) {
        Ok(_) => {
            journal.ok("start_serve", OpenMode::Container.as_str());
            report(journal);
        }
        Err(e) => {
            journal.fail("start_serve", e.to_string());
            skip_rest(journal, "start_serve");
            report(journal);
            return;
        }
    }

    journal.start("t3_preflight");
    report(journal);
    match runtime.preflight_t3() {
        Ok(()) => journal.ok("t3_preflight", "packs ready"),
        Err(e) => journal.fail("t3_preflight", e.to_string()),
    }
    report(journal);
}

fn skip_rest(journal: &mut LaunchJournal, after: &str) {
    let mut skip = false;
    for step in &mut journal.steps {
        if skip && step.state == StepState::Pending {
            step.state = StepState::Skipped;
        }
        if step.id == after {
            skip = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_control::IngestState;
    use std::path::Path;

    #[test]
    fn launch_journal_and_status_modal_value_objects() {
        assert_eq!(StepState::Pending.as_str(), "pending");
        assert_eq!(StepState::Running.as_str(), "running");
        assert_eq!(StepState::Ok.as_str(), "ok");
        assert_eq!(StepState::Fail.as_str(), "fail");
        assert_eq!(StepState::Skipped.as_str(), "skipped");
        let step = LaunchStep::new("id", "label")
            .with_state(StepState::Ok)
            .with_detail("d");
        assert_eq!(step.id(), "id");
        assert_eq!(step.label(), "label");
        assert_eq!(step.state(), StepState::Ok);
        assert_eq!(step.detail(), Some("d"));
        assert!(LaunchStep::new("x", "y").with_detail("").detail().is_none());

        let plan = LaunchJournal::container_plan();
        assert_eq!(plan.len(), 6);
        assert!(!plan.is_empty());
        assert!(!plan.is_failed());
        assert!(!plan.all_ok());
        assert_eq!(LaunchJournal::new().len(), 0);
        assert!(LaunchJournal::new().is_empty());

        let skipped = LaunchJournal::native_t3_skipped();
        assert_eq!(skipped.steps()[0].state(), StepState::Skipped);
        assert!(skipped.steps()[0].detail().unwrap().contains("Container"));

        let t1 = LaunchJournal::t1_from_ingest(IngestState::Running);
        assert_eq!(t1.steps()[0].state(), StepState::Ok);
        assert_eq!(t1.steps()[1].state(), StepState::Running);
        assert!(LaunchJournal::t1_from_ingest(IngestState::NotStarted)
            .steps()
            .iter()
            .all(|s| s.state() == StepState::Pending));
        assert!(LaunchJournal::t1_from_ingest(IngestState::Done)
            .steps()
            .iter()
            .all(|s| s.state() == StepState::Ok));
        assert_eq!(
            LaunchJournal::t2_from_ingest(IngestState::Done).steps()[0].state(),
            StepState::Ok
        );
        assert_eq!(
            LaunchJournal::t2_from_ingest(IngestState::NotStarted).steps()[0].state(),
            StepState::Pending
        );
        assert_eq!(
            LaunchJournal::t2_from_ingest(IngestState::Running).steps()[0].state(),
            StepState::Running
        );

        let mut modal = StatusModal::closed();
        assert!(!modal.is_open());
        assert!(modal.kind().is_none());
        modal = StatusModal::open(StatusModalKind::T3);
        assert_eq!(modal.kind(), Some(StatusModalKind::T3));
        assert_eq!(StatusModalKind::T1.as_str(), "T1");
        assert_eq!(StatusModalKind::T2.as_str(), "T2");
        assert_eq!(StatusModalKind::T3.as_str(), "T3");
        assert_eq!(StatusModalKind::Container.as_str(), "container");
        modal.close();
        assert!(!modal.is_open());
        assert_ne!(StatusModalKind::T1, StatusModalKind::T3);
        assert_ne!(StatusModalKind::Container, StatusModalKind::T3);

        let native_ok =
            LaunchJournal::native_t3_from_wire(Some(WireTier::Types), IngestState::Done);
        assert_eq!(native_ok.steps()[0].state(), StepState::Ok);
        let native_skip =
            LaunchJournal::native_t3_from_wire(Some(WireTier::Syntax), IngestState::Done);
        assert_eq!(native_skip.steps()[0].state(), StepState::Skipped);
        let native_wait = LaunchJournal::native_t3_from_wire(None, IngestState::Running);
        assert_eq!(native_wait.steps()[0].state(), StepState::Pending);
    }

    #[test]
    fn run_launch_fake_runtime_paths_never_touch_docker() {
        let ws = Path::new("/ws");
        let mut journal = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::ready(), ws, &mut journal);
        assert!(journal.all_ok(), "{:?}", journal.steps());
        assert_eq!(journal.steps()[2].detail(), Some(RUNTIME_IMAGE));

        let mut empty = LaunchJournal::new();
        run_launch(&FakeRuntime::ready(), ws, &mut empty);
        assert!(empty.all_ok());

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::docker_missing(), ws, &mut j);
        assert!(j.is_failed());
        assert_eq!(j.steps()[0].state(), StepState::Fail);
        assert_eq!(j.steps()[1].state(), StepState::Skipped);

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::probe_err("boom"), ws, &mut j);
        assert_eq!(j.steps()[0].state(), StepState::Fail);
        assert!(j.steps()[0].detail().unwrap().contains("boom"));

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::bad_platform(), ws, &mut j);
        assert_eq!(j.steps()[1].state(), StepState::Fail);
        assert!(j.steps()[1].detail().unwrap().contains("darwin/arm64"));

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::image_missing(), ws, &mut j);
        assert_eq!(j.steps()[2].state(), StepState::Fail);
        assert_eq!(j.steps()[3].state(), StepState::Skipped);

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::start_fails(), ws, &mut j);
        assert_eq!(j.steps()[4].state(), StepState::Fail);
        assert_eq!(j.steps()[5].state(), StepState::Skipped);

        let mut j = LaunchJournal::container_plan();
        run_launch(&FakeRuntime::preflight_fails(), ws, &mut j);
        assert_eq!(j.steps()[5].state(), StepState::Fail);
        assert!(j.is_failed());
        assert!(!j.is_running());
        j.start("t3_preflight");
        assert!(j.is_running());
        j.skip("t3_preflight", "later");
        assert_eq!(j.steps()[5].state(), StepState::Skipped);

        let mut reports = 0usize;
        let mut live = LaunchJournal::container_plan();
        run_launch_reporting(&FakeRuntime::ready(), ws, &mut live, |_| {
            reports += 1;
        });
        assert!(live.all_ok());
        assert!(reports >= 6);
    }

    #[test]
    fn runtime_info_linux_pack_platforms_and_session() {
        assert!(RuntimeInfo::new(true, "linux/arm64").is_linux_pack_platform());
        assert!(RuntimeInfo::new(true, "linux/amd64").is_linux_pack_platform());
        assert!(RuntimeInfo::new(true, "linux/aarch64").is_linux_pack_platform());
        assert!(RuntimeInfo::new(true, "linux/x86_64").is_linux_pack_platform());
        assert!(!RuntimeInfo::new(true, "darwin/arm64").is_linux_pack_platform());
        let info = RuntimeInfo::new(true, "linux/arm64");
        assert!(info.available());
        assert_eq!(info.platform(), "linux/arm64");
        let session = RuntimeSession::new("/ws");
        assert_eq!(session.workspace(), Path::new("/ws"));
        assert_eq!(RUNTIME_IMAGE, "progressive-lsp-runtime:local");
    }

    #[test]
    fn docker_runtime_missing_binary_fails_probe_without_daemon() {
        let rt = DockerRuntime::from_binary("/no/such/docker-binary");
        let err = rt.probe().unwrap_err();
        assert!(err.is_runtime());
        assert!(err.to_string().contains("docker probe"));
        let img = rt.ensure_image(RUNTIME_IMAGE).unwrap_err();
        assert!(img.is_runtime());
        assert!(rt.start(Path::new("/ws")).unwrap_err().is_runtime());
        assert!(rt.preflight_t3().unwrap_err().is_runtime());
        assert_eq!(DockerRuntime::new().docker, PathBuf::from("docker"));
        assert_eq!(DockerRuntime::default().docker, PathBuf::from("docker"));
    }
}
