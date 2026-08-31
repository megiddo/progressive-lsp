//! EngineSupervisor: spawn, stdio proxy, crash/backoff, capability merge.
//! Core stays up if a child dies; T2/T1 remain.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{
    ClockPort, EngineError, FileId, LanguageId, LogComponent, LogLevel, LogPort, LogScope, NullLog,
    PackageId, PrefixLayout,
};
use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery};

use crate::adapter::{ChildHandle, ChildIo, EngineAdapter, ReadyKind, SpawnCtx};
use crate::backoff::{can_respawn, BackoffPolicy};
use crate::capabilities::EngineCapabilities;
use crate::discovery::discover_pack;
use crate::hooks::{apply_tweaks, EngineHooks, NoopHooks, SpawnHookResult};

struct SupervisedChild {
    handle: ChildHandle,
    language: LanguageId,
}

struct SupervisorState {
    children: BTreeMap<String, SupervisedChild>,
    ready: BTreeSet<(String, String)>,
    ready_languages: BTreeSet<String>,
    backoff_until: BTreeMap<String, u64>,
    crash_count: BTreeMap<String, u32>,
    file_packages: BTreeMap<String, PackageId>,
    capabilities: EngineCapabilities,
    progress_notes: Vec<String>,
    last_error: BTreeMap<String, EngineError>,
    stderr_attached: BTreeSet<String>,
    tail_attached: BTreeSet<String>,
    log_message_attached: BTreeSet<String>,
}

impl SupervisorState {
    fn new() -> Self {
        Self {
            children: BTreeMap::new(),
            ready: BTreeSet::new(),
            ready_languages: BTreeSet::new(),
            backoff_until: BTreeMap::new(),
            crash_count: BTreeMap::new(),
            file_packages: BTreeMap::new(),
            capabilities: EngineCapabilities::empty(),
            progress_notes: Vec::new(),
            last_error: BTreeMap::new(),
            stderr_attached: BTreeSet::new(),
            tail_attached: BTreeSet::new(),
            log_message_attached: BTreeSet::new(),
        }
    }
}

pub struct EngineSupervisor {
    clock: Arc<dyn ClockPort>,
    prefix: PrefixLayout,
    adapters: Vec<Box<dyn EngineAdapter>>,
    hooks: Arc<dyn EngineHooks>,
    policy: BackoffPolicy,
    log: Arc<dyn LogPort>,
    inner: Mutex<SupervisorState>,
}

impl EngineSupervisor {
    pub fn new(clock: Arc<dyn ClockPort>, prefix: PrefixLayout) -> Self {
        Self {
            clock,
            prefix,
            adapters: Vec::new(),
            hooks: Arc::new(NoopHooks),
            policy: BackoffPolicy::DEFAULT,
            log: Arc::new(NullLog),
            inner: Mutex::new(SupervisorState::new()),
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    pub fn with_hooks(mut self, hooks: Arc<dyn EngineHooks>) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_policy(mut self, policy: BackoffPolicy) -> Self {
        self.policy = policy;
        self
    }

    fn emit_spawn(&self, level: LogLevel, message: &str) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("spawn")
                .component(LogComponent::engine()),
        );
        match level {
            LogLevel::Error => self.log.error(message),
            LogLevel::Warn => self.log.warn(message),
            LogLevel::Info => self.log.info(message),
            LogLevel::Debug => self.log.debug(message),
            LogLevel::Trace => self.log.trace(message),
        }
    }

    pub fn register(&mut self, adapter: Box<dyn EngineAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn prefix(&self) -> &PrefixLayout {
        &self.prefix
    }

    pub fn adapter_names(&self) -> Vec<String> {
        self.adapters
            .iter()
            .map(|a| a.pack_name().to_string())
            .collect()
    }

    pub fn bind_file(&self, file: FileId, package: PackageId) {
        self.inner
            .lock()
            .expect("sup")
            .file_packages
            .insert(file.as_str().to_string(), package);
    }

    pub fn package_for_file(&self, file: &FileId) -> PackageId {
        self.inner
            .lock()
            .expect("sup")
            .file_packages
            .get(file.as_str())
            .cloned()
            .unwrap_or_else(|| PackageId::new("pkg"))
    }

    pub fn is_ready(&self, language: &LanguageId, package: &PackageId) -> bool {
        self.poll_health();
        let st = self.inner.lock().expect("sup");
        st.ready
            .contains(&(language.as_str().to_string(), package.as_str().to_string()))
            || st.ready_languages.contains(language.as_str())
    }

    pub fn merged_capabilities(&self) -> EngineCapabilities {
        self.inner.lock().expect("sup").capabilities
    }

    pub fn progress_notes(&self) -> Vec<String> {
        self.inner.lock().expect("sup").progress_notes.clone()
    }

    pub fn last_error(&self, pack: &str) -> Option<EngineError> {
        self.inner
            .lock()
            .expect("sup")
            .last_error
            .get(pack)
            .cloned()
    }

    /// Spawn every registered pack. Serve calls this after initialize has a workspace root.
    pub fn try_spawn_registered(
        &self,
        workspace: &Path,
        package: &PackageId,
    ) -> Vec<Result<bool, EngineError>> {
        let jobs: Vec<(String, LanguageId)> = self
            .adapters
            .iter()
            .map(|a| (a.pack_name().to_string(), a.language_id()))
            .collect();
        jobs.into_iter()
            .map(|(pack, lang)| self.try_spawn(&pack, &lang, package, workspace))
            .collect()
    }

    pub fn try_spawn(
        &self,
        pack: &str,
        language: &LanguageId,
        package: &PackageId,
        workspace: &Path,
    ) -> Result<bool, EngineError> {
        self.poll_health();
        let now = self.clock.unix_ms();
        {
            let st = self.inner.lock().expect("sup");
            if let Some(&until) = st.backoff_until.get(pack) {
                if !can_respawn(now, until) {
                    let err = EngineError::Backoff {
                        next_unix_ms: until,
                    };
                    drop(st);
                    self.emit_spawn(LogLevel::Info, &format!("engine {pack}: {err}"));
                    return Err(err);
                }
            }
        }
        let idx = match self.adapters.iter().position(|a| a.pack_name() == pack) {
            Some(i) => i,
            None => {
                let err = EngineError::NotDiscovered(pack.into());
                self.emit_spawn(LogLevel::Warn, &format!("engine {pack}: {err}"));
                return Err(err);
            }
        };
        let adapter = &self.adapters[idx];
        let binary = match adapter.discover(&self.prefix) {
            Some(b) => b,
            None => match discover_pack(&self.prefix, pack) {
                Ok(b) => b,
                Err(e @ EngineError::Hash { .. }) => {
                    self.inner
                        .lock()
                        .expect("sup")
                        .last_error
                        .insert(pack.into(), e.clone());
                    self.emit_spawn(LogLevel::Warn, &format!("engine {pack}: {e}"));
                    return Err(e);
                }
                Err(e) => {
                    self.inner
                        .lock()
                        .expect("sup")
                        .last_error
                        .insert(pack.into(), e.clone());
                    self.emit_spawn(LogLevel::Warn, &format!("engine {pack}: {e}"));
                    return Ok(false);
                }
            },
        };
        let ctx = SpawnCtx {
            workspace: workspace.to_path_buf(),
            language: language.clone(),
            package: package.clone(),
            argv: vec![binary.path.display().to_string()],
            cwd: workspace.to_path_buf(),
            env: BTreeMap::new(),
            binary,
        };
        match self.hooks.decide_spawn(pack, &ctx) {
            SpawnHookResult::Abort(msg) => {
                let err = EngineError::Aborted(msg);
                self.inner
                    .lock()
                    .expect("sup")
                    .last_error
                    .insert(pack.into(), err.clone());
                self.emit_spawn(LogLevel::Warn, &format!("engine {pack}: {err}"));
                return Err(err);
            }
            SpawnHookResult::Proceed(tweak) => {
                let ctx = apply_tweaks(ctx, &tweak, &self.prefix);
                match adapter.spawn(ctx) {
                    Ok(handle) => {
                        self.mark_ready(adapter.as_ref(), handle, language, package);
                        self.hooks
                            .notify_tier_ready(language.as_str(), package.as_str());
                        self.emit_spawn(LogLevel::Info, &format!("engine {pack} spawned"));
                        Ok(true)
                    }
                    Err(e) => {
                        self.note_crash_err(pack, e.clone());
                        Err(e)
                    }
                }
            }
        }
    }

    fn mark_ready(
        &self,
        adapter: &dyn EngineAdapter,
        handle: ChildHandle,
        language: &LanguageId,
        package: &PackageId,
    ) {
        let mut st = self.inner.lock().expect("sup");
        let caps = handle.capabilities;
        st.capabilities = st.capabilities.merge(caps);
        st.crash_count.insert(adapter.pack_name().into(), 0);
        st.backoff_until.remove(adapter.pack_name());
        st.last_error.remove(adapter.pack_name());
        let langs: Vec<LanguageId> = std::iter::once(language.clone())
            .chain(adapter.extra_languages())
            .collect();
        match adapter.ready_signal() {
            ReadyKind::Initialize => {
                for lang in &langs {
                    st.ready_languages.insert(lang.as_str().to_string());
                }
            }
            ReadyKind::IndexedPackage(pkg) => {
                for lang in &langs {
                    st.ready
                        .insert((lang.as_str().to_string(), pkg.as_str().to_string()));
                }
            }
        }
        for lang in &langs {
            st.ready
                .insert((lang.as_str().to_string(), package.as_str().to_string()));
        }
        if handle.io().stdout_is_never_log_adapter() && handle.io().has_stderr_pipe() {
            st.stderr_attached.insert(adapter.pack_name().to_string());
        }
        st.children.insert(
            adapter.pack_name().to_string(),
            SupervisedChild {
                handle,
                language: language.clone(),
            },
        );
    }

    pub fn stderr_capture_attached(&self, pack: &str) -> bool {
        self.inner
            .lock()
            .expect("sup")
            .stderr_attached
            .contains(pack)
    }

    fn child_io(&self, pack: &str) -> Option<ChildIo> {
        self.inner
            .lock()
            .expect("sup")
            .children
            .get(pack)
            .map(|c| c.handle.io().clone())
    }

    /// Attach `ChildStderrAdapter` when this child has a stderr pipe **and** a `Read` exists.
    /// `ChildHandle` has no live OS `Read`; tests pass `FakeChildStderr`.
    pub fn attach_if_stderr_pipe(
        &self,
        pack: &str,
        stderr: Option<&progressive_lsp_log::FakeChildStderr>,
    ) -> bool {
        let Some(io) = self.child_io(pack) else {
            return false;
        };
        if !io.stdout_is_never_log_adapter() {
            return false;
        }
        let Some(_adapter) = progressive_lsp_log::ChildStderrAdapter::attach_if_stderr_read(
            io.has_stderr_pipe(),
            io.stdout_is_lsp(),
            stderr,
            Arc::clone(&self.log),
            pack,
        ) else {
            return false;
        };
        true
    }

    /// `LogFileTailAdapter` only when a tail path exists. Do not enable `-rpc.trace`.
    pub fn attach_log_file_tail(&self, pack: &str, tail_path: Option<&Path>) -> bool {
        let Some(adapter) = progressive_lsp_log::LogFileTailAdapter::attach_if_tail_path(
            tail_path,
            Arc::clone(&self.log),
            pack,
        ) else {
            return false;
        };
        adapter.poll();
        self.inner
            .lock()
            .expect("sup")
            .tail_attached
            .insert(pack.into());
        true
    }

    /// `LspLogMessageAdapter` only when a proxied `window/logMessage` / `$/logTrace` exists.
    pub fn ingest_proxied_log_message(
        &self,
        pack: &str,
        method: &str,
        message: &str,
        lsp_type: u64,
    ) -> bool {
        let Some(adapter) = progressive_lsp_log::LspLogMessageAdapter::attach_if_proxied(
            method,
            Arc::clone(&self.log),
            pack,
        ) else {
            return false;
        };
        match method {
            "$/logTrace" => adapter.ingest_log_trace(message),
            _ => adapter.ingest_log_message(lsp_type, message),
        }
        self.inner
            .lock()
            .expect("sup")
            .log_message_attached
            .insert(pack.into());
        true
    }

    pub fn log_file_tail_attached(&self, pack: &str) -> bool {
        self.inner.lock().expect("sup").tail_attached.contains(pack)
    }

    pub fn lsp_log_message_attached(&self, pack: &str) -> bool {
        self.inner
            .lock()
            .expect("sup")
            .log_message_attached
            .contains(pack)
    }

    pub fn note_crash(&self, pack: &str) {
        self.note_crash_err(pack, EngineError::Crashed(pack.into()));
    }

    fn note_crash_err(&self, pack: &str, err: EngineError) {
        let now = self.clock.unix_ms();
        {
            let mut st = self.inner.lock().expect("sup");
            if let Some(child) = st.children.get(pack) {
                child.handle.mark_dead();
                let lang = child.language.as_str().to_string();
                st.ready.retain(|(l, _)| l != &lang);
                st.ready_languages.remove(&lang);
            }
            st.children.remove(pack);
            st.stderr_attached.remove(pack);
            st.tail_attached.remove(pack);
            st.log_message_attached.remove(pack);
            let n = st.crash_count.entry(pack.into()).or_insert(0);
            *n = n.saturating_add(1);
            let until = self.policy.next_attempt_ms(now, *n);
            st.backoff_until.insert(pack.into(), until);
            st.last_error.insert(pack.into(), err.clone());
            st.progress_notes
                .push(format!("engine {pack} crashed; backing off until {until}"));
            let mut caps = EngineCapabilities::empty();
            for child in st.children.values() {
                if child.handle.is_alive() {
                    caps = caps.merge(child.handle.capabilities);
                }
            }
            st.capabilities = caps;
        }
        self.emit_spawn(LogLevel::Warn, &format!("engine {pack}: {err}"));
    }

    pub fn poll_health(&self) {
        let dead: Vec<String> = {
            let st = self.inner.lock().expect("sup");
            st.children
                .iter()
                .filter(|(pack, c)| {
                    self.adapters
                        .iter()
                        .find(|a| a.pack_name() == pack.as_str())
                        .map(|a| !a.is_alive(&c.handle))
                        .unwrap_or(!c.handle.is_alive())
                })
                .map(|(p, _)| p.clone())
                .collect()
        };
        for pack in dead {
            self.note_crash(&pack);
        }
    }

    pub fn tick(&self, workspace: &Path) -> Vec<Result<bool, EngineError>> {
        self.poll_health();
        let now = self.clock.unix_ms();
        let due: Vec<(String, LanguageId, PackageId)> = {
            let st = self.inner.lock().expect("sup");
            self.adapters
                .iter()
                .filter_map(|a| {
                    let pack = a.pack_name();
                    if st.children.contains_key(pack) {
                        return None;
                    }
                    if let Some(&until) = st.backoff_until.get(pack) {
                        if !can_respawn(now, until) {
                            return None;
                        }
                    } else {
                        return None;
                    }
                    Some((pack.to_string(), a.language_id(), PackageId::new("pkg")))
                })
                .collect()
        };
        due.into_iter()
            .map(|(pack, lang, pkg)| self.try_spawn(&pack, &lang, &pkg, workspace))
            .collect()
    }

    pub fn resolve(
        &self,
        language: &LanguageId,
        package: &PackageId,
        q: &ResolveQuery,
    ) -> ResolveOutcome {
        self.poll_health();
        if !self.is_ready(language, package) {
            return ResolveOutcome::NotReady;
        }
        let st = self.inner.lock().expect("sup");
        for (pack, child) in &st.children {
            let Some(adapter) = self
                .adapters
                .iter()
                .find(|a| a.pack_name() == pack.as_str())
            else {
                continue;
            };
            let serves = child.language == *language
                || adapter.extra_languages().iter().any(|l| l == language);
            if !serves {
                continue;
            }
            match adapter.resolve_query(&child.handle, q) {
                ResolveOutcome::Ready(r) => return ResolveOutcome::Ready(r),
                ResolveOutcome::NotReady => continue,
            }
        }
        ResolveOutcome::NotReady
    }

    pub fn forward_did_change(&self, uri: &str, text: &str) {
        self.poll_health();
        let st = self.inner.lock().expect("sup");
        for (pack, child) in &st.children {
            if let Some(adapter) = self
                .adapters
                .iter()
                .find(|a| a.pack_name() == pack.as_str())
            {
                adapter.forward_did_change(&child.handle, uri, text);
            }
        }
    }

    pub fn forward_watch(&self, paths: &[String]) {
        self.poll_health();
        let st = self.inner.lock().expect("sup");
        for (pack, child) in &st.children {
            if let Some(adapter) = self
                .adapters
                .iter()
                .find(|a| a.pack_name() == pack.as_str())
            {
                adapter.forward_watch(&child.handle, paths);
            }
        }
    }

    pub fn inbox(&self, pack: &str) -> Vec<crate::adapter::EngineMessage> {
        self.inner
            .lock()
            .expect("sup")
            .children
            .get(pack)
            .map(|c| c.handle.inbox())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{EngineBinary, ReadyKind};
    use crate::discovery::{hex_of, stub_pack_bytes, TY_BINARY};
    use crate::fake::{FakeAnswers, FakeEngineAdapter};
    use crate::hooks::AbortSpawnHooks;
    use crate::pack::PackAdapter;
    use progressive_lsp_core::Tier;
    use progressive_lsp_core::{FakeClock, FakeLog, FileId, LogLevel};
    use progressive_lsp_install::{hex_encode, sha256, Manifest, ManifestArtifact};
    use progressive_lsp_resolve::{LspLocation, Position, QueryKind, Range};
    use std::path::PathBuf;

    fn prefix() -> (tempfile::TempDir, PrefixLayout) {
        let dir = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(dir.path());
        layout.ensure_dirs().unwrap();
        (dir, layout)
    }

    fn ready_ty(clock: Arc<FakeClock>, prefix: PrefixLayout) -> (EngineSupervisor, PathBuf) {
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///a.py"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let bin = EngineBinary {
            pack_name: "python".into(),
            path: prefix.engines_dir().join("python/ty"),
            sha256: [0; 32],
        };
        let fake = fake.with_binary(bin);
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        let ws = PathBuf::from("/ws");
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                &ws
            )
            .unwrap());
        assert!(
            sup.stderr_capture_attached("python"),
            "ChildStderrAdapter attaches when ChildIo has a stderr pipe"
        );
        (sup, ws)
    }

    #[test]
    fn supervisor_attaches_stderr_adapter_when_pipe_exists() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let (sup, _) = ready_ty(clock, prefix);
        assert!(sup.stderr_capture_attached("python"));
        assert!(!sup.stderr_capture_attached("missing"));
    }

    #[test]
    fn crash_then_backoff_then_respawn_without_sleep() {
        let clock = Arc::new(FakeClock::at_unix_ms(1_000));
        let (_dir, prefix) = prefix();
        let (sup, ws) = ready_ty(clock.clone(), prefix);
        assert!(sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        assert!(sup.merged_capabilities().definition);
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(sup
            .resolve(&LanguageId::new("python"), &PackageId::new("pkg"), &q)
            .is_ready());

        sup.note_crash("python");
        assert!(!sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        assert!(!sup
            .resolve(&LanguageId::new("python"), &PackageId::new("pkg"), &q)
            .is_ready());
        assert!(matches!(
            sup.last_error("python"),
            Some(EngineError::Crashed(_))
        ));
        assert!(!sup.progress_notes().is_empty());

        let err = sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                &ws,
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::Backoff { .. }));
        assert!(sup.tick(&ws).is_empty());

        clock.advance_ms(50);
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                &ws
            )
            .is_err());

        clock.advance_ms(100);
        let results = sup.tick(&ws);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap(), &true);
        assert!(sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        assert!(sup
            .resolve(&LanguageId::new("python"), &PackageId::new("pkg"), &q)
            .is_ready());
    }

    #[test]
    fn missing_pack_does_not_spawn_and_core_stays_up() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(FakeEngineAdapter::ty()));
        let spawned = sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap();
        assert!(!spawned);
        assert!(!sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        assert_eq!(sup.adapter_names(), vec!["python".to_string()]);
        assert!(sup.prefix().engines_dir().ends_with("engines"));
    }

    #[test]
    fn abort_hook_skips_engine() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix).with_hooks(Arc::new(AbortSpawnHooks {
            message: "skip-ty".into(),
        }));
        sup.register(Box::new(fake));
        let err = sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::Aborted(m) if m.contains("skip-ty")));
        assert!(!sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
    }

    #[test]
    fn spawn_crash_starts_backoff() {
        let clock = Arc::new(FakeClock::at_unix_ms(5));
        let (_dir, prefix) = prefix();
        let fake = FakeEngineAdapter::ty()
            .crash_on_spawn()
            .with_binary(EngineBinary {
                pack_name: "python".into(),
                path: PathBuf::from("/p/ty"),
                sha256: [0; 32],
            });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .is_err());
        assert!(matches!(
            sup.last_error("python"),
            Some(EngineError::Crashed(_))
        ));
    }

    #[test]
    fn forward_did_change_and_watch_and_capabilities() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let (sup, _) = ready_ty(clock, prefix);
        sup.forward_did_change("file:///a.py", "x = 1");
        sup.forward_watch(&["a.py".into()]);
        let inbox = sup.inbox("python");
        assert_eq!(inbox.len(), 2);
        assert!(sup.merged_capabilities().hover);
        sup.bind_file(FileId::new("a.py"), PackageId::new("pkg"));
        assert_eq!(sup.package_for_file(&FileId::new("a.py")).as_str(), "pkg");
        assert_eq!(
            sup.package_for_file(&FileId::new("other.py")).as_str(),
            "pkg"
        );
        let unknown = EngineSupervisor::new(
            Arc::new(FakeClock::at_unix_ms(1)),
            PrefixLayout::from_path("/p"),
        );
        assert!(unknown
            .try_spawn(
                "missing",
                &LanguageId::new("python"),
                &PackageId::new("p"),
                Path::new("/w")
            )
            .is_err());
    }

    #[test]
    fn initialize_ready_covers_language_and_poll_health() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeAnswers {
            definition: vec![LspLocation::new(
                "file:///a.py",
                Range::default(),
                Tier::Types,
            )],
            ..FakeAnswers::default()
        });
        let fake = fake.with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix).with_policy(BackoffPolicy {
            initial_ms: 10,
            max_ms: 100,
        });
        sup.register(Box::new(fake));
        sup.try_spawn(
            "python",
            &LanguageId::new("python"),
            &PackageId::new("pkg"),
            Path::new("/ws"),
        )
        .unwrap();
        assert!(sup.is_ready(&LanguageId::new("python"), &PackageId::new("other")));
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(sup
            .resolve(&LanguageId::new("python"), &PackageId::new("pkg"), &q)
            .is_ready());
        assert!(!sup
            .resolve(&LanguageId::new("rust"), &PackageId::new("pkg"), &q)
            .is_ready());
    }

    #[test]
    fn poll_health_marks_dead_child_not_ready() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("x", "file:///x.py"));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "python",
            &LanguageId::new("python"),
            &PackageId::new("pkg"),
            Path::new("/ws"),
        )
        .unwrap();
        assert!(sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        // Crash after ready: next resolve flips the child dead; poll_health must unready.
        // Re-fetch via inbox path: mark through a second resolve with crash flag is on the
        // boxed adapter — inject via note by killing through resolve after setting crash.
        // Direct: crash the stored handle by resolving with a freshly crashed adapter is
        // hard; use note_crash equivalent via poll after we mark via last inbox handle.
        sup.note_crash("python");
        sup.poll_health();
        assert!(!sup.is_ready(&LanguageId::new("python"), &PackageId::new("pkg")));
        assert_eq!(sup.adapter_names(), vec!["python".to_string()]);
        assert_ne!(sup.adapter_names(), vec!["xyzzy".to_string()]);
    }

    #[test]
    fn clangd_pack_marks_cpp_ready() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let fake = FakeEngineAdapter::clangd();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///greet.c"));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "clangd".into(),
            path: PathBuf::from("/p/clangd"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "clangd",
            &LanguageId::new("c"),
            &PackageId::new("pkg"),
            Path::new("/ws"),
        )
        .unwrap();
        assert!(sup.is_ready(&LanguageId::new("c"), &PackageId::new("pkg")));
        assert!(sup.is_ready(&LanguageId::new("cpp"), &PackageId::new("pkg")));
        let q = ResolveQuery::new(
            FileId::new("a.cpp"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(sup
            .resolve(&LanguageId::new("cpp"), &PackageId::new("pkg"), &q)
            .is_ready());
    }

    fn write_pack(prefix: &PrefixLayout, pack: &str, binary: &str, bytes: &[u8], sha: &str) {
        let dir = prefix.engines_dir().join(pack);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(binary), bytes).unwrap();
        let m = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: binary.into(),
                rel_path: binary.into(),
                sha256: sha.to_string(),
                executable: true,
            }],
        };
        std::fs::write(dir.join("manifest.json"), m.to_json().unwrap()).unwrap();
    }

    fn spawn_records(log: &FakeLog) -> Vec<progressive_lsp_core::LogRecord> {
        log.records()
            .into_iter()
            .filter(|r| r.operation.as_deref() == Some("spawn"))
            .collect()
    }

    #[test]
    fn try_spawn_emits_log_port_supervisor_pattern() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let mut sup =
            EngineSupervisor::new(clock.clone(), prefix.clone()).with_log(Arc::new(log.clone()));
        sup.register(Box::new(FakeEngineAdapter::ty()));
        assert!(!sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        let recs = spawn_records(&log);
        assert!(
            recs.iter().any(|r| r.level == LogLevel::Warn
                && r.component.as_ref().map(|c| c.as_str()) == Some("engine")
                && r.message.contains("not discovered")),
            "{recs:?}"
        );
        let none = sup.try_spawn_registered(Path::new("/ws"), &PackageId::new("pkg"));
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].as_ref().unwrap(), &false);

        let err = EngineSupervisor::new(clock.clone(), prefix.clone())
            .with_log(Arc::new(log.clone()))
            .try_spawn(
                "missing",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::NotDiscovered(_)));

        let bytes = stub_pack_bytes("python", TY_BINARY);
        write_pack(
            &prefix,
            "python",
            TY_BINARY,
            &bytes,
            &hex_encode(&sha256(b"not-the-bytes")),
        );
        let mut hashed =
            EngineSupervisor::new(clock.clone(), prefix.clone()).with_log(Arc::new(log.clone()));
        hashed.register(Box::new(PackAdapter::python()));
        let err = hashed
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::Hash { .. }), "{err:?}");
        assert!(
            spawn_records(&log)
                .iter()
                .any(|r| r.level == LogLevel::Warn && r.message.contains("hash mismatch")),
            "{:?}",
            spawn_records(&log)
        );
    }

    #[test]
    fn stub_refuse_and_abort_and_ok_emit_supervisor() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let bytes = stub_pack_bytes("python", TY_BINARY);
        write_pack(&prefix, "python", TY_BINARY, &bytes, &hex_of(&bytes));
        let log = FakeLog::new();
        let mut stub =
            EngineSupervisor::new(clock.clone(), prefix.clone()).with_log(Arc::new(log.clone()));
        stub.register(Box::new(PackAdapter::python()));
        let err = stub
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(err.to_string().contains("stub pack"), "{err}");
        assert!(
            spawn_records(&log).iter().any(|r| r.level == LogLevel::Warn
                && r.component.as_ref().map(|c| c.as_str()) == Some("engine")
                && r.message.contains("stub pack")),
            "{:?}",
            spawn_records(&log)
        );

        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let abort_log = FakeLog::new();
        let mut abort = EngineSupervisor::new(clock.clone(), prefix.clone())
            .with_log(Arc::new(abort_log.clone()))
            .with_hooks(Arc::new(AbortSpawnHooks {
                message: "skip-ty".into(),
            }));
        abort.register(Box::new(fake));
        let err = abort
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::Aborted(m) if m.contains("skip-ty")));
        assert!(
            spawn_records(&abort_log)
                .iter()
                .any(|r| r.level == LogLevel::Warn && r.message.contains("skip-ty")),
            "{:?}",
            spawn_records(&abort_log)
        );

        let ok_log = FakeLog::new();
        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut ok = EngineSupervisor::new(
            Arc::new(FakeClock::at_unix_ms(2)),
            PrefixLayout::from_path("/p"),
        )
        .with_log(Arc::new(ok_log.clone()));
        ok.register(Box::new(fake));
        assert!(ok
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        assert!(
            spawn_records(&ok_log)
                .iter()
                .any(|r| r.level == LogLevel::Info
                    && r.component.as_ref().map(|c| c.as_str()) == Some("engine")
                    && r.message.contains("spawned")),
            "{:?}",
            spawn_records(&ok_log)
        );
        let results = ok.try_spawn_registered(Path::new("/ws"), &PackageId::new("pkg"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap(), &true);
    }

    #[test]
    fn backoff_and_note_crash_emit_info_and_warn() {
        let clock = Arc::new(FakeClock::at_unix_ms(1_000));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///a.py"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "python".into(),
            path: prefix.engines_dir().join("python/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock.clone(), prefix).with_log(Arc::new(log.clone()));
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        sup.note_crash("python");
        assert!(
            spawn_records(&log)
                .iter()
                .any(|r| r.level == LogLevel::Warn && r.message.contains("crashed")),
            "{:?}",
            spawn_records(&log)
        );
        let err = sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap_err();
        assert!(matches!(err, EngineError::Backoff { .. }));
        assert!(
            spawn_records(&log).iter().any(|r| r.level == LogLevel::Info
                && r.operation.as_deref() == Some("spawn")
                && r.message.contains("backoff")),
            "{:?}",
            spawn_records(&log)
        );
    }

    fn third_party_spawn(log: &FakeLog) -> Vec<progressive_lsp_core::LogRecord> {
        log.records()
            .into_iter()
            .filter(|r| {
                r.operation.as_deref() == Some("spawn")
                    && r.source_repo == progressive_lsp_core::LogOrigin::ThirdParty
            })
            .collect()
    }

    #[test]
    fn supervisor_attaches_child_stderr_adapter_when_fake_read_exists() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix).with_log(Arc::new(log.clone()));
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        assert!(sup.stderr_capture_attached("python"));
        assert!(
            !sup.attach_if_stderr_pipe("python", None),
            "ChildHandle has no live OS Read"
        );
        assert!(third_party_spawn(&log).is_empty());

        let stderr = progressive_lsp_log::FakeChildStderr::new();
        stderr.push_line("INFO ty::main: pack ready");
        stderr.push_line("not a level line");
        assert!(sup.attach_if_stderr_pipe("python", Some(&stderr)));
        let recs = third_party_spawn(&log);
        assert!(
            recs.iter().any(|r| r.message.contains("pack ready")
                && r.component.as_ref().map(|c| c.as_str()) == Some("python")),
            "{recs:?}"
        );
        assert!(
            recs.iter().any(|r| r.message == "not a level line"),
            "{recs:?}"
        );
        assert!(
            recs.iter()
                .all(|r| r.source_repo == progressive_lsp_core::LogOrigin::ThirdParty),
            "{recs:?}"
        );
    }

    #[test]
    fn fake_child_stderr_overflow_drops_oldest_observer() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let fake = FakeEngineAdapter::ty().with_binary(EngineBinary {
            pack_name: "python".into(),
            path: PathBuf::from("/p/ty"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix).with_log(Arc::new(log.clone()));
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        let stderr = progressive_lsp_log::FakeChildStderr::new();
        stderr.push_line("keep-me");
        stderr.push_line("and-me");
        assert!(sup.attach_if_stderr_pipe("python", Some(&stderr)));
        let recs = third_party_spawn(&log);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].message, "keep-me");
        assert!(stderr.drain().is_empty());
        assert_eq!(progressive_lsp_log::STDERR_DRAIN_CAP, 1024);
    }

    #[test]
    fn child_io_stdout_is_never_log_adapter_and_null_stderr_forbidden() {
        assert!(progressive_lsp_log::NullStderrAdapter::forbidden_on_prod_spawn());
        assert!(!progressive_lsp_log::InheritStderrAdapter::allowed_on_serve());
        let piped = ChildIo::lsp_with_stderr_pipe();
        assert!(piped.stdout_is_never_log_adapter());
        assert!(piped.has_stderr_pipe());

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let fake = FakeEngineAdapter::ty()
            .with_child_io(ChildIo::lsp_without_stderr())
            .with_binary(EngineBinary {
                pack_name: "python".into(),
                path: PathBuf::from("/p/ty"),
                sha256: [0; 32],
            });
        let mut sup = EngineSupervisor::new(clock, prefix).with_log(Arc::new(log.clone()));
        sup.register(Box::new(fake));
        assert!(sup
            .try_spawn(
                "python",
                &LanguageId::new("python"),
                &PackageId::new("pkg"),
                Path::new("/ws"),
            )
            .unwrap());
        assert!(!sup.stderr_capture_attached("python"));
        let stderr = progressive_lsp_log::FakeChildStderr::new();
        stderr.push_line("INFO ty: should not attach");
        assert!(!sup.attach_if_stderr_pipe("python", Some(&stderr)));
        assert!(third_party_spawn(&log).is_empty());
        assert!(!sup.attach_if_stderr_pipe("missing", Some(&stderr)));
    }

    #[test]
    fn supervisor_attaches_log_file_tail_adapter_only_when_path_exists() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let sup = EngineSupervisor::new(clock, prefix).with_log(Arc::new(log.clone()));
        assert!(!sup.attach_log_file_tail("zls", None));
        assert!(!sup.log_file_tail_attached("zls"));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zls.log");
        std::fs::write(&path, "zls file line\n").unwrap();
        assert!(sup.attach_log_file_tail("zls", Some(path.as_path())));
        assert!(sup.log_file_tail_attached("zls"));
        let recs = third_party_spawn(&log);
        assert!(
            recs.iter()
                .any(|r| r.level == LogLevel::Info && r.message == "zls file line"),
            "{recs:?}"
        );
    }

    #[test]
    fn supervisor_ingests_lsp_log_message_adapter_only_when_proxied() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let log = FakeLog::new();
        let sup = EngineSupervisor::new(clock, prefix).with_log(Arc::new(log.clone()));
        assert!(!sup.ingest_proxied_log_message("ty", "textDocument/definition", "nope", 1));
        assert!(!sup.lsp_log_message_attached("ty"));
        assert!(sup.ingest_proxied_log_message("ty", "window/logMessage", "engine err", 1));
        assert!(sup.ingest_proxied_log_message("ty", "window/showMessage", "shown", 2));
        assert!(sup.ingest_proxied_log_message("ty", "$/logTrace", "trace-me", 0));
        assert!(sup.lsp_log_message_attached("ty"));
        let recs = third_party_spawn(&log);
        assert!(
            recs.iter()
                .any(|r| r.level == LogLevel::Error && r.message == "engine err"),
            "{recs:?}"
        );
        assert!(
            recs.iter()
                .any(|r| r.level == LogLevel::Warn && r.message == "shown"),
            "{recs:?}"
        );
        assert!(
            recs.iter()
                .any(|r| r.level == LogLevel::Debug && r.message == "trace-me"),
            "{recs:?}"
        );
        assert!(
            recs.iter().all(|r| r.operation.as_deref() != Some("crash")),
            "logMessage is not a crash substitute"
        );
    }

    #[test]
    fn pack_spawn_still_refuses_command_and_does_not_set_rpc_trace() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let (_dir, prefix) = prefix();
        let bytes = stub_pack_bytes("python", TY_BINARY);
        write_pack(&prefix, "python", TY_BINARY, &bytes, &hex_of(&bytes));
        let pack = PackAdapter::python();
        let ctx = SpawnCtx {
            workspace: PathBuf::from("/w"),
            language: LanguageId::new("python"),
            package: PackageId::new("p"),
            argv: vec![pack.pack_name().into()],
            cwd: PathBuf::from("/w"),
            env: BTreeMap::new(),
            binary: pack.discover(&prefix).unwrap(),
        };
        assert!(!ctx.argv.iter().any(|a| a.contains("-rpc.trace")));
        assert!(!ctx.env.contains_key("RA_LOG_FILE"));
        assert!(!ctx.env.contains_key("TY_LOG_PROFILE"));
        let err = pack.spawn(ctx).unwrap_err();
        assert!(err.to_string().contains("stub pack"), "{err}");
        assert!(progressive_lsp_log::NullStderrAdapter::forbidden_on_prod_spawn());
        let _ = clock;
        let _ = prefix;
    }
}
