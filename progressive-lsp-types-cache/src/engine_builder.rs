//! Background fill from T3 engine (off mux thread).

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use progressive_lsp_core::{language_id_from_path, FileId, LanguageId, PackageId};
use progressive_lsp_engine::EngineSupervisor;
use progressive_lsp_resolve::{QueryKind, ResolveOutcome, ResolveQuery};

use crate::builder::{BuilderQueue, TypesCacheBuilder};

/// Fired on the builder worker when a T3′ key becomes ready (control `CacheReady` push).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheReadyNotice {
    pub file: FileId,
    pub kind: QueryKind,
    pub location_count: u32,
}

pub type CacheReadyListener = Arc<dyn Fn(CacheReadyNotice) + Send + Sync>;
use crate::key::{CacheEntrySource, CacheGeneration, TypesCacheEntry, TypesCacheKey};
use crate::port::GenerationPort;
use crate::store::TypesCacheStore;

/// Builder only runs when the file is open in the index (same rule as lsp_child warm).
pub trait OpenDocumentPort: Send + Sync {
    fn is_open(&self, file: &FileId) -> bool;
}

#[derive(Clone)]
pub struct IndexOpenPort {
    index: progressive_lsp_index::SharedIndex,
}

impl IndexOpenPort {
    pub fn new(index: progressive_lsp_index::SharedIndex) -> Self {
        Self { index }
    }
}

impl OpenDocumentPort for IndexOpenPort {
    /// Background T3′ fill runs for LSP-open buffers and for files ingested at workspace open.
    fn is_open(&self, file: &FileId) -> bool {
        let path = Path::new(file.as_str());
        let index = self.index.lock();
        index.is_open(path) || index.indexed(path).is_some()
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct MissIdentity {
    kind: QueryKind,
    file: FileId,
    position: progressive_lsp_resolve::Position,
}

impl From<&TypesCacheKey> for MissIdentity {
    fn from(key: &TypesCacheKey) -> Self {
        Self {
            kind: key.kind,
            file: key.file.clone(),
            position: key.position,
        }
    }
}

pub struct EngineTypesCacheBuilder {
    queue: Arc<BuilderQueue>,
    wake: Sender<()>,
    inflight: Arc<Mutex<HashSet<MissIdentity>>>,
    resolve_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl EngineTypesCacheBuilder {
    pub fn resolve_count(&self) -> usize {
        self.resolve_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

struct WorkerCtx {
    supervisor: Arc<EngineSupervisor>,
    store: Arc<TypesCacheStore>,
    generation: Arc<dyn GenerationPort>,
    open: Arc<dyn OpenDocumentPort>,
    queue: Arc<BuilderQueue>,
    inflight: Arc<Mutex<HashSet<MissIdentity>>>,
    resolve_count: Arc<std::sync::atomic::AtomicUsize>,
    cache_ready: Option<CacheReadyListener>,
    wake_rx: Receiver<()>,
}

fn package_for(supervisor: &EngineSupervisor, file: &FileId) -> PackageId {
    let bound = supervisor.package_for_file(file);
    if bound.as_str() == "pkg" {
        PackageId::new("pkg")
    } else {
        bound
    }
}

fn query_from_key(key: &TypesCacheKey) -> ResolveQuery {
    ResolveQuery::new(key.file.clone(), key.position, key.kind)
}

fn drain_queue(ctx: &WorkerCtx) {
    while let Some(mut key) = ctx.queue.pop() {
        if !ctx.open.is_open(&key.file) {
            continue;
        }
        let id = MissIdentity::from(&key);
        {
            let mut inflight = ctx.inflight.lock().expect("inflight");
            if !inflight.insert(id.clone()) {
                continue;
            }
        }
        let gen = ctx.generation.file_generation(&key.file);
        key.generation = gen;
        let language = language_id_from_path(key.file.as_str())
            .unwrap_or_else(|| LanguageId::new("java"));
        let package = package_for(&ctx.supervisor, &key.file);
        let q = query_from_key(&key);
        ctx.resolve_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let outcome = ctx.supervisor.resolve(&language, &package, &q);
        ctx.inflight.lock().expect("inflight").remove(&id);
        if let ResolveOutcome::Ready(result) = outcome {
            let location_count = u32::try_from(result.locations.len()).unwrap_or(u32::MAX);
            let entry = TypesCacheEntry::new(result, 0, CacheEntrySource::Engine)
                .with_engine_generation(ctx.store.engine_generation());
            let notice = CacheReadyNotice {
                file: key.file.clone(),
                kind: key.kind,
                location_count,
            };
            ctx.store.put(key, entry);
            if let Some(listener) = &ctx.cache_ready {
                listener(notice);
            }
        }
    }
}

impl WorkerCtx {
    fn run(self) {
        while self.wake_rx.recv().is_ok() {
            drain_queue(&self);
        }
    }
}

/// Spawns a worker thread; returns builder handle wired to the queue.
pub fn spawn_engine_types_cache_builder(
    supervisor: Arc<EngineSupervisor>,
    store: Arc<TypesCacheStore>,
    generation: Arc<dyn GenerationPort>,
    open: Arc<dyn OpenDocumentPort>,
    cache_ready: Option<CacheReadyListener>,
) -> (Arc<dyn TypesCacheBuilder>, JoinHandle<()>) {
    let queue = Arc::new(BuilderQueue::new());
    let inflight = Arc::new(Mutex::new(HashSet::new()));
    let resolve_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (wake_tx, wake_rx) = mpsc::channel();
    let builder = Arc::new(EngineTypesCacheBuilder {
        queue: Arc::clone(&queue),
        wake: wake_tx.clone(),
        inflight: Arc::clone(&inflight),
        resolve_count: Arc::clone(&resolve_count),
    });
    let worker = WorkerCtx {
        supervisor,
        store,
        generation,
        open,
        queue,
        inflight,
        resolve_count,
        cache_ready,
        wake_rx,
    };
    let handle = thread::spawn(move || worker.run());
    let _ = wake_tx.send(());
    (builder, handle)
}

impl TypesCacheBuilder for EngineTypesCacheBuilder {
    fn on_miss(&self, key: TypesCacheKey) {
        self.queue.push(key);
        let _ = self.wake.send(());
    }

    fn terminal_miss_at_types(&self) -> bool {
        true
    }

    fn is_inflight(&self, key: &TypesCacheKey) -> bool {
        let id = MissIdentity::from(key);
        self.inflight.lock().expect("inflight").contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::FixedGenerationPort;
    use progressive_lsp_core::{FakeClock, PrefixLayout};
    use progressive_lsp_engine::EngineBinary;
    use progressive_lsp_engine::FakeEngineAdapter;
    use progressive_lsp_index::{IndexService, SharedIndex};
    use progressive_lsp_resolve::{Position, QueryKind};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_supervisor() -> EngineSupervisor {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let dir = tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path().join("pfx"));
        let fake = FakeEngineAdapter::java();
        fake.set_answers(FakeEngineAdapter::typed_fixture("Lib", "file:///def"));
        let fake = fake.with_binary(EngineBinary {
            pack_name: "java".into(),
            path: PathBuf::from("/p/javacs"),
            sha256: [0; 32],
        });
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        sup.try_spawn(
            "java",
            &LanguageId::new("java"),
            &PackageId::new("pkg"),
            std::path::Path::new("/ws"),
        )
        .unwrap();
        sup
    }

    #[test]
    fn single_flight_coalesces_duplicate_misses() {
        let sup = Arc::new(test_supervisor());
        let store = Arc::new(TypesCacheStore::new());
        let index = SharedIndex::new(IndexService::new());
        {
            let mut idx = index.lock();
            idx.open_buffer("App.java");
        }
        let open = Arc::new(IndexOpenPort::new(index));
        let gen = Arc::new(FixedGenerationPort::new(CacheGeneration::zero()));
        let queue = Arc::new(BuilderQueue::new());
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        let resolve_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let key = TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("App.java"),
            Position::new(0, 0),
            CacheGeneration::zero(),
        );
        queue.push(key.clone());
        queue.push(key);
        drain_queue(&WorkerCtx {
            supervisor: Arc::clone(&sup),
            store: Arc::clone(&store),
            generation: gen,
            open,
            queue,
            inflight,
            resolve_count: Arc::clone(&resolve_count),
            cache_ready: None,
            wake_rx: mpsc::channel().1,
        });
        assert_eq!(resolve_count.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(store
            .get(&TypesCacheKey::new(
                QueryKind::Definition,
                FileId::new("App.java"),
                Position::new(0, 0),
                CacheGeneration::zero(),
            ))
            .is_some());
    }

    #[test]
    fn builder_runs_for_ingested_file_without_lsp_open() {
        let sup = Arc::new(test_supervisor());
        let store = Arc::new(TypesCacheStore::new());
        let index = SharedIndex::new(IndexService::new());
        {
            let mut idx = index.lock();
            idx.index_text(
                Path::new("Ingested.java"),
                "class Ingested {}",
                &progressive_lsp_lang_java::JavaIndexer,
                false,
            );
        }
        let open = Arc::new(IndexOpenPort::new(index));
        let queue = Arc::new(BuilderQueue::new());
        queue.push(TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("Ingested.java"),
            Position::new(0, 0),
            CacheGeneration::zero(),
        ));
        let resolve_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        drain_queue(&WorkerCtx {
            supervisor: sup,
            store,
            generation: Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
            open,
            queue,
            inflight: Arc::new(Mutex::new(HashSet::new())),
            resolve_count: Arc::clone(&resolve_count),
            cache_ready: None,
            wake_rx: mpsc::channel().1,
        });
        assert_eq!(resolve_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn builder_skips_unindexed_files() {
        let sup = Arc::new(test_supervisor());
        let store = Arc::new(TypesCacheStore::new());
        let index = SharedIndex::new(IndexService::new());
        let open = Arc::new(IndexOpenPort::new(index));
        let queue = Arc::new(BuilderQueue::new());
        queue.push(TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("Closed.java"),
            Position::new(0, 0),
            CacheGeneration::zero(),
        ));
        let resolve_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        drain_queue(&WorkerCtx {
            supervisor: sup,
            store,
            generation: Arc::new(FixedGenerationPort::new(CacheGeneration::zero())),
            open,
            queue,
            inflight: Arc::new(Mutex::new(HashSet::new())),
            resolve_count: Arc::clone(&resolve_count),
            cache_ready: None,
            wake_rx: mpsc::channel().1,
        });
        assert_eq!(resolve_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
