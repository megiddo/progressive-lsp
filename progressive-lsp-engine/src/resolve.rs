//! T3 resolver: Ready only when EngineSupervisor is ready for (language, package).

use std::sync::Arc;

use progressive_lsp_core::{LanguageId, PackageId};
use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery, Resolver};

use crate::supervisor::EngineSupervisor;

pub struct EngineResolver {
    supervisor: Arc<EngineSupervisor>,
    language: LanguageId,
    default_package: PackageId,
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
        }
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
        Self::new(supervisor, LanguageId::new("typescript"), PackageId::new("pkg"))
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

    fn package(&self, q: &ResolveQuery) -> PackageId {
        let bound = self.supervisor.package_for_file(&q.file);
        if bound.as_str() == "pkg" {
            self.default_package.clone()
        } else {
            bound
        }
    }
}

impl Resolver for EngineResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        let package = self.package(q);
        if !self.supervisor.is_ready(&self.language, &package) {
            return ResolveOutcome::NotReady;
        }
        self.supervisor.resolve(&self.language, &package, q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{EngineBinary, ReadyKind};
    use crate::fake::FakeEngineAdapter;
    use progressive_lsp_core::{FakeClock, FileId, PrefixLayout, Tier};
    use progressive_lsp_resolve::{
        FakeResolver, Position, QueryKind, Range, ResolveQuery, ResolverChain,
        TreeSitterResolver,
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
        let q = ResolveQuery::new(FileId::new("a.py"), Position::default(), QueryKind::Definition);
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
        assert!(!EngineResolver::phpantom(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::superhtml(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::biome(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::gopls(empty.clone()).resolve(&q).is_ready());
        assert!(!EngineResolver::zls(empty).resolve(&q).is_ready());
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
        let _ = TreeSitterResolver::new(std::sync::Arc::new(
            progressive_lsp_resolve::EmptyIndex,
        ));
    }
}
