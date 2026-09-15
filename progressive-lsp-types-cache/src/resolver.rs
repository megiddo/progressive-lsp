//! T3′ chain step: read cache only; never calls engine.

use std::sync::Arc;

use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery, Resolver};

use crate::builder::TypesCacheBuilder;
use crate::key::TypesCacheKey;
use crate::port::GenerationPort;
use crate::store::TypesCacheStore;

/// Chain of Responsibility step for types cache hits.
pub struct TypesCacheResolver {
    store: Arc<TypesCacheStore>,
    generation: Arc<dyn GenerationPort>,
    builder: Arc<dyn TypesCacheBuilder>,
}

impl TypesCacheResolver {
    pub fn new(
        store: Arc<TypesCacheStore>,
        generation: Arc<dyn GenerationPort>,
        builder: Arc<dyn TypesCacheBuilder>,
    ) -> Self {
        Self {
            store,
            generation,
            builder,
        }
    }

    fn key_for(&self, q: &ResolveQuery) -> TypesCacheKey {
        let generation = self.generation.file_generation(&q.file);
        TypesCacheKey::new(q.kind, q.file.clone(), q.position, generation)
    }
}

impl Resolver for TypesCacheResolver {
    fn resolve(&self, q: &ResolveQuery) -> ResolveOutcome {
        let key = self.key_for(q);
        if let Some(entry) = self.store.get(&key) {
            return ResolveOutcome::Ready(entry.result);
        }
        self.builder.on_miss(key);
        ResolveOutcome::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RecordingBuilder;
    use crate::fake_chain::chain_with_types_cache_and_t2;
    use crate::key::{CacheEntrySource, CacheGeneration, TypesCacheEntry};
    use crate::port::FixedGenerationPort;
    use progressive_lsp_core::{FileId, Tier};
    use progressive_lsp_resolve::{
        fake::FakeResolver, LspLocation, Position, QueryKind, Range, ResolveQuery, ResolverChain,
    };

    #[test]
    fn stub_miss_falls_through_to_fake_t2() {
        let store = Arc::new(TypesCacheStore::new());
        let builder = Arc::new(RecordingBuilder::new());
        let resolver = TypesCacheResolver::new(
            Arc::clone(&store),
            Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
            Arc::clone(&builder) as Arc<dyn TypesCacheBuilder>,
        );
        let chain = chain_with_types_cache_and_t2(resolver);
        let q = ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        );
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Graph);
                assert_eq!(r.locations[0].uri, "file:///t2");
            }
            ResolveOutcome::NotReady => panic!("T2 must answer after cache miss"),
        }
        assert_eq!(builder.recorded_misses().len(), 1);
    }

    #[test]
    fn cache_hit_returns_types_tier_without_t2() {
        let store = Arc::new(TypesCacheStore::new());
        let gen = CacheGeneration::new(0);
        let q = ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        );
        let key = TypesCacheKey::new(q.kind, q.file.clone(), q.position, gen);
        store.put(
            key,
            TypesCacheEntry::new(
                progressive_lsp_resolve::ResolveResult::locations(
                    Tier::Types,
                    vec![LspLocation::new(
                        "file:///cached",
                        Range::default(),
                        Tier::Types,
                    )],
                ),
                0,
                CacheEntrySource::Engine,
            ),
        );
        let resolver = TypesCacheResolver::new(
            Arc::clone(&store),
            Arc::new(FixedGenerationPort::new(gen)),
            Arc::new(RecordingBuilder::new()),
        );
        let chain = ResolverChain::new(vec![
            Box::new(resolver),
            Box::new(FakeResolver::graph("t2").with_location(LspLocation::new(
                "file:///t2",
                Range::default(),
                Tier::Graph,
            ))),
        ]);
        match chain.resolve(&q) {
            ResolveOutcome::Ready(r) => {
                assert_eq!(r.tier, Tier::Types);
                assert_eq!(r.locations[0].uri, "file:///cached");
            }
            other => panic!("{other:?}"),
        }
    }
}
