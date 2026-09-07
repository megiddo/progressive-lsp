//! Java LanguageFactory. T3 EngineResolver when supervisor ready; T2 then T1.

use std::sync::Arc;

use progressive_lsp_core::{LanguageId, PackageId, T2Backend};
use progressive_lsp_engine::{EngineResolver, EngineSupervisor};
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphIndex, ResolverChain, SymbolIndex, T2Strategy, TreeSitterResolver,
};

use crate::{grammar_id, language_id};

#[derive(Clone)]
pub struct JavaLanguageFactory {
    index: Option<Arc<dyn SymbolIndex>>,
    graph: Option<Arc<dyn GraphIndex>>,
    t2: T2Strategy,
    supervisor: Option<Arc<EngineSupervisor>>,
}

impl JavaLanguageFactory {
    pub fn new() -> Self {
        Self {
            index: None,
            graph: None,
            t2: T2Strategy::default_heuristic(),
            supervisor: None,
        }
    }

    pub fn with_index(index: Arc<dyn SymbolIndex>) -> Self {
        Self {
            index: Some(index),
            graph: None,
            t2: T2Strategy::default_heuristic(),
            supervisor: None,
        }
    }

    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            index: Some(graph.clone()),
            graph: Some(graph),
            t2: T2Strategy::default_heuristic(),
            supervisor: None,
        }
    }

    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }

    pub fn with_t2(mut self, t2: T2Strategy) -> Self {
        self.t2 = t2;
        self
    }

    pub fn with_t2_backend(self, backend: T2Backend) -> Self {
        self.with_t2(T2Strategy::from_backend(backend))
    }

    pub fn t2_name(&self) -> &'static str {
        self.t2.backend_name()
    }

    pub fn bind(&self, index: Arc<dyn SymbolIndex>) -> ResolverChain {
        ResolverChain::new(vec![Box::new(TreeSitterResolver::new(index))])
    }

    pub fn bind_t2(&self, graph: Arc<dyn GraphIndex>) -> ResolverChain {
        ResolverChain::new(vec![
            self.t2.build(graph.clone()),
            Box::new(TreeSitterResolver::new(graph)),
        ])
    }

    fn t3_resolver(&self) -> Option<Box<dyn progressive_lsp_resolve::Resolver>> {
        self.supervisor.as_ref().map(|s| {
            Box::new(EngineResolver::new(
                s.clone(),
                language_id(),
                PackageId::new("pkg"),
            )) as Box<dyn progressive_lsp_resolve::Resolver>
        })
    }
}

impl Default for JavaLanguageFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageFactory for JavaLanguageFactory {
    fn language_id(&self) -> LanguageId {
        language_id()
    }

    fn grammar_id(&self) -> &str {
        grammar_id()
    }

    fn resolver_chain(&self) -> ResolverChain {
        if let Some(g) = &self.graph {
            return ResolverChain::with_tiers(
                self.t3_resolver(),
                Some(self.t2.build(g.clone())),
                Box::new(TreeSitterResolver::new(g.clone())),
            );
        }
        match &self.index {
            Some(idx) => ResolverChain::with_tiers(
                self.t3_resolver(),
                None,
                Box::new(TreeSitterResolver::new(idx.clone())),
            ),
            None => ResolverChain::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_resolve::Resolver;

    #[test]
    fn factory_ids_and_empty_chain_without_index() {
        let f = JavaLanguageFactory::new();
        assert_eq!(f.language_id().as_str(), "java");
        assert_eq!(f.grammar_id(), "tree-sitter-java");
        assert!(f.resolver_chain().is_empty());
        let d = JavaLanguageFactory::default();
        assert!(d.resolver_chain().is_empty());
    }

    #[test]
    fn factory_with_index_builds_t1_chain() {
        let idx: Arc<dyn SymbolIndex> = Arc::new(progressive_lsp_resolve::EmptyIndex);
        let f = JavaLanguageFactory::with_index(idx.clone());
        assert_eq!(f.resolver_chain().len(), 1);
        let chain = f.bind(idx);
        assert_eq!(chain.len(), 1);
        let q = progressive_lsp_resolve::ResolveQuery::workspace_symbol("x");
        assert!(chain.resolve(&q).is_ready());
        let g: Arc<dyn GraphIndex> = Arc::new(progressive_lsp_resolve::EmptyIndex);
        let t2 = JavaLanguageFactory::with_graph(g);
        assert_eq!(t2.resolver_chain().len(), 2);
        assert_eq!(t2.t2_name(), "heuristic");
        assert_eq!(
            t2.bind_t2(Arc::new(progressive_lsp_resolve::EmptyIndex))
                .len(),
            2
        );
    }

    #[test]
    fn factory_selects_stack_graphs_strategy_from_backend() {
        let g: Arc<dyn GraphIndex> = Arc::new(progressive_lsp_resolve::EmptyIndex);
        let f = JavaLanguageFactory::with_graph(g).with_t2_backend(T2Backend::StackGraphs);
        assert_eq!(f.t2_name(), "stack-graphs");
        assert_eq!(f.resolver_chain().len(), 2);
    }

    #[test]
    fn factory_injects_fake_t2() {
        use progressive_lsp_core::FileId;
        use progressive_lsp_core::Tier;
        use progressive_lsp_resolve::{
            FakeResolver, LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery,
            Resolver,
        };
        let fake = FakeResolver::graph("injected-java").with_location(LspLocation::new(
            "file:///injected",
            Range::default(),
            Tier::Graph,
        ));
        let g: Arc<dyn GraphIndex> = Arc::new(progressive_lsp_resolve::EmptyIndex);
        let f = JavaLanguageFactory::with_graph(g)
            .with_t2(progressive_lsp_resolve::T2Strategy::inject(Arc::new(fake)));
        assert_eq!(f.t2_name(), "injected");
        match f.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => assert_eq!(r.locations[0].uri, "file:///injected"),
            ResolveOutcome::NotReady => panic!("injected T2"),
        }
    }

    #[test]
    fn java_t3_when_fake_engine_ready_else_t2() {
        use progressive_lsp_core::{FakeClock, FileId, PrefixLayout, Tier};
        use progressive_lsp_engine::{
            EngineBinary, EngineSupervisor, FakeEngineAdapter, ReadyKind,
        };
        use progressive_lsp_index::{IndexService, SharedIndex};
        use progressive_lsp_resolve::{
            Position, QueryKind, ResolveOutcome, ResolveQuery, Resolver,
        };
        use std::path::PathBuf;

        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let tmp = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(tmp.path());
        prefix.ensure_dirs().unwrap();
        let fake = FakeEngineAdapter::java();
        fake.set_answers(FakeEngineAdapter::typed_fixture("App", "file:///App.java"));
        fake.set_ready_kind(ReadyKind::IndexedPackage(PackageId::new("pkg")));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "java".into(),
            path: PathBuf::from("/p/javacs"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "java",
            &LanguageId::new("java"),
            &PackageId::new("pkg"),
            PathBuf::from("/ws").as_path(),
        )
        .unwrap();
        let factory =
            JavaLanguageFactory::with_graph(Arc::new(SharedIndex::new(IndexService::new())))
                .with_supervisor(Arc::new(sup));
        assert_eq!(factory.resolver_chain().len(), 3);
        match factory.resolver_chain().resolve(&ResolveQuery::new(
            FileId::new("Main.java"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.iter().any(|l| l.uri.contains("App.java")));
            }
            other => panic!("{other:?}"),
        }
        let t2_only =
            JavaLanguageFactory::with_graph(Arc::new(SharedIndex::new(IndexService::new())));
        assert_eq!(t2_only.resolver_chain().len(), 2);
    }
}
