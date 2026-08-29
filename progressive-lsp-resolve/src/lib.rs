//! Resolver trait, chain of responsibility, and test doubles.
//! T3 `NotReady` must not drop T2.

pub mod chain;
pub mod fake;
pub mod graph;
pub mod heuristic;
pub mod query;
pub mod stack_graph;
pub mod t2;
pub mod tree_sitter;
#[cfg(feature = "t2-stack-graphs")]
pub mod tsg_runtime;

pub use chain::ResolverChain;
pub use fake::{FakeResolver, NotReadyResolver};
pub use graph::{CallSite, GraphFacts, GraphIndex, ImportDecl, TypeEdge};
pub use heuristic::HeuristicResolver;
pub use query::{
    DocumentSymbol, EmptyIndex, Hover, LspLocation, Position, QueryKind, Range, ResolveOutcome,
    ResolveQuery, ResolveResult, SymbolKind,
};
pub use stack_graph::{looks_like_java_tsg, StackGraphResolver, TsgLoadState, TsgPin};
pub use t2::T2Strategy;
pub use tree_sitter::{IndexedSymbol, SymbolIndex, TreeSitterResolver};

use crate::query::ResolveQuery as Q;

/// Domain resolver. JSON-RPC stays in the protocol crate.
pub trait Resolver: Send + Sync {
    fn resolve(&self, q: &Q) -> ResolveOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FileId, LanguageId, PackageId, Tier};

    #[test]
    fn public_reexports_resolve() {
        let _ = Position { line: 0, character: 0 };
        let _ = QueryKind::Definition;
        let _ = ResolverChain::new(Vec::new());
        let _ = FakeResolver::graph("t2");
        let _ = NotReadyResolver::new(LanguageId::new("java"), PackageId::new("p"));
        let _ = TreeSitterResolver::new(std::sync::Arc::new(query::EmptyIndex));
        let _ = HeuristicResolver::new(std::sync::Arc::new(query::EmptyIndex));
        let _ = StackGraphResolver::unused();
        let _ = T2Strategy::from_backend(progressive_lsp_core::T2Backend::Heuristic);
        let _ = TsgPin::java_upstream();
        let _ = FileId::new("f");
        assert_eq!(Tier::Syntax.as_str(), "syntax");
    }
}
