//! Chain of Responsibility: T3 → T2 → T1. One tier answers each query — no merge, no race.
//!
//! A step returns [`ResolveOutcome::NotReady`] when that tier cannot serve the query (engine
//! down, graph/heuristics not ready, etc.); the chain tries the next step. The first
//! [`ResolveOutcome::Ready`] wins, including `Ready` with an empty location list (terminal:
//! do not fall through to a lower tier). Lower tiers never append to or combine with a higher
//! tier’s result.

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

    pub fn prepend(&mut self, step: Box<dyn Resolver>) {
        self.steps.insert(0, step);
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

fn clamp_result_tier(result: &mut ResolveResult, ceiling: Tier) {
    if result.tier.is_above(ceiling) {
        result.tier = ceiling;
    }
    for loc in &mut result.locations {
        if loc.tier.is_above(ceiling) {
            loc.tier = ceiling;
        }
    }
}

impl ResolverChain {
    /// Resolve with chain-step count (for progressive meta / tests).
    pub fn resolve_with_steps(&self, q: &ResolveQuery) -> (ResolveOutcome, u8) {
        let policy = q.chain_policy;
        let iter_cap = policy.max_chain_iter.unwrap_or(u8::MAX);
        let mut steps = 0u8;
        for step in &self.steps {
            if steps >= iter_cap {
                return (ResolveOutcome::NotReady, steps);
            }
            steps += 1;
            match step.resolve(q) {
                ResolveOutcome::Ready(mut result) => {
                    if let Some(ceiling) = policy.max_tier {
                        if result.tier.is_above(ceiling) {
                            continue;
                        }
                        clamp_result_tier(&mut result, ceiling);
                    }
                    return (ResolveOutcome::Ready(result), steps);
                }
                ResolveOutcome::NotReady => continue,
            }
        }
        if policy.max_chain_iter.is_some() && steps >= iter_cap {
            return (ResolveOutcome::NotReady, steps);
        }
        (ResolveOutcome::Ready(ResolveResult::empty(Tier::Syntax)), steps)
    }
}

impl Resolver for ResolverChain {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        self.resolve_with_steps(q).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeResolver, NotReadyResolver};
    use crate::query::{LspLocation, Position, QueryKind, Range};
    use progressive_lsp_core::{FileId, LanguageId, PackageId, Tier};

    fn def_query() -> ResolveQuery {
        ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        )
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
        chain.prepend(Box::new(NotReadyResolver::new(
            LanguageId::new("python"),
            PackageId::new("pkg"),
        )));
        assert_eq!(chain.len(), 5);
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
            Some(Box::new(FakeResolver::graph("t2").with_location(
                LspLocation::new("file:///t2", Range::default(), Tier::Graph),
            ))),
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
            Some(Box::new(FakeResolver::types("t3").with_location(
                LspLocation::new("file:///t3", Range::default(), Tier::Types),
            ))),
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
    fn t3_ready_empty_does_not_fall_to_t1() {
        let chain = ResolverChain::with_tiers(
            Some(Box::new(FakeResolver::types("t3-empty"))),
            None,
            Box::new(FakeResolver::syntax("t1").with_location(LspLocation::new(
                "file:///t1",
                Range::default(),
                Tier::Syntax,
            ))),
        );
        match chain.resolve(&def_query()) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert!(r.locations.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn max_chain_iter_stops_after_n_invocations() {
        use std::sync::atomic::{AtomicU8, Ordering};
        use std::sync::Arc;

        struct CountingResolver {
            calls: Arc<AtomicU8>,
        }

        impl Resolver for CountingResolver {
            fn resolve(&self, _q: &ResolveQuery) -> ResolveOutcome {
                self.calls.fetch_add(1, Ordering::SeqCst);
                ResolveOutcome::NotReady
            }
        }

        let calls = Arc::new(AtomicU8::new(0));
        let chain = ResolverChain::new(vec![
            Box::new(CountingResolver {
                calls: Arc::clone(&calls),
            }),
            Box::new(FakeResolver::graph("t2")),
            Box::new(FakeResolver::syntax("t1")),
        ]);
        let mut q = def_query();
        q.chain_policy.max_chain_iter = Some(1);
        let (outcome, steps) = chain.resolve_with_steps(&q);
        assert_eq!(steps, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(outcome, ResolveOutcome::NotReady));
    }

    #[test]
    fn max_tier_graph_skips_types_ready() {
        let chain = ResolverChain::with_tiers(
            Some(Box::new(FakeResolver::types("t3").with_location(
                LspLocation::new("file:///t3", Range::default(), Tier::Types),
            ))),
            Some(Box::new(FakeResolver::graph("t2").with_location(
                LspLocation::new("file:///t2", Range::default(), Tier::Graph),
            ))),
            Box::new(FakeResolver::syntax("t1")),
        );
        let mut q = def_query();
        q.chain_policy.max_tier = Some(Tier::Graph);
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert_eq!(r.locations[0].uri, "file:///t2");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn max_tier_and_iter_combined() {
        let chain = ResolverChain::with_tiers(
            Some(Box::new(NotReadyResolver::new(
                LanguageId::new("java"),
                PackageId::new("p"),
            ))),
            Some(Box::new(FakeResolver::graph("t2").with_location(
                LspLocation::new("file:///t2", Range::default(), Tier::Graph),
            ))),
            Box::new(FakeResolver::syntax("t1")),
        );
        let mut q = def_query();
        q.chain_policy.max_chain_iter = Some(1);
        q.chain_policy.max_tier = Some(Tier::Graph);
        assert!(matches!(chain.resolve(&q), ResolveOutcome::NotReady));
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
