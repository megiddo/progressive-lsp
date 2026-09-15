//! Maps index/engine events to store invalidation.

use std::sync::Arc;

use progressive_lsp_core::FileId;

use crate::key::CacheGeneration;
use crate::port::GenerationPort;
use crate::store::TypesCacheStore;

/// Strategy: bump generations → drop stale query keys.
pub struct InvalidationPolicy {
    store: Arc<TypesCacheStore>,
    generation: Arc<dyn GenerationPort>,
}

impl InvalidationPolicy {
    pub fn new(store: Arc<TypesCacheStore>, generation: Arc<dyn GenerationPort>) -> Self {
        Self { store, generation }
    }

    pub fn on_file_dirty(&self, file: &FileId) {
        let current = self.generation.file_generation(file);
        self.store.invalidate_stale_generations(file, current);
    }

    pub fn on_engine_restart(&self, engine_generation: u64) {
        self.store.bump_engine_generation(engine_generation);
    }

    pub fn generation_for(&self, file: &FileId) -> CacheGeneration {
        self.generation.file_generation(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{CacheEntrySource, TypesCacheEntry, TypesCacheKey};
    use crate::port::{FixedGenerationPort, SharedGenerationPort};
    use progressive_lsp_core::Tier;
    use progressive_lsp_resolve::{Position, QueryKind, ResolveResult};

    #[test]
    fn did_change_fixture_bumps_gen_and_invalidates() {
        let store = Arc::new(TypesCacheStore::new());
        let gen = Arc::new(SharedGenerationPort::new(Arc::new(FixedGenerationPort::new(
            CacheGeneration::new(1),
        ))));
        let policy = InvalidationPolicy::new(
            Arc::clone(&store),
            gen.clone() as Arc<dyn GenerationPort>,
        );
        let file = FileId::new("A.java");
        let key = TypesCacheKey::new(
            QueryKind::Definition,
            file.clone(),
            Position::new(0, 0),
            CacheGeneration::new(1),
        );
        store.put(
            key.clone(),
            TypesCacheEntry::new(
                ResolveResult::empty(Tier::Types),
                0,
                CacheEntrySource::Engine,
            ),
        );
        assert!(store.get(&key).is_some());

        gen.set_file_generation(file.clone(), CacheGeneration::new(2));
        policy.on_file_dirty(&file);
        assert!(store.get(&key).is_none());
        assert_eq!(policy.generation_for(&file), CacheGeneration::new(2));
    }

    #[test]
    fn engine_restart_bumps_store_engine_generation() {
        let store = Arc::new(TypesCacheStore::new());
        let policy = InvalidationPolicy::new(
            Arc::clone(&store),
            Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
        );
        policy.on_engine_restart(3);
        assert_eq!(store.engine_generation(), 3);
    }
}
