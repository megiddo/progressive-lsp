//! EngineAdapter + spawn/ready value objects. Supervisor does not parse pack layouts.

use std::collections::BTreeMap;
use std::path::PathBuf;
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

#[derive(Clone, Debug)]
pub struct ChildHandle {
    pub id: u64,
    pub pack_name: String,
    pub capabilities: EngineCapabilities,
    alive: Arc<AtomicBool>,
    inbox: Arc<Mutex<Vec<EngineMessage>>>,
    io: ChildIo,
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
        }
    }

    pub fn with_io(mut self, io: ChildIo) -> Self {
        self.io = io;
        self
    }

    pub fn io(&self) -> &ChildIo {
        &self.io
    }

    pub fn is_alive(&self) -> bool {
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
    }
}
