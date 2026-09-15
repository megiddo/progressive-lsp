//! T3′ chain step: read cache only; never calls engine.

use std::sync::{Arc, Mutex, RwLock};

use progressive_lsp_core::Tier;
use progressive_lsp_resolve::{ResolveOutcome, ResolveQuery, ResolveResult, Resolver};

use crate::builder::TypesCacheBuilder;
use crate::key::TypesCacheKey;
use crate::port::GenerationPort;
use crate::serve_state::CacheServeState;
use crate::store::TypesCacheStore;

/// Chain of Responsibility step for types cache hits.
pub struct TypesCacheResolver {
    store: Arc<TypesCacheStore>,
    generation: Arc<dyn GenerationPort>,
    builder: Arc<RwLock<Arc<dyn TypesCacheBuilder>>>,
    serve_state: Arc<Mutex<Option<CacheServeState>>>,
}

impl TypesCacheResolver {
    pub fn new(
        store: Arc<TypesCacheStore>,
        generation: Arc<dyn GenerationPort>,
        builder: Arc<RwLock<Arc<dyn TypesCacheBuilder>>>,
        serve_state: Arc<Mutex<Option<CacheServeState>>>,
    ) -> Self {
        Self {
            store,
            generation,
            builder,
            serve_state,
        }
    }

    fn note_state(&self, state: CacheServeState) {
        *self.serve_state.lock().expect("serve_state") = Some(state);
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
            self.note_state(CacheServeState::Hit);
            return ResolveOutcome::Ready(entry.result);
        }
        let builder = self.builder.read().expect("builder");
        let state = if builder.is_inflight(&key) {
            CacheServeState::Inflight
        } else {
            CacheServeState::Miss
        };
        self.note_state(state);
        builder.on_miss(key);
        if builder.terminal_miss_at_types() {
            return ResolveOutcome::Ready(ResolveResult::empty(Tier::Types));
        }
        ResolveOutcome::NotReady
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{assert_within_budget, elapsed};
    use std::time::Instant;
    use crate::builder::RecordingBuilder;
    use crate::fake_chain::chain_with_types_cache_and_t2;
    use crate::key::{CacheEntrySource, CacheGeneration, TypesCacheEntry};
    use crate::port::FixedGenerationPort;
    use progressive_lsp_core::{FileId, Tier};
    use progressive_lsp_resolve::{
        fake::FakeResolver, LspLocation, Position, QueryKind, Range, ResolveQuery, ResolverChain,
    };

    fn shared_builder(
        builder: Arc<dyn TypesCacheBuilder>,
    ) -> Arc<RwLock<Arc<dyn TypesCacheBuilder>>> {
        Arc::new(RwLock::new(builder))
    }

    #[test]
    fn t3_prime_resolve_within_20ms_on_small_fixture() {
        let store = Arc::new(TypesCacheStore::new());
        let resolver = TypesCacheResolver::new(
            store,
            Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
            shared_builder(Arc::new(RecordingBuilder::new())),
            Arc::new(Mutex::new(None)),
        );
        let q = ResolveQuery::new(
            FileId::new("A.java"),
            Position::new(0, 0),
            QueryKind::Definition,
        );
        let start = Instant::now();
        let _ = resolver.resolve(&q);
        assert_within_budget(elapsed(start), "TypesCacheResolver");
    }

    #[test]
    fn stub_miss_falls_through_to_fake_t2() {
        let store = Arc::new(TypesCacheStore::new());
        let serve_state = Arc::new(Mutex::new(None));
        let resolver = TypesCacheResolver::new(
            Arc::clone(&store),
            Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
            shared_builder(Arc::new(RecordingBuilder::new())),
            Arc::clone(&serve_state),
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
        assert_eq!(
            *serve_state.lock().expect("serve_state"),
            Some(CacheServeState::Miss)
        );
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
            shared_builder(Arc::new(RecordingBuilder::new())),
            Arc::new(Mutex::new(None)),
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
