//! T3′ types cache: store, resolver chain step, builder hook, invalidation.

pub mod builder;
pub mod fake_chain;
pub mod invalidation;
pub mod key;
pub mod port;
pub mod resolver;
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
pub use resolver::TypesCacheResolver;
pub use store::{FakeTypesCacheStore, TypesCacheStore};

/// Empty store + recording builder: resolver always misses (TC-2 default hook).
pub fn stub_types_cache_resolver() -> TypesCacheResolver {
    TypesCacheResolver::new(
        std::sync::Arc::new(TypesCacheStore::new()),
        std::sync::Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
        std::sync::Arc::new(RecordingBuilder::new()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_compile() {
        let _ = stub_types_cache_resolver();
        let _ = TypesCacheStore::new();
        let _ = BuilderQueue::new();
    }
}
