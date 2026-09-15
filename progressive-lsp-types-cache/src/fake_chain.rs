//! Test helpers for resolver chain ordering.

use progressive_lsp_core::Tier;
use progressive_lsp_resolve::{
    chain::ResolverChain, fake::FakeResolver, LspLocation, Range, Resolver,
};

pub fn chain_with_types_cache_and_t2(cache: impl Resolver + 'static) -> ResolverChain {
    ResolverChain::new(vec![
        Box::new(cache),
        Box::new(FakeResolver::graph("t2").with_location(LspLocation::new(
            "file:///t2",
            Range::default(),
            Tier::Graph,
        ))),
    ])
}
