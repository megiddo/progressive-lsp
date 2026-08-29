//! T2 Strategy factory. LanguageFactory picks a compiled-in backend; tests inject a fake.

use std::sync::Arc;

use progressive_lsp_core::T2Backend;

use crate::graph::GraphIndex;
use crate::heuristic::HeuristicResolver;
use crate::stack_graph::{StackGraphResolver, TsgPin};
use crate::Resolver;

/// Strategy selector for the T2 slot on the Resolver chain.
///
/// Production picks are [`T2Backend`]. Tests inject [`crate::FakeResolver`] via
/// [`T2Strategy::inject`]. Scripts cannot register definition — this type is
/// compiled-in only.
#[derive(Clone)]
pub enum T2Strategy {
    Heuristic,
    StackGraphs { pin: TsgPin },
    Injected(Arc<dyn Resolver>),
}

impl T2Strategy {
    pub fn from_backend(backend: T2Backend) -> Self {
        match backend {
            T2Backend::Heuristic => Self::Heuristic,
            T2Backend::StackGraphs => Self::StackGraphs {
                pin: TsgPin::java_upstream(),
            },
        }
    }

    pub fn default_heuristic() -> Self {
        Self::Heuristic
    }

    pub fn inject(resolver: Arc<dyn Resolver>) -> Self {
        Self::Injected(resolver)
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Heuristic => T2Backend::Heuristic.as_str(),
            Self::StackGraphs { .. } => T2Backend::StackGraphs.as_str(),
            Self::Injected(_) => "injected",
        }
    }

    /// Build the T2 step. Heuristic uses the graph index. Stack-graphs loads
    /// the pinned Java TSG (not the unused `NotReady` slot).
    pub fn build(&self, graph: Arc<dyn GraphIndex>) -> Box<dyn Resolver> {
        match self {
            Self::Heuristic => Box::new(HeuristicResolver::new(graph)),
            Self::StackGraphs { pin } => Box::new(StackGraphResolver::load_java(pin.clone())),
            Self::Injected(r) => Box::new(InjectedT2(r.clone())),
        }
    }
}

struct InjectedT2(Arc<dyn Resolver>);

impl Resolver for InjectedT2 {
    fn resolve(&self, q: &crate::query::ResolveQuery) -> crate::query::ResolveOutcome {
        self.0.resolve(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeResolver;
    use crate::query::{LspLocation, Position, QueryKind, Range, ResolveOutcome, ResolveQuery};
    use crate::EmptyIndex;
    use progressive_lsp_core::{FileId, Tier};

    #[test]
    fn from_backend_defaults_to_heuristic() {
        let h = T2Strategy::from_backend(T2Backend::Heuristic);
        assert_eq!(h.backend_name(), "heuristic");
        assert_eq!(T2Strategy::default_heuristic().backend_name(), "heuristic");
        let s = T2Strategy::from_backend(T2Backend::StackGraphs);
        assert_eq!(s.backend_name(), "stack-graphs");
        match s {
            T2Strategy::StackGraphs { pin } => {
                assert_eq!(pin.sha, TsgPin::java_upstream().sha);
            }
            _ => panic!("stack-graphs pick"),
        }
    }

    #[test]
    fn inject_fake_t2_is_ready_graph() {
        let fake = FakeResolver::graph("fake-t2").with_location(LspLocation::new(
            "file:///fake",
            Range::default(),
            Tier::Graph,
        ));
        let strategy = T2Strategy::inject(Arc::new(fake));
        assert_eq!(strategy.backend_name(), "injected");
        let step = strategy.build(Arc::new(EmptyIndex));
        match step.resolve(&ResolveQuery::new(
            FileId::new("A.java"),
            Position::default(),
            QueryKind::Definition,
        )) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert_eq!(r.locations[0].uri, "file:///fake");
            }
            ResolveOutcome::NotReady => panic!("injected T2 must run"),
        }
    }

    #[test]
    fn heuristic_build_is_not_ready_on_empty_index() {
        let step = T2Strategy::from_backend(T2Backend::Heuristic).build(Arc::new(EmptyIndex));
        let q = ResolveQuery::new(FileId::new("A.java"), Position::default(), QueryKind::Definition);
        assert!(!step.resolve(&q).is_ready());
    }
}
