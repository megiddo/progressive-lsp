//! Session wiring: store, resolver, builder worker.

use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use progressive_lsp_engine::EngineSupervisor;
use progressive_lsp_index::SharedIndex;
use progressive_lsp_resolve::{ResolveQuery, ResolveResult, ResolverChain};

use crate::builder::{RecordingBuilder, TypesCacheBuilder};
use crate::key::{CacheEntrySource, TypesCacheEntry, TypesCacheKey};
use crate::engine_builder::{
    spawn_engine_types_cache_builder, CacheReadyListener, IndexOpenPort, OpenDocumentPort,
};
use crate::port::{GenerationPort, IndexGenerationPort};
use crate::resolver::TypesCacheResolver;
use crate::serve_state::CacheServeState;
use crate::invalidation::InvalidationPolicy;
use crate::store::TypesCacheStore;
use progressive_lsp_core::FileId;
use progressive_lsp_watch::{WatchBatch, WatchKind};

pub struct TypesCacheStack {
    pub store: Arc<TypesCacheStore>,
    serve_state: Arc<Mutex<Option<CacheServeState>>>,
    generation: Arc<dyn GenerationPort>,
    builder: Arc<RwLock<Arc<dyn TypesCacheBuilder>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TypesCacheStack {
    pub fn new(index: SharedIndex) -> Self {
        let store = Arc::new(TypesCacheStore::new());
        let generation: Arc<dyn GenerationPort> = Arc::new(IndexGenerationPort::new(index));
        let builder = Arc::new(RwLock::new(Arc::new(RecordingBuilder::new())
            as Arc<dyn TypesCacheBuilder>));
        Self {
            store,
            serve_state: Arc::new(Mutex::new(None)),
            generation,
            builder,
            worker: Mutex::new(None),
        }
    }

    pub fn prepend_resolver(&self, chain: &mut ResolverChain) {
        chain.prepend(Box::new(TypesCacheResolver::new(
            Arc::clone(&self.store),
            Arc::clone(&self.generation),
            Arc::clone(&self.builder),
            Arc::clone(&self.serve_state),
        )));
    }

    pub fn take_serve_state(&self) -> CacheServeState {
        self.serve_state
            .lock()
            .expect("serve_state")
            .take()
            .unwrap_or(CacheServeState::NotApplicable)
    }

    pub fn types_cache_entry_count(&self) -> u64 {
        self.store.len() as u64
    }

    /// Store a synchronous engine hit so the next mux discover is ≤20 ms.
    pub fn remember_engine_hit(&self, q: &ResolveQuery, result: &ResolveResult) {
        let generation = self.generation.file_generation(&q.file);
        let key = TypesCacheKey::new(q.kind, q.file.clone(), q.position, generation);
        let entry = TypesCacheEntry::new(result.clone(), 0, CacheEntrySource::Engine)
            .with_engine_generation(self.store.engine_generation());
        self.store.put(key, entry);
    }

    /// T3′ invalidation when the file-event hub delivers a batch.
    pub fn on_watch_batch(&self, batch: &WatchBatch) {
        let policy = InvalidationPolicy::new(
            Arc::clone(&self.store),
            Arc::clone(&self.generation),
        );
        for ev in &batch.events {
            if ev.kind == WatchKind::Delete {
                continue;
            }
            policy.on_file_dirty(&FileId::new(&ev.path));
        }
    }

    pub fn attach_engine_builder(
        &mut self,
        supervisor: Arc<EngineSupervisor>,
        index: SharedIndex,
        cache_ready: Option<CacheReadyListener>,
    ) {
        if self.worker.lock().expect("worker").is_some() {
            return;
        }
        let open: Arc<dyn OpenDocumentPort> = Arc::new(IndexOpenPort::new(index));
        let (builder, handle) = spawn_engine_types_cache_builder(
            supervisor,
            Arc::clone(&self.store),
            Arc::clone(&self.generation),
            open,
            cache_ready,
        );
        *self.builder.write().expect("builder") = builder;
        *self.worker.lock().expect("worker") = Some(handle);
    }
}
