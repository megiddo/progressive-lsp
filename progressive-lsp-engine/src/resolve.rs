//! T3 resolver: Ready only when EngineSupervisor is ready for (language, package).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use progressive_lsp_core::{
    LanguageId, LogComponent, LogLevel, LogPort, LogRecord, LogScope, NullLog, PackageId,
};
use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery, Resolver};

use crate::supervisor::EngineSupervisor;

pub struct EngineResolver {
    supervisor: Arc<EngineSupervisor>,
    language: LanguageId,
    default_package: PackageId,
    log: Arc<dyn LogPort>,
    skipped: Mutex<HashSet<(String, String)>>,
    use_file_language: bool,
}

impl EngineResolver {
    pub fn new(
        supervisor: Arc<EngineSupervisor>,
        language: LanguageId,
        default_package: PackageId,
    ) -> Self {
        Self {
            supervisor,
            language,
            default_package,
            log: Arc::new(NullLog),
            skipped: Mutex::new(HashSet::new()),
            use_file_language: false,
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    /// Session chain: pick language from the query file so one Adapter covers all packs.
    pub fn with_file_language(mut self) -> Self {
        self.use_file_language = true;
        self
    }

    pub fn python(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("python"), PackageId::new("pkg"))
    }

    pub fn rust(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("rust"), PackageId::new("pkg"))
    }

    pub fn clangd(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("c"), PackageId::new("pkg"))
    }

    pub fn tsgo(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(
            supervisor,
            LanguageId::new("typescript"),
            PackageId::new("pkg"),
        )
    }

    pub fn phpantom(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("php"), PackageId::new("pkg"))
    }

    pub fn superhtml(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("html"), PackageId::new("pkg"))
    }

    pub fn biome(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("css"), PackageId::new("pkg"))
    }

    pub fn gopls(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("go"), PackageId::new("pkg"))
    }

    pub fn zls(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("zig"), PackageId::new("pkg"))
    }

    pub fn java(supervisor: Arc<EngineSupervisor>) -> Self {
        Self::new(supervisor, LanguageId::new("java"), PackageId::new("pkg"))
    }

    fn package(&self, q: &ResolveQuery) -> PackageId {
        let bound = self.supervisor.package_for_file(&q.file);
        if bound.as_str() == "pkg" {
            self.default_package.clone()
        } else {
            bound
        }
    }

    fn language_of(&self, q: &ResolveQuery) -> LanguageId {
        if self.use_file_language {
            language_from_file(q.file.as_str()).unwrap_or_else(|| self.language.clone())
        } else {
            self.language.clone()
        }
    }

    fn note_skip(&self, language: &LanguageId, package: &PackageId) {
        let key = (language.as_str().to_string(), package.as_str().to_string());
        {
            let mut skipped = self.skipped.lock().expect("skip");
            if !skipped.insert(key) {
                return;
            }
        }
        let _g = LogScope::enter(
            LogScope::new()
                .operation("resolve")
                .component(LogComponent::engine()),
        );
        self.log.info(&format!(
            "pack skipped ({}, {}): EngineNotReady",
            language.as_str(),
            package.as_str()
        ));
    }

    fn emit_t3_timing(
        &self,
        q: &ResolveQuery,
        started: Instant,
        outcome: &str,
        location_count: usize,
        budget: Option<Duration>,
    ) {
        if !matches!(
            q.kind,
            QueryKind::Definition
                | QueryKind::Implementation
                | QueryKind::References
                | QueryKind::TypeDefinition
        ) {
            return;
        }
        let t3_try_ms = started.elapsed().as_millis() as u64;
        let _g = LogScope::enter(
            LogScope::new()
                .operation("engine_resolve")
                .component(LogComponent::engine())
                .path(q.file.as_str())
                .line(q.position.line),
        );
        let mut extras = BTreeMap::new();
        extras.insert("t3_try_ms".into(), t3_try_ms.to_string());
        extras.insert("t3_outcome".into(), outcome.into());
        extras.insert("query_kind".into(), q.kind.as_str().to_string());
        extras.insert("location_count".into(), location_count.to_string());
        if let Some(b) = budget {
            extras.insert("t3_budget_ms".into(), b.as_millis().to_string());
        }
        let mut rec = LogRecord::at_caller(LogLevel::Info, "engine_resolve");
        rec.message = format!(
            "engine_resolve {outcome} t3_try_ms={t3_try_ms} kind={} locations={location_count}",
            q.kind.as_str()
        );
        rec.extras = Some(extras);
        self.log.emit(rec);
    }
}

fn language_from_file(path: &str) -> Option<LanguageId> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let id = match ext {
        "java" => "java",
        "php" => "php",
        "html" | "htm" => "html",
        "css" => "css",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "go" => "go",
        "zig" => "zig",
        "py" => "python",
        "rs" => "rust",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        _ => return None,
    };
    Some(LanguageId::new(id))
}

impl Resolver for EngineResolver {
    /// Language-factory chains only. Serve mux uses T3′ + background builder (REQ-NFR-1.3).
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        let language = self.language_of(q);
        let package = self.package(q);
        if !self.supervisor.is_ready(&language, &package) {
            self.note_skip(&language, &package);
            return ResolveOutcome::NotReady;
        }
        if !self.supervisor.engine_session_warmed(&language) {
            self.emit_t3_timing(q, Instant::now(), "skip_unwarmed", 0, None);
            return ResolveOutcome::NotReady;
        }
        let started = Instant::now();
        let outcome = self.supervisor.resolve(&language, &package, q);
        let (tag, locs) = match &outcome {
            ResolveOutcome::Ready(r) => ("ready", r.locations.len()),
            ResolveOutcome::NotReady => ("not_ready", 0),
        };
        self.emit_t3_timing(q, started, tag, locs, None);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{EngineBinary, ReadyKind};
    use crate::fake::FakeEngineAdapter;
    use progressive_lsp_core::{FakeClock, FileId, PrefixLayout, Tier};
    use progressive_lsp_resolve::{
        FakeResolver, Position, QueryKind, Range, ResolveQuery, ResolverChain, TreeSitterResolver,
    };
    use std::path::PathBuf;

    fn ready_sup() -> Arc<EngineSupervisor> {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        std::mem::forget(dir);
        let fake = FakeEngineAdapter::ty();
        fake.set_answers(FakeEngineAdapter::typed_fixture("greet", "file:///a.py"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
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
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        Arc::new(sup)
    }

    #[test]
    fn t3_when_ready_else_t1() {
        let sup = ready_sup();
        let t3 = EngineResolver::python(sup.clone());
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        match t3.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert_eq!(r.locations[0].uri, "file:///a.py");
            }
            other => panic!("{other:?}"),
        }
        match t3.resolve(&ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Hover,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.hover.unwrap().signature(), "greet: int"),
            other => panic!("{other:?}"),
        }
        let rust = EngineResolver::rust(sup);
        assert!(!rust.resolve(&q).is_ready());
        let empty = Arc::new(EngineSupervisor::new(
            Arc::new(FakeClock::at_unix_ms(1)),
            PrefixLayout::from_path("/tmp/no-m4"),
        ));
        assert!(!EngineResolver::clangd(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::tsgo(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::phpantom(empty.clone())
            .resolve(&q)
            .is_ready());
        assert!(!EngineResolver::superhtml(empty.clone())
            .resolve(&q)
            .is_ready());
        assert!(!EngineResolver::biome(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::gopls(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::zls(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::java(empty).resolve(&q).is_ready());
    }

    #[test]
    fn not_ready_falls_to_t1() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let prefix = PrefixLayout::from_path("/tmp/no-engines");
        let sup = Arc::new(EngineSupervisor::new(clock, prefix));
        let chain = ResolverChain::with_tiers(
            Some(Box::new(EngineResolver::python(sup))),
            None,
            Box::new(FakeResolver::syntax("t1").with_location(
                progressive_lsp_resolve::LspLocation::new(
                    "file:///t1",
                    Range::default(),
                    Tier::Syntax,
                ),
            )),
        );
        match chain.resolve(&ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                assert_eq!(r.locations[0].uri, "file:///t1");
            }
            other => panic!("{other:?}"),
        }
        let _ = TreeSitterResolver::new(std::sync::Arc::new(progressive_lsp_resolve::EmptyIndex));
    }

    #[test]
    fn skip_once_per_language_package_does_not_fail_user() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let prefix = PrefixLayout::from_path("/tmp/no-engines-skip");
        let sup = Arc::new(EngineSupervisor::new(clock, prefix));
        let log = progressive_lsp_core::FakeLog::new();
        let t3 = EngineResolver::python(sup).with_log(Arc::new(log.clone()));
        let chain = ResolverChain::with_tiers(
            Some(Box::new(t3)),
            None,
            Box::new(FakeResolver::syntax("t1").with_location(
                progressive_lsp_resolve::LspLocation::new(
                    "file:///t1",
                    Range::default(),
                    Tier::Syntax,
                ),
            )),
        );
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                assert_eq!(r.locations[0].uri, "file:///t1");
            }
            other => panic!("{other:?}"),
        }
        let skip_after_first = log
            .records()
            .iter()
            .filter(|r| {
                r.level == progressive_lsp_core::LogLevel::Info
                    && r.operation.as_deref() == Some("resolve")
                    && r.component.as_ref().map(|c| c.as_str()) == Some("engine")
                    && r.message.contains("pack skipped")
            })
            .count();
        assert_eq!(
            skip_after_first,
            1,
            "first skip must emit: {:?}",
            log.records()
        );
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => assert_eq!(r.tier, Tier::Syntax),
            other => panic!("{other:?}"),
        }
        let skips: Vec<_> = log
            .records()
            .into_iter()
            .filter(|r| {
                r.level == progressive_lsp_core::LogLevel::Info
                    && r.operation.as_deref() == Some("resolve")
                    && r.component.as_ref().map(|c| c.as_str()) == Some("engine")
                    && r.message.contains("pack skipped")
            })
            .collect();
        assert_eq!(skips.len(), 1, "{skips:?}");
        assert!(skips[0].message.contains("python"));
        for (path, lang) in [
            ("a.java", "java"),
            ("a.php", "php"),
            ("a.html", "html"),
            ("a.htm", "html"),
            ("a.css", "css"),
            ("a.js", "javascript"),
            ("a.mjs", "javascript"),
            ("a.cjs", "javascript"),
            ("a.ts", "typescript"),
            ("a.go", "go"),
            ("a.zig", "zig"),
            ("a.py", "python"),
            ("a.rs", "rust"),
            ("a.c", "c"),
            ("a.h", "c"),
            ("a.cc", "cpp"),
            ("a.cpp", "cpp"),
            ("a.cxx", "cpp"),
            ("a.hpp", "cpp"),
            ("a.hh", "cpp"),
            ("a.cs", "csharp"),
        ] {
            assert_eq!(language_from_file(path).unwrap().as_str(), lang);
        }
        assert!(language_from_file("nopath").is_none());
        assert!(language_from_file("x.unknown").is_none());
    }

    #[test]
    fn file_language_skip_uses_extension_not_constructor_language() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let prefix = PrefixLayout::from_path("/tmp/no-engines-file-lang");
        let sup = Arc::new(EngineSupervisor::new(clock, prefix));
        let log = progressive_lsp_core::FakeLog::new();
        let t3 = EngineResolver::new(sup, LanguageId::new("java"), PackageId::new("pkg"))
            .with_log(Arc::new(log.clone()))
            .with_file_language();
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(!t3.resolve(&q).is_ready());
        assert!(!t3.resolve(&q).is_ready());
        let skips: Vec<_> = log
            .records()
            .into_iter()
            .filter(|r| r.message.contains("pack skipped"))
            .collect();
        assert_eq!(skips.len(), 1, "{skips:?}");
        assert!(skips[0].message.contains("python"));
        assert!(!skips[0].message.contains("java"));
    }

    #[test]
    fn skip_uses_bound_package_not_default_pkg() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let prefix = PrefixLayout::from_path("/tmp/no-engines-bound-pkg");
        let sup = EngineSupervisor::new(clock, prefix);
        sup.bind_file(FileId::new("a.py"), PackageId::new("other"));
        let sup = Arc::new(sup);
        let log = progressive_lsp_core::FakeLog::new();
        let t3 = EngineResolver::new(sup, LanguageId::new("python"), PackageId::new("custom"))
            .with_log(Arc::new(log.clone()));
        let q = ResolveQuery::new(
            FileId::new("a.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(!t3.resolve(&q).is_ready());
        let msg = log
            .records()
            .into_iter()
            .find(|r| r.message.contains("pack skipped"))
            .map(|r| r.message)
            .expect("skip row");
        assert!(msg.contains("other"), "{msg}");
        assert!(!msg.contains("custom"), "{msg}");

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let prefix = PrefixLayout::from_path("/tmp/no-engines-default-pkg");
        let sup = Arc::new(EngineSupervisor::new(clock, prefix));
        let log2 = progressive_lsp_core::FakeLog::new();
        let t3 = EngineResolver::new(sup, LanguageId::new("python"), PackageId::new("custom"))
            .with_log(Arc::new(log2.clone()));
        let q = ResolveQuery::new(
            FileId::new("unbound.py"),
            Position::default(),
            QueryKind::Definition,
        );
        assert!(!t3.resolve(&q).is_ready());
        let msg = log2
            .records()
            .into_iter()
            .find(|r| r.message.contains("pack skipped"))
            .map(|r| r.message)
            .expect("skip row");
        assert!(msg.contains("custom"), "{msg}");
        assert!(!msg.contains("other"), "{msg}");
    }
}
