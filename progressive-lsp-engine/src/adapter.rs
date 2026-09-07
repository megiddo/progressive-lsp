//! EngineAdapter + spawn/ready value objects. Supervisor does not parse pack layouts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{EngineError, LanguageId, PackageId, PrefixLayout};
use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery};

use crate::capabilities::EngineCapabilities;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineBinary {
    pub pack_name: String,
    pub path: PathBuf,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnCtx {
    pub workspace: PathBuf,
    pub language: LanguageId,
    pub package: PackageId,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub binary: EngineBinary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadyKind {
    Initialize,
    IndexedPackage(PackageId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineMessage {
    DidChange { uri: String, text: String },
    Watch { paths: Vec<String> },
}

/// Value object. stdout is LSP JSON-RPC (never a log Adapter).
/// stderr is an optional capture pipe for `ChildStderrAdapter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildIo {
    stdout_lsp: bool,
    stderr_pipe: bool,
}

impl ChildIo {
    /// Production pack spawn: LSP stdout + stderr pipe. Never `NullStderrAdapter`.
    pub fn lsp_with_stderr_pipe() -> Self {
        Self {
            stdout_lsp: true,
            stderr_pipe: true,
        }
    }

    /// No stderr pipe (tests / reserved). Still never a log Adapter on stdout.
    pub fn lsp_without_stderr() -> Self {
        Self {
            stdout_lsp: true,
            stderr_pipe: false,
        }
    }

    pub fn stdout_is_lsp(&self) -> bool {
        self.stdout_lsp
    }

    /// Invariant: a log Adapter is never attached to engine stdout.
    pub fn stdout_is_never_log_adapter(&self) -> bool {
        self.stdout_lsp
    }

    pub fn has_stderr_pipe(&self) -> bool {
        self.stderr_pipe
    }
}

/// Value object. Linux `Command` argv/cwd/env/`ChildIo` without exec on Darwin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnPlan {
    program: PathBuf,
    argv: Vec<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    io: ChildIo,
}

impl SpawnPlan {
    /// Production pack plan: LSP stdout + stderr pipe. Never `NullStderrAdapter`.
    pub fn from_spawn_ctx(ctx: &SpawnCtx) -> Self {
        Self {
            program: ctx.binary.path.clone(),
            argv: ctx.argv.clone(),
            cwd: ctx.cwd.clone(),
            env: ctx.env.clone(),
            io: ChildIo::lsp_with_stderr_pipe(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    pub fn io(&self) -> &ChildIo {
        &self.io
    }

    /// Extra argv after the program path when `argv[0]` repeats the binary.
    pub fn command_args(&self) -> &[String] {
        let program = self.program.display().to_string();
        match self.argv.first() {
            Some(first) if first == &program => &self.argv[1..],
            _ => &self.argv,
        }
    }

    /// Tests assert the Port refuses a plan that is not `lsp_with_stderr_pipe`.
    pub fn with_io(mut self, io: ChildIo) -> Self {
        self.io = io;
        self
    }
}

/// Port. Production is Linux `Command`; tests inject a recording double.
pub trait SpawnPort: Send + Sync {
    fn spawn_child(&self, plan: &SpawnPlan, pack_name: &str) -> Result<ChildHandle, EngineError>;
}

/// Production Port. `std::process::Command` on Linux only.
pub struct CommandSpawnPort;

impl SpawnPort for CommandSpawnPort {
    fn spawn_child(&self, plan: &SpawnPlan, pack_name: &str) -> Result<ChildHandle, EngineError> {
        spawn_linux_command(plan, pack_name)
    }
}

/// Test double. Records the plan and returns a handle — never `Command`.
#[derive(Default)]
pub struct RecordingSpawnPort {
    last: Mutex<Option<SpawnPlan>>,
}

impl RecordingSpawnPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_plan(&self) -> Option<SpawnPlan> {
        self.last.lock().expect("RecordingSpawnPort").clone()
    }
}

impl SpawnPort for RecordingSpawnPort {
    fn spawn_child(&self, plan: &SpawnPlan, pack_name: &str) -> Result<ChildHandle, EngineError> {
        *self.last.lock().expect("RecordingSpawnPort") = Some(plan.clone());
        Ok(
            ChildHandle::new(1, pack_name, EngineCapabilities::types_full())
                .with_io(plan.io.clone()),
        )
    }
}

pub(crate) fn spawn_linux_command(
    plan: &SpawnPlan,
    pack_name: &str,
) -> Result<ChildHandle, EngineError> {
    if !plan.io.stdout_is_never_log_adapter() || !plan.io.has_stderr_pipe() {
        return Err(EngineError::Spawn(
            "production pack spawn requires ChildIo::lsp_with_stderr_pipe".into(),
        ));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pack_name;
        return Err(EngineError::Spawn(
            "host process spawn of engine packs is reserved for Linux (not this OS)".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new(plan.program());
        cmd.args(plan.command_args())
            .current_dir(plan.cwd())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (k, v) in plan.env() {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| {
            EngineError::Spawn(format!("command {}: {e}", plan.program().display()))
        })?;
        let stderr = child.stderr.take();
        Ok(ChildHandle::new(
            u64::from(child.id()),
            pack_name,
            EngineCapabilities::types_full(),
        )
        .with_io(plan.io.clone())
        .with_os_child(child, stderr))
    }
}

struct OsChild {
    child: Mutex<std::process::Child>,
    stderr: Mutex<Option<std::process::ChildStderr>>,
}

#[derive(Clone)]
pub struct ChildHandle {
    pub id: u64,
    pub pack_name: String,
    pub capabilities: EngineCapabilities,
    alive: Arc<AtomicBool>,
    inbox: Arc<Mutex<Vec<EngineMessage>>>,
    io: ChildIo,
    os: Option<Arc<OsChild>>,
}

impl std::fmt::Debug for ChildHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildHandle")
            .field("id", &self.id)
            .field("pack_name", &self.pack_name)
            .field("capabilities", &self.capabilities)
            .field("io", &self.io)
            .field("has_os_child", &self.os.is_some())
            .finish()
    }
}

impl PartialEq for ChildHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.pack_name == other.pack_name
    }
}

impl Eq for ChildHandle {}

impl ChildHandle {
    pub fn new(id: u64, pack_name: impl Into<String>, capabilities: EngineCapabilities) -> Self {
        Self {
            id,
            pack_name: pack_name.into(),
            capabilities,
            alive: Arc::new(AtomicBool::new(true)),
            inbox: Arc::new(Mutex::new(Vec::new())),
            io: ChildIo::lsp_with_stderr_pipe(),
            os: None,
        }
    }

    pub fn with_io(mut self, io: ChildIo) -> Self {
        self.io = io;
        self
    }

    pub fn with_os_child(
        mut self,
        child: std::process::Child,
        stderr: Option<std::process::ChildStderr>,
    ) -> Self {
        self.os = Some(Arc::new(OsChild {
            child: Mutex::new(child),
            stderr: Mutex::new(stderr),
        }));
        self
    }

    pub fn io(&self) -> &ChildIo {
        &self.io
    }

    /// True when Linux `Command` left a stderr pipe `Read` on this handle.
    pub fn has_os_stderr(&self) -> bool {
        self.os
            .as_ref()
            .and_then(|os| os.stderr.lock().ok())
            .is_some_and(|g| g.is_some())
    }

    pub fn is_alive(&self) -> bool {
        if let Some(os) = &self.os {
            if let Ok(mut child) = os.child.lock() {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.alive.store(false, Ordering::SeqCst);
                        return false;
                    }
                    Ok(None) => return true,
                    Err(_) => {}
                }
            }
        }
        self.alive.load(Ordering::SeqCst)
    }

    pub fn mark_dead(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    pub fn push_message(&self, msg: EngineMessage) {
        self.inbox.lock().expect("inbox").push(msg);
    }

    pub fn inbox(&self) -> Vec<EngineMessage> {
        self.inbox.lock().expect("inbox").clone()
    }
}

/// Child argv/stdio/ready → supervisor API.
pub trait EngineAdapter: Send + Sync {
    fn pack_name(&self) -> &str;
    fn language_id(&self) -> LanguageId;
    fn discover(&self, prefix: &PrefixLayout) -> Option<EngineBinary>;
    fn spawn(&self, ctx: SpawnCtx) -> Result<ChildHandle, EngineError>;
    fn ready_signal(&self) -> ReadyKind;

    fn resolve_query(&self, _handle: &ChildHandle, _q: &ResolveQuery) -> ResolveOutcome {
        ResolveOutcome::NotReady
    }

    fn forward_did_change(&self, handle: &ChildHandle, uri: &str, text: &str) {
        handle.push_message(EngineMessage::DidChange {
            uri: uri.to_string(),
            text: text.to_string(),
        });
    }

    fn forward_watch(&self, handle: &ChildHandle, paths: &[String]) {
        handle.push_message(EngineMessage::Watch {
            paths: paths.to_vec(),
        });
    }

    fn is_alive(&self, handle: &ChildHandle) -> bool {
        handle.is_alive()
    }

    /// Sibling languages served by the same pack (clangd → cpp, tsgo → javascript).
    fn extra_languages(&self) -> Vec<LanguageId> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::PrefixLayout;

    struct Stub;

    impl EngineAdapter for Stub {
        fn pack_name(&self) -> &str {
            "python"
        }
        fn language_id(&self) -> LanguageId {
            LanguageId::new("python")
        }
        fn discover(&self, _prefix: &PrefixLayout) -> Option<EngineBinary> {
            None
        }
        fn spawn(&self, _ctx: SpawnCtx) -> Result<ChildHandle, EngineError> {
            Err(EngineError::NotDiscovered("python".into()))
        }
        fn ready_signal(&self) -> ReadyKind {
            ReadyKind::Initialize
        }
    }

    #[test]
    fn child_handle_alive_and_inbox() {
        let h = ChildHandle::new(1, "python", EngineCapabilities::types_full());
        assert!(h.is_alive());
        assert_eq!(h.pack_name, "python");
        assert_eq!(h.id, 1);
        h.push_message(EngineMessage::Watch {
            paths: vec!["a.py".into()],
        });
        assert_eq!(
            h.inbox(),
            vec![EngineMessage::Watch {
                paths: vec!["a.py".into()]
            }]
        );
        h.mark_dead();
        assert!(!h.is_alive());
        let other = ChildHandle::new(1, "python", EngineCapabilities::empty());
        assert_eq!(h, other);
        assert_ne!(
            h,
            ChildHandle::new(2, "python", EngineCapabilities::empty())
        );
    }

    #[test]
    fn stub_adapter_defaults() {
        let a = Stub;
        assert_eq!(a.pack_name(), "python");
        assert_eq!(a.language_id().as_str(), "python");
        assert!(a.discover(&PrefixLayout::from_path("/p")).is_none());
        assert!(a
            .spawn(SpawnCtx {
                workspace: PathBuf::from("/w"),
                language: LanguageId::new("python"),
                package: PackageId::new("pkg"),
                argv: Vec::new(),
                cwd: PathBuf::from("/w"),
                env: BTreeMap::new(),
                binary: EngineBinary {
                    pack_name: "python".into(),
                    path: PathBuf::from("/missing"),
                    sha256: [0; 32],
                },
            })
            .is_err());
        assert_eq!(a.ready_signal(), ReadyKind::Initialize);
        let h = ChildHandle::new(3, "python", EngineCapabilities::empty());
        assert!(!a
            .resolve_query(
                &h,
                &ResolveQuery::new(
                    progressive_lsp_core::FileId::new("a.py"),
                    progressive_lsp_resolve::Position::default(),
                    progressive_lsp_resolve::QueryKind::Definition,
                )
            )
            .is_ready());
        a.forward_did_change(&h, "file:///a.py", "x = 1");
        a.forward_watch(&h, &["a.py".into()]);
        assert_eq!(h.inbox().len(), 2);
        assert!(a.is_alive(&h), "default is_alive follows ChildHandle");
        h.mark_dead();
        assert!(!a.is_alive(&h), "default is_alive is false after mark_dead");
        assert_eq!(
            ReadyKind::IndexedPackage(PackageId::new("p")),
            ReadyKind::IndexedPackage(PackageId::new("p"))
        );
        assert!(a.extra_languages().is_empty());
    }

    #[test]
    fn child_io_stdout_is_never_log_adapter_value_object() {
        let piped = ChildIo::lsp_with_stderr_pipe();
        assert!(piped.stdout_is_lsp());
        assert!(piped.stdout_is_never_log_adapter());
        assert!(piped.has_stderr_pipe());
        let quiet = ChildIo::lsp_without_stderr();
        assert!(quiet.stdout_is_lsp());
        assert!(quiet.stdout_is_never_log_adapter());
        assert!(!quiet.has_stderr_pipe());
        let h = ChildHandle::new(1, "python", EngineCapabilities::empty());
        assert!(h.io().has_stderr_pipe());
        assert!(h.io().stdout_is_never_log_adapter());
        let h = h.with_io(ChildIo::lsp_without_stderr());
        assert!(!h.io().has_stderr_pipe());
        assert!(!h.has_os_stderr());
    }

    #[test]
    fn spawn_plan_is_value_object_for_linux_command_without_exec() {
        let ctx = SpawnCtx {
            workspace: PathBuf::from("/w"),
            language: LanguageId::new("python"),
            package: PackageId::new("pkg"),
            argv: vec!["/engines/python/ty".into(), "--stdio".into()],
            cwd: PathBuf::from("/w"),
            env: BTreeMap::from([("TY_LOG".into(), "info".into())]),
            binary: EngineBinary {
                pack_name: "python".into(),
                path: PathBuf::from("/engines/python/ty"),
                sha256: [0; 32],
            },
        };
        let plan = SpawnPlan::from_spawn_ctx(&ctx);
        assert_eq!(plan.program(), Path::new("/engines/python/ty"));
        assert_eq!(plan.argv(), &["/engines/python/ty", "--stdio"]);
        assert_eq!(plan.command_args(), &["--stdio"]);
        assert_eq!(plan.cwd(), Path::new("/w"));
        assert_eq!(plan.env().get("TY_LOG").map(String::as_str), Some("info"));
        assert!(plan.io().has_stderr_pipe());
        assert!(plan.io().stdout_is_never_log_adapter());
        assert_eq!(plan, SpawnPlan::from_spawn_ctx(&ctx));
        let no_dup = SpawnPlan::from_spawn_ctx(&SpawnCtx {
            argv: vec!["--stdio".into()],
            ..ctx
        });
        assert_eq!(no_dup.command_args(), &["--stdio"]);
        let recorded = RecordingSpawnPort::new();
        let handle = recorded
            .spawn_child(&plan, "python")
            .expect("RecordingSpawnPort never execs");
        assert_eq!(recorded.last_plan().as_ref(), Some(&plan));
        assert!(handle.io().has_stderr_pipe());
        assert!(!handle.has_os_stderr());
        let reserved = CommandSpawnPort
            .spawn_child(&plan, "python")
            .expect_err("Darwin / missing binary must not look like a live musl child");
        assert!(
            reserved.to_string().contains("not this OS")
                || reserved.to_string().contains("command"),
            "{reserved}"
        );
        let quiet = plan.clone().with_io(ChildIo::lsp_without_stderr());
        let io_err = CommandSpawnPort
            .spawn_child(&quiet, "python")
            .expect_err("NullStderrAdapter / no-pipe is forbidden");
        assert!(
            io_err.to_string().contains("lsp_with_stderr_pipe"),
            "{io_err}"
        );
        let mut child = std::process::Command::new("true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("host true is not a pack engine");
        let stderr = child.stderr.take();
        let os =
            ChildHandle::new(9, "python", EngineCapabilities::empty()).with_os_child(child, stderr);
        assert!(os.has_os_stderr());
        let _ = os.is_alive();
        let debug = format!("{os:?}");
        assert!(debug.contains("python"), "{debug}");
        assert!(debug.contains("has_os_child: true"), "{debug}");
        let plain = format!(
            "{:?}",
            ChildHandle::new(1, "python", EngineCapabilities::empty())
        );
        assert!(plain.contains("has_os_child: false"), "{plain}");
    }
}
