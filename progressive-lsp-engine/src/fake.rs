//! Test double. Same EngineAdapter trait as production. Never execs a child.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use progressive_lsp_core::{EngineError, LanguageId, PrefixLayout, Tier};
use progressive_lsp_resolve::{
    Hover, LspLocation, QueryKind, Range, ResolveOutcome, ResolveQuery, ResolveResult,
};

use crate::adapter::{ChildHandle, ChildIo, EngineAdapter, EngineBinary, ReadyKind, SpawnCtx};
use crate::capabilities::EngineCapabilities;
use crate::discovery::{
    discover_pack_opt, BIOME_PACK, CLANGD_PACK, GOPLS_PACK, PHPANTOM_PACK, PYTHON_PACK,
    SUPERHTML_PACK, TSGO_PACK, ZLS_PACK,
};

#[derive(Clone, Debug, Default)]
pub struct FakeAnswers {
    pub definition: Vec<LspLocation>,
    pub references: Vec<LspLocation>,
    pub implementation: Vec<LspLocation>,
    pub hover: Option<Hover>,
}

pub struct FakeEngineAdapter {
    pack_name: String,
    language: LanguageId,
    discovered: Mutex<Option<EngineBinary>>,
    crash_on_spawn: AtomicBool,
    crash_after_ready: AtomicBool,
    alive: AtomicBool,
    spawn_count: AtomicU32,
    answers: Mutex<FakeAnswers>,
    capabilities: EngineCapabilities,
    ready_kind: Mutex<ReadyKind>,
    extra: Vec<LanguageId>,
    io: ChildIo,
}

impl FakeEngineAdapter {
    pub fn new(pack_name: impl Into<String>, language: impl Into<LanguageId>) -> Self {
        Self {
            pack_name: pack_name.into(),
            language: language.into(),
            discovered: Mutex::new(None),
            crash_on_spawn: AtomicBool::new(false),
            crash_after_ready: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            spawn_count: AtomicU32::new(0),
            answers: Mutex::new(FakeAnswers::default()),
            capabilities: EngineCapabilities::types_full(),
            ready_kind: Mutex::new(ReadyKind::Initialize),
            extra: Vec::new(),
            io: ChildIo::lsp_with_stderr_pipe(),
        }
    }

    pub fn ty() -> Self {
        Self::new(PYTHON_PACK, LanguageId::new("python"))
    }

    pub fn rust_analyzer() -> Self {
        Self::new(crate::discovery::RUST_PACK, LanguageId::new("rust"))
    }

    pub fn clangd() -> Self {
        let mut a = Self::new(CLANGD_PACK, LanguageId::new("c"));
        a.extra = vec![LanguageId::new("cpp")];
        a
    }

    pub fn tsgo() -> Self {
        let mut a = Self::new(TSGO_PACK, LanguageId::new("typescript"));
        a.extra = vec![LanguageId::new("javascript")];
        a
    }

    pub fn phpantom() -> Self {
        Self::new(PHPANTOM_PACK, LanguageId::new("php"))
    }

    pub fn superhtml() -> Self {
        Self::new(SUPERHTML_PACK, LanguageId::new("html"))
    }

    pub fn biome() -> Self {
        Self::new(BIOME_PACK, LanguageId::new("css"))
    }

    pub fn gopls() -> Self {
        Self::new(GOPLS_PACK, LanguageId::new("go"))
    }

    pub fn zls() -> Self {
        Self::new(ZLS_PACK, LanguageId::new("zig"))
    }

    pub fn with_binary(self, binary: EngineBinary) -> Self {
        *self.discovered.lock().expect("disc") = Some(binary);
        self
    }

    pub fn with_child_io(mut self, io: ChildIo) -> Self {
        self.io = io;
        self
    }

    pub fn crash_on_spawn(self) -> Self {
        self.crash_on_spawn.store(true, Ordering::SeqCst);
        self
    }

    pub fn set_crash_on_spawn(&self, v: bool) {
        self.crash_on_spawn.store(v, Ordering::SeqCst);
    }

    pub fn set_crash_after_ready(&self, v: bool) {
        self.crash_after_ready.store(v, Ordering::SeqCst);
    }

    pub fn set_alive(&self, v: bool) {
        self.alive.store(v, Ordering::SeqCst);
    }

    pub fn set_answers(&self, answers: FakeAnswers) {
        *self.answers.lock().expect("ans") = answers;
    }

    pub fn set_ready_kind(&self, kind: ReadyKind) {
        *self.ready_kind.lock().expect("rk") = kind;
    }

    pub fn spawn_count(&self) -> u32 {
        self.spawn_count.load(Ordering::SeqCst)
    }

    pub fn typed_fixture(name: &str, uri: &str) -> FakeAnswers {
        let loc = LspLocation::new(uri, Range::default(), Tier::Types);
        FakeAnswers {
            definition: vec![loc.clone()],
            references: vec![loc.clone()],
            implementation: vec![loc],
            hover: Some(Hover::typed(name, "int")),
        }
    }
}

impl EngineAdapter for FakeEngineAdapter {
    fn pack_name(&self) -> &str {
        &self.pack_name
    }

    fn language_id(&self) -> LanguageId {
        self.language.clone()
    }

    fn discover(&self, prefix: &PrefixLayout) -> Option<EngineBinary> {
        if let Some(b) = self.discovered.lock().expect("disc").clone() {
            return Some(b);
        }
        discover_pack_opt(prefix, &self.pack_name)
    }

    fn spawn(&self, ctx: SpawnCtx) -> Result<ChildHandle, EngineError> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        if self.crash_on_spawn.load(Ordering::SeqCst) {
            return Err(EngineError::Crashed(format!("{} spawn", self.pack_name)));
        }
        if ctx.binary.pack_name != self.pack_name && !ctx.binary.pack_name.is_empty() {
            return Err(EngineError::Spawn("pack mismatch".into()));
        }
        self.alive.store(true, Ordering::SeqCst);
        Ok(ChildHandle::new(
            u64::from(self.spawn_count.load(Ordering::SeqCst)),
            self.pack_name.clone(),
            self.capabilities,
        )
        .with_io(self.io.clone()))
    }

    fn ready_signal(&self) -> ReadyKind {
        self.ready_kind.lock().expect("rk").clone()
    }

    fn resolve_query(&self, handle: &ChildHandle, q: &ResolveQuery) -> ResolveOutcome {
        if !handle.is_alive() || !self.alive.load(Ordering::SeqCst) {
            return ResolveOutcome::NotReady;
        }
        if self.crash_after_ready.load(Ordering::SeqCst) {
            handle.mark_dead();
            self.alive.store(false, Ordering::SeqCst);
            return ResolveOutcome::NotReady;
        }
        let a = self.answers.lock().expect("ans");
        match q.kind {
            QueryKind::Definition | QueryKind::TypeDefinition => {
                if a.definition.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(Tier::Types, a.definition.clone()))
            }
            QueryKind::References => {
                if a.references.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(Tier::Types, a.references.clone()))
            }
            QueryKind::Implementation => {
                if a.implementation.is_empty() {
                    return ResolveOutcome::NotReady;
                }
                ResolveOutcome::Ready(ResolveResult::locations(
                    Tier::Types,
                    a.implementation.clone(),
                ))
            }
            QueryKind::Hover => {
                let Some(h) = a.hover.clone() else {
                    return ResolveOutcome::NotReady;
                };
                ResolveOutcome::Ready(ResolveResult {
                    locations: a.definition.clone(),
                    tier: Tier::Types,
                    hover: Some(h),
                    symbols: Vec::new(),
                })
            }
            QueryKind::DocumentSymbol | QueryKind::WorkspaceSymbol => ResolveOutcome::NotReady,
        }
    }

    fn is_alive(&self, handle: &ChildHandle) -> bool {
        handle.is_alive() && self.alive.load(Ordering::SeqCst)
    }

    fn extra_languages(&self) -> Vec<LanguageId> {
        self.extra.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FileId, PackageId};
    use progressive_lsp_resolve::Position;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn spawn_ctx(pack: &str) -> SpawnCtx {
        SpawnCtx {
            workspace: PathBuf::from("/w"),
            language: LanguageId::new("python"),
            package: PackageId::new("pkg"),
            argv: Vec::new(),
            cwd: PathBuf::from("/w"),
            env: BTreeMap::new(),
            binary: EngineBinary {
                pack_name: pack.into(),
                path: PathBuf::from("/w/ty"),
                sha256: [0; 32],
            },
        }
    }

    #[test]
    fn fake_answers_definition_refs_hover_impl() {
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///a.py"));
        let h = fake.spawn(spawn_ctx("python")).unwrap();
        assert!(h.io().has_stderr_pipe());
        assert!(h.io().stdout_is_never_log_adapter());
        assert_eq!(fake.spawn_count(), 1);
        assert_eq!(fake.pack_name(), "python");
        assert_eq!(fake.language_id().as_str(), "python");
        assert_eq!(fake.ready_signal(), ReadyKind::Initialize);
        let q = |k| ResolveQuery::new(FileId::new("a.py"), Position::default(), k);
        match fake.resolve_query(&h, &q(QueryKind::Definition)) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert_eq!(r.locations[0].uri, "file:///a.py");
            }
            other => panic!("{other:?}"),
        }
        assert!(fake.resolve_query(&h, &q(QueryKind::References)).is_ready());
        assert!(fake
            .resolve_query(&h, &q(QueryKind::Implementation))
            .is_ready());
        match fake.resolve_query(&h, &q(QueryKind::Hover)) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.hover.as_ref().unwrap().signature(), "greet: int");
            }
            other => panic!("{other:?}"),
        }
        assert!(!fake
            .resolve_query(&h, &q(QueryKind::DocumentSymbol))
            .is_ready());
        let ra = FakeEngineAdapter::rust_analyzer();
        assert_eq!(ra.pack_name(), "rust");
        assert_eq!(ra.language_id().as_str(), "rust");
        assert_eq!(FakeEngineAdapter::clangd().pack_name(), "clangd");
        assert_eq!(
            FakeEngineAdapter::clangd().extra_languages(),
            vec![LanguageId::new("cpp")]
        );
        assert_eq!(
            FakeEngineAdapter::tsgo().language_id().as_str(),
            "typescript"
        );
        assert_eq!(FakeEngineAdapter::phpantom().pack_name(), "phpantom");
        assert_eq!(
            FakeEngineAdapter::superhtml().language_id().as_str(),
            "html"
        );
        assert_eq!(FakeEngineAdapter::biome().language_id().as_str(), "css");
        assert_eq!(FakeEngineAdapter::gopls().language_id().as_str(), "go");
        assert_eq!(FakeEngineAdapter::zls().language_id().as_str(), "zig");
    }

    #[test]
    fn fake_crash_on_spawn_and_after_ready() {
        let boom = FakeEngineAdapter::ty().crash_on_spawn();
        assert!(boom.spawn(spawn_ctx("python")).is_err());
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("x", "file:///x.py"));
        fake.set_crash_after_ready(true);
        let h = fake.spawn(spawn_ctx("python")).unwrap();
        let q = ResolveQuery::new(
            FileId::new("x.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(!fake.resolve_query(&h, &q).is_ready());
        assert!(!h.is_alive());
        fake.set_alive(true);
        fake.set_crash_after_ready(false);
        fake.set_answers(FakeAnswers::default());
        let h2 = fake.spawn(spawn_ctx("python")).unwrap();
        assert!(!fake.resolve_query(&h2, &q).is_ready());
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        assert_eq!(
            fake.ready_signal(),
            ReadyKind::IndexedPackage(PackageId::new("pkg"))
        );
        assert!(fake.discover(&PrefixLayout::from_path("/nope")).is_none());
        let with = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [1; 32],
        });
        assert!(with.discover(&PrefixLayout::from_path("/nope")).is_some());
        assert!(fake.spawn(spawn_ctx("other")).is_err());
        let quiet = FakeEngineAdapter::ty().with_child_io(ChildIo::lsp_without_stderr());
        let hq = quiet.spawn(spawn_ctx("python")).unwrap();
        assert!(!hq.io().has_stderr_pipe());
        assert!(hq.io().stdout_is_never_log_adapter());
    }
}
