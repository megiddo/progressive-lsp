//! Chain of Responsibility: T3 → T2 → T1. `NotReady` does not drop the next handler.

use crate::query::{ResolveOutcome, ResolveQuery, ResolveResult};
use crate::Resolver;
use progressive_lsp_core::Tier;

/// Ordered resolvers. First `Ready` wins; `NotReady` continues.
pub struct ResolverChain {
    steps: Vec<Box<dyn Resolver>>,
}

impl ResolverChain {
    pub fn new(steps: Vec<Box<dyn Resolver>>) -> Self {
        Self { steps }
    }

    pub fn empty() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn push(&mut self, step: Box<dyn Resolver>) {
        self.steps.push(step);
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// T3 (if any), then T2 (if any), then T1. First `Ready` wins.
    pub fn with_tiers(
        t3: Option<Box<dyn Resolver>>,
        t2: Option<Box<dyn Resolver>>,
        t1: Box<dyn Resolver>,
    ) -> Self {
        let mut steps = Vec::new();
        if let Some(r) = t3 {
            steps.push(r);
        }
        if let Some(r) = t2 {
            steps.push(r);
        }
        steps.push(t1);
        Self { steps }
    }
}

impl Resolver for ResolverChain {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        for step in &self.steps {
            match step.resolve(q) {
                ResolveOutcome::Ready(result) => return ResolveOutcome::Ready(result),
                ResolveOutcome::NotReady => continue,
            }
        }
        ResolveOutcome::Ready(ResolveResult::empty(Tier::Syntax))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeResolver, NotReadyResolver};
    use crate::query::{LspLocation, Position, QueryKind, Range};
    use progressive_lsp_core::{FileId, LanguageId, PackageId, Tier};

    fn def_query() -> ResolveQuery {
        ResolveQuery::new(FileId::new("A.java"), Position::new(0, 0), QueryKind::Definition)
    }

    #[test]
    fn empty_chain_is_ready_empty_syntax() {
        let chain = ResolverChain::empty();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert!(r.locations.is_empty());
                assert_eq!(r.tier, Tier::Syntax);
            }
            ResolveOutcome::NotReady => panic!("empty chain must not be NotReady"),
        }
    }

    #[test]
    fn not_ready_t3_does_not_drop_t2() {
        let t2 = FakeResolver::graph("t2-hit").with_location(LspLocation::new(
            "file:///t2",
            Range::default(),
            Tier::Graph,
        ));
        let mut chain = ResolverChain::new(vec![
            Box::new(NotReadyResolver::new(
                LanguageId::new("java"),
                PackageId::new("pkg"),
            )),
            Box::new(t2),
            Box::new(FakeResolver::syntax("t1-hit")),
        ]);
        chain.push(Box::new(FakeResolver::syntax("unused")));
        assert_eq!(chain.len(), 4);
        assert!(!chain.is_empty());
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert_eq!(r.locations.len(), 1);
                assert_eq!(r.locations[0].uri, "file:///t2");
            }
            ResolveOutcome::NotReady => panic!("T2 must run after T3 NotReady"),
        }
    }

    #[test]
    fn first_ready_wins_even_if_later_steps_exist() {
        let chain = ResolverChain::new(vec![
            Box::new(FakeResolver::syntax("t1").with_location(LspLocation::new(
                "file:///t1",
                Range::default(),
                Tier::Syntax,
            ))),
            Box::new(FakeResolver::graph("t2").with_location(LspLocation::new(
                "file:///t2",
                Range::default(),
                Tier::Graph,
            ))),
        ]);
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.locations[0].uri, "file:///t1");
                assert_eq!(r.tier, Tier::Syntax);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn with_tiers_is_t3_then_t2_then_t1() {
        let chain = ResolverChain::with_tiers(
            Some(Box::new(NotReadyResolver::new(
                LanguageId::new("python"),
                PackageId::new("pkg"),
            ))),
            Some(Box::new(FakeResolver::graph("t2").with_location(LspLocation::new(
                "file:///t2",
                Range::default(),
                Tier::Graph,
            )))),
            Box::new(FakeResolver::syntax("t1").with_location(LspLocation::new(
                "file:///t1",
                Range::default(),
                Tier::Syntax,
            ))),
        );
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert_eq!(r.locations[0].uri, "file:///t2");
            }
            other => panic!("{other:?}"),
        }
        let t3_ready = ResolverChain::with_tiers(
            Some(Box::new(FakeResolver::types("t3").with_location(LspLocation::new(
                "file:///t3",
                Range::default(),
                Tier::Types,
            )))),
            Some(Box::new(FakeResolver::graph("t2"))),
            Box::new(FakeResolver::syntax("t1")),
        );
        match t3_ready.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert_eq!(r.locations[0].uri, "file:///t3");
            }
            other => panic!("{other:?}"),
        }
        let no_t2 = ResolverChain::with_tiers(
            Some(Box::new(NotReadyResolver::new(
                LanguageId::new("rust"),
                PackageId::new("p"),
            ))),
            None,
            Box::new(FakeResolver::syntax("t1").with_location(LspLocation::new(
                "file:///t1",
                Range::default(),
                Tier::Syntax,
            ))),
        );
        match no_t2.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Syntax);
                assert_eq!(r.locations[0].uri, "file:///t1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn all_not_ready_falls_back_to_empty_syntax() {
        let chain = ResolverChain::new(vec![Box::new(NotReadyResolver::new(
            LanguageId::new("java"),
            PackageId::new("p"),
        ))]);
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert!(r.locations.is_empty());
                assert_eq!(r.tier, Tier::Syntax);
            }
            ResolveOutcome::NotReady => panic!("chain itself is always Ready"),
        }
    }
}
