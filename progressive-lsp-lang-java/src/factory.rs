//! Java LanguageFactory. Produces grammar id + T1/T2 resolver chain.

use std::sync::Arc;

use progressive_lsp_core::{LanguageId, T2Backend};
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
}

impl JavaLanguageFactory {
    pub fn new() -> Self {
        Self {
            index: None,
            graph: None,
            t2: T2Strategy::default_heuristic(),
        }
    }

    pub fn with_index(index: Arc<dyn SymbolIndex>) -> Self {
        Self {
            index: Some(index),
            graph: None,
            t2: T2Strategy::default_heuristic(),
        }
    }

    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            index: Some(graph.clone()),
            graph: Some(graph),
            t2: T2Strategy::default_heuristic(),
        }
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
            return self.bind_t2(g.clone());
        }
        match &self.index {
            Some(idx) => self.bind(idx.clone()),
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
        assert_eq!(t2.bind_t2(Arc::new(progressive_lsp_resolve::EmptyIndex)).len(), 2);
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
        use progressive_lsp_resolve::{
            FakeResolver, LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery,
            Resolver,
        };
        use progressive_lsp_core::Tier;
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
}
