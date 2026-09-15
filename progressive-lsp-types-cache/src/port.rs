//! Ports for generation lookup and store substitution in tests.

use std::path::Path;
use std::sync::{Arc, Mutex};

use progressive_lsp_core::FileId;
use progressive_lsp_index::SharedIndex;

use crate::key::CacheGeneration;

/// Supplies per-file generation for cache keys and invalidation.
pub trait GenerationPort: Send + Sync {
    fn file_generation(&self, file: &FileId) -> CacheGeneration;
}

/// Fixed generation for unit tests.
#[derive(Clone, Debug, Default)]
pub struct FixedGenerationPort {
    generation: CacheGeneration,
}

impl FixedGenerationPort {
    pub fn new(generation: CacheGeneration) -> Self {
        Self { generation }
    }
}

impl GenerationPort for FixedGenerationPort {
    fn file_generation(&self, _file: &FileId) -> CacheGeneration {
        self.generation
    }
}

/// Reads generation from [`IndexService`] dirty set and global counter.
#[derive(Clone)]
pub struct IndexGenerationPort {
    index: SharedIndex,
}

impl IndexGenerationPort {
    pub fn new(index: SharedIndex) -> Self {
        Self { index }
    }
}

impl GenerationPort for IndexGenerationPort {
    fn file_generation(&self, file: &FileId) -> CacheGeneration {
        let idx = self.index.lock();
        let path = Path::new(file.as_str());
        let gen = idx
            .dirty
            .generation_of(path)
            .unwrap_or_else(|| idx.generation());
        CacheGeneration::new(gen)
    }
}

/// Optional swap for store in session composition tests.
pub trait TypesCachePort: Send + Sync {
    fn generation_port(&self) -> Arc<dyn GenerationPort>;
}

#[derive(Clone)]
pub struct SharedGenerationPort {
    inner: Arc<dyn GenerationPort>,
    overrides: Arc<Mutex<std::collections::HashMap<FileId, CacheGeneration>>>,
}

impl SharedGenerationPort {
    pub fn new(inner: Arc<dyn GenerationPort>) -> Self {
        Self {
            inner,
            overrides: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn set_file_generation(&self, file: FileId, generation: CacheGeneration) {
        self.overrides
            .lock()
            .expect("overrides")
            .insert(file, generation);
    }
}

impl GenerationPort for SharedGenerationPort {
    fn file_generation(&self, file: &FileId) -> CacheGeneration {
        if let Some(g) = self.overrides.lock().expect("overrides").get(file) {
            return *g;
        }
        self.inner.file_generation(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_index::IndexService;
    use progressive_lsp_watch::{DefaultIgnoreFilter, WatchBatch, WatchEvent, WatchKind};

    #[test]
    fn index_generation_port_uses_dirty_then_global() {
        let svc = IndexService::new();
        let shared = SharedIndex::new(svc);
        let port = IndexGenerationPort::new(shared.clone());
        let file = FileId::new("src/Lib.java");
        assert_eq!(port.file_generation(&file), CacheGeneration::zero());

        {
            let mut idx = shared.lock();
            let mut batch = WatchBatch::empty(5);
            batch.events.push(WatchEvent::new(file.as_str(), WatchKind::Modify));
            idx.apply_watch_batch(&batch, &DefaultIgnoreFilter);
        }
        assert_eq!(
            port.file_generation(&file),
            CacheGeneration::new(5)
        );
        assert_eq!(
            port.file_generation(&FileId::new("other.java")),
            CacheGeneration::new(5)
        );
    }

    #[test]
    fn shared_generation_port_override_wins() {
        let base = Arc::new(FixedGenerationPort::new(CacheGeneration::new(1)));
        let port = SharedGenerationPort::new(base);
        let file = FileId::new("x");
        assert_eq!(port.file_generation(&file), CacheGeneration::new(1));
        port.set_file_generation(file.clone(), CacheGeneration::new(9));
        assert_eq!(port.file_generation(&file), CacheGeneration::new(9));
    }
}
