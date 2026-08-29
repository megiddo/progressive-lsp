//! Java LanguageFactory. Produces grammar id + T1 resolver chain.

use std::sync::Arc;

use progressive_lsp_core::LanguageId;
use progressive_lsp_plugin::LanguageFactory;
use progressive_lsp_resolve::{ResolverChain, SymbolIndex, TreeSitterResolver};

use crate::{grammar_id, language_id};

#[derive(Clone)]
pub struct JavaLanguageFactory {
    index: Option<Arc<dyn SymbolIndex>>,
}

impl JavaLanguageFactory {
    pub fn new() -> Self {
        Self { index: None }
    }

    pub fn with_index(index: Arc<dyn SymbolIndex>) -> Self {
        Self { index: Some(index) }
    }

    pub fn bind(&self, index: Arc<dyn SymbolIndex>) -> ResolverChain {
        ResolverChain::new(vec![Box::new(TreeSitterResolver::new(index))])
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
    }
}
