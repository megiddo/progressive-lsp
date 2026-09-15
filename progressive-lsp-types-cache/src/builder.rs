//! Background builder hook and test doubles.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::key::TypesCacheKey;

/// Enqueues cache misses for background fill (engine in TC-3).
pub trait TypesCacheBuilder: Send + Sync {
    fn on_miss(&self, key: TypesCacheKey);
}

/// Coalesced miss queue for builder worker (priority in TC-3).
#[derive(Default)]
pub struct BuilderQueue {
    pending: Mutex<VecDeque<TypesCacheKey>>,
}

impl BuilderQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, key: TypesCacheKey) {
        let mut q = self.pending.lock().expect("builder queue");
        if !q.iter().any(|k| keys_equal_ignore_generation(k, &key)) {
            q.push_back(key);
        }
    }

    pub fn pop(&self) -> Option<TypesCacheKey> {
        self.pending.lock().expect("builder queue").pop_front()
    }

    pub fn len(&self) -> usize {
        self.pending.lock().expect("builder queue").len()
    }
}

fn keys_equal_ignore_generation(a: &TypesCacheKey, b: &TypesCacheKey) -> bool {
    a.kind == b.kind && a.file == b.file && a.position == b.position && a.package == b.package
}

/// Routes misses into a [`BuilderQueue`].
#[derive(Clone)]
pub struct QueuingBuilder {
    queue: Arc<BuilderQueue>,
}

impl QueuingBuilder {
    pub fn new(queue: Arc<BuilderQueue>) -> Self {
        Self { queue }
    }
}

impl TypesCacheBuilder for QueuingBuilder {
    fn on_miss(&self, key: TypesCacheKey) {
        self.queue.push(key);
    }
}

/// Captures enqueued keys without spawning an engine.
#[derive(Default)]
pub struct RecordingBuilder {
    misses: Mutex<Vec<TypesCacheKey>>,
}

impl RecordingBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recorded_misses(&self) -> Vec<TypesCacheKey> {
        self.misses.lock().expect("recording").clone()
    }
}

impl TypesCacheBuilder for RecordingBuilder {
    fn on_miss(&self, key: TypesCacheKey) {
        self.misses.lock().expect("recording").push(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{CacheGeneration, TypesCacheKey};
    use progressive_lsp_core::FileId;
    use progressive_lsp_resolve::{Position, QueryKind};

    fn key_at(line: u32) -> TypesCacheKey {
        TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("A.java"),
            Position::new(line, 0),
            CacheGeneration::new(0),
        )
    }

    #[test]
    fn recording_builder_captures_misses() {
        let b = RecordingBuilder::new();
        b.on_miss(key_at(1));
        b.on_miss(key_at(2));
        assert_eq!(b.recorded_misses().len(), 2);
    }

    #[test]
    fn builder_queue_deduplicates_same_query() {
        let q = Arc::new(BuilderQueue::new());
        let b = QueuingBuilder::new(Arc::clone(&q));
        b.on_miss(key_at(0));
        b.on_miss(key_at(0).with_generation(CacheGeneration::new(1)));
        assert_eq!(q.len(), 1);
        assert!(q.pop().is_some());
        assert_eq!(q.len(), 0);
        assert!(q.pop().is_none());
    }

    #[test]
    fn builder_queue_len_tracks_two_distinct_queries() {
        let q = BuilderQueue::new();
        q.push(key_at(0));
        q.push(key_at(1));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn builder_queue_does_not_dedupe_different_kinds() {
        let q = BuilderQueue::new();
        q.push(key_at(0));
        q.push(TypesCacheKey::new(
            QueryKind::References,
            FileId::new("A.java"),
            Position::new(0, 0),
            CacheGeneration::new(0),
        ));
        assert_eq!(q.len(), 2);
    }
}
