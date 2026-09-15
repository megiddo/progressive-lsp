//! In-memory query cache and relation graph slice (graph in TC-5).

use std::collections::HashMap;
use std::sync::Mutex;

use progressive_lsp_core::FileId;

use crate::key::{CacheGeneration, TypesCacheEntry, TypesCacheKey};

/// Repository + facade for T3′ query cache.
pub struct TypesCacheStore {
    entries: Mutex<HashMap<TypesCacheKey, TypesCacheEntry>>,
    engine_generation: Mutex<u64>,
}

impl Default for TypesCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TypesCacheStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            engine_generation: Mutex::new(0),
        }
    }

    pub fn get(&self, key: &TypesCacheKey) -> Option<TypesCacheEntry> {
        let map = self.entries.lock().expect("store");
        map.get(key).cloned()
    }

    pub fn put(&self, key: TypesCacheKey, entry: TypesCacheEntry) {
        self.entries
            .lock()
            .expect("store")
            .insert(key, entry);
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("store").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bump_engine_generation(&self, generation: u64) {
        let mut g = self.engine_generation.lock().expect("engine gen");
        if generation > *g {
            *g = generation;
        }
        let engine_gen = *g;
        drop(g);
        self.entries
            .lock()
            .expect("store")
            .retain(|_, entry| entry.engine_generation == 0 || entry.engine_generation >= engine_gen);
    }

    pub fn engine_generation(&self) -> u64 {
        *self.engine_generation.lock().expect("engine gen")
    }

    /// Drop query keys for `file` whose generation differs from `current`.
    pub fn invalidate_file(&self, file: &FileId, current: CacheGeneration) {
        self.invalidate_stale_generations(file, current);
    }

    pub fn invalidate_stale_generations(&self, file: &FileId, current: CacheGeneration) {
        self.entries.lock().expect("store").retain(|k, _| {
            if k.file == *file {
                k.generation == current
            } else {
                true
            }
        });
    }

    pub fn clear(&self) {
        self.entries.lock().expect("store").clear();
    }
}

/// Test double with the same surface as production store.
pub type FakeTypesCacheStore = TypesCacheStore;

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::Tier;
    use progressive_lsp_resolve::{Position, QueryKind, ResolveResult};

    fn sample_key(gen: u64) -> TypesCacheKey {
        TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("A.java"),
            Position::new(0, 0),
            CacheGeneration::new(gen),
        )
    }

    fn sample_entry() -> TypesCacheEntry {
        TypesCacheEntry::new(
            ResolveResult::empty(Tier::Types),
            100,
            crate::key::CacheEntrySource::Engine,
        )
    }

    #[test]
    fn generation_mismatch_is_miss() {
        let store = TypesCacheStore::new();
        store.put(sample_key(1), sample_entry());
        assert!(store.get(&sample_key(2)).is_none());
        assert!(store.get(&sample_key(1)).is_some());
    }

    #[test]
    fn put_and_get_round_trip() {
        let store = TypesCacheStore::new();
        assert!(store.is_empty());
        store.put(sample_key(0), sample_entry());
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn invalidate_file_drops_matching_file_keys() {
        let store = TypesCacheStore::new();
        store.put(sample_key(1), sample_entry());
        let other = TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("B.java"),
            Position::new(0, 0),
            CacheGeneration::new(1),
        );
        store.put(other, sample_entry());
        store.invalidate_stale_generations(&FileId::new("A.java"), CacheGeneration::new(2));
        assert!(store.get(&sample_key(1)).is_none());
        assert!(store.get(&TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("B.java"),
            Position::new(0, 0),
            CacheGeneration::new(1),
        ))
        .is_some());
    }

    #[test]
    fn engine_generation_bump_stales_engine_entries() {
        let store = TypesCacheStore::new();
        let entry = sample_entry().with_engine_generation(1);
        store.put(sample_key(0), entry);
        store.bump_engine_generation(2);
        assert!(store.get(&sample_key(0)).is_none());
        assert_eq!(store.engine_generation(), 2);
        store.bump_engine_generation(1);
        assert_eq!(store.engine_generation(), 2);
    }

    #[test]
    fn is_empty_reflects_entries() {
        let store = TypesCacheStore::new();
        assert!(store.is_empty());
        store.put(sample_key(0), sample_entry());
        assert!(!store.is_empty());
    }

    #[test]
    fn invalidate_file_drops_only_stale_file_keys() {
        let store = TypesCacheStore::new();
        store.put(sample_key(3), sample_entry());
        store.invalidate_file(&FileId::new("A.java"), CacheGeneration::new(5));
        assert!(store.get(&sample_key(3)).is_none());
        let current = TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("A.java"),
            Position::new(0, 0),
            CacheGeneration::new(5),
        );
        store.put(current.clone(), sample_entry());
        store.invalidate_file(&FileId::new("A.java"), CacheGeneration::new(5));
        assert!(store.get(&current).is_some());
        store.invalidate_file(&FileId::new("A.java"), CacheGeneration::new(6));
        assert!(store.get(&current).is_none());
    }
}
