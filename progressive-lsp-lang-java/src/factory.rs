//! Java LanguageFactory. Produces grammar id + T1 resolver chain.

use std::sync::Arc;

use progressive_lsp_core::LanguageId;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{
    GraphIndex, HeuristicResolver, ResolverChain, SymbolIndex, TreeSitterResolver,
};

use crate::{grammar_id, language_id};

#[derive(Clone)]
pub struct JavaLanguageFactory {
    index: Option<Arc<dyn SymbolIndex>>,
    graph: Option<Arc<dyn GraphIndex>>,
}

impl JavaLanguageFactory {
    pub fn new() -> Self {
        Self {
            index: None,
            graph: None,
        }
    }

    pub fn with_index(index: Arc<dyn SymbolIndex>) -> Self {
        Self {
            index: Some(index),
            graph: None,
        }
    }

    pub fn with_graph(graph: Arc<dyn GraphIndex>) -> Self {
        Self {
            index: Some(graph.clone()),
            graph: Some(graph),
        }
    }

    pub fn bind(&self, index: Arc<dyn SymbolIndex>) -> ResolverChain {
        ResolverChain::new(vec![Box::new(TreeSitterResolver::new(index))])
    }

    pub fn bind_t2(&self, graph: Arc<dyn GraphIndex>) -> ResolverChain {
        ResolverChain::new(vec![
            Box::new(HeuristicResolver::new(graph.clone())),
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
        assert_eq!(t2.bind_t2(Arc::new(progressive_lsp_resolve::EmptyIndex)).len(), 2);
    }
}
