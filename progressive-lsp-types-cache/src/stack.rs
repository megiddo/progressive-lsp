//! Session wiring: store, resolver, builder worker.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use progressive_lsp_engine::EngineSupervisor;
use progressive_lsp_index::SharedIndex;
use progressive_lsp_resolve::ResolverChain;

use crate::builder::{RecordingBuilder, TypesCacheBuilder};
use crate::engine_builder::{spawn_engine_types_cache_builder, IndexOpenPort, OpenDocumentPort};
use crate::port::{GenerationPort, IndexGenerationPort};
use crate::resolver::TypesCacheResolver;
use crate::serve_state::CacheServeState;
use crate::store::TypesCacheStore;

pub struct TypesCacheStack {
    pub store: Arc<TypesCacheStore>,
    serve_state: Arc<Mutex<Option<CacheServeState>>>,
    generation: Arc<dyn GenerationPort>,
    builder: Arc<dyn TypesCacheBuilder>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TypesCacheStack {
    pub fn new(index: SharedIndex) -> Self {
        let store = Arc::new(TypesCacheStore::new());
        let generation: Arc<dyn GenerationPort> = Arc::new(IndexGenerationPort::new(index));
        let builder: Arc<dyn TypesCacheBuilder> = Arc::new(RecordingBuilder::new());
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

    pub fn attach_engine_builder(
        &mut self,
        supervisor: Arc<EngineSupervisor>,
        index: SharedIndex,
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
        );
        self.builder = builder;
        *self.worker.lock().expect("worker") = Some(handle);
    }
}
