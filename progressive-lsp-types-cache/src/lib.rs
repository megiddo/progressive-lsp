//! T3′ types cache: store, resolver chain step, builder hook, invalidation.

pub mod builder;
pub mod engine_builder;
pub mod fake_chain;
pub mod invalidation;
pub mod key;
pub mod port;
pub mod resolver;
pub mod serve_state;
pub mod stack;
pub mod store;

pub use builder::{
    BuilderQueue, QueuingBuilder, RecordingBuilder, TypesCacheBuilder,
};
pub use invalidation::InvalidationPolicy;
pub use key::{
    CacheEntrySource, CacheGeneration, TypesCacheEntry, TypesCacheKey,
};
pub use port::{
    FixedGenerationPort, GenerationPort, IndexGenerationPort, SharedGenerationPort,
    TypesCachePort,
};
pub use engine_builder::{IndexOpenPort, OpenDocumentPort, spawn_engine_types_cache_builder};
pub use resolver::TypesCacheResolver;
pub use serve_state::CacheServeState;
pub use stack::TypesCacheStack;
pub use store::{FakeTypesCacheStore, TypesCacheStore};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_compile() {
        let _ = TypesCacheStore::new();
        let _ = BuilderQueue::new();
        let _ = TypesCacheStack::new(progressive_lsp_index::SharedIndex::new(
            progressive_lsp_index::IndexService::new(),
        ));
    }
}
