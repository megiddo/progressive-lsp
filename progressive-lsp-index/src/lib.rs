//! Index facade: DirtySet, PriorityIndex, IndexCache, incremental parse.

pub mod cache;
pub mod dirty;
pub mod ingest;
pub mod priority;
pub mod service;

pub use cache::{sanitize_component, CacheKey, IndexCache};
pub use dirty::DirtySet;
pub use ingest::{IngestReport, PackageIngest, ProgressKind, WorkDoneProgress};
pub use priority::{IndexClass, PriorityIndex};
pub use service::{
    count_error_nodes, tree_unparsed, IndexService, IndexedFile, InputChange, LanguageIndexer,
    SharedIndex,
};

use progressive_lsp_core::FileId;
use progressive_lsp_resolve::{IndexedSymbol, SymbolIndex};

impl SymbolIndex for IndexService {
    fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol> {
        self.symbols_for(file)
    }

    fn all_symbols(&self) -> Vec<IndexedSymbol> {
        self.all_indexed_symbols()
    }

    fn file_text(&self, file: &FileId) -> Option<&str> {
        self.source(std::path::Path::new(file.as_str()))
    }

    fn indexed_files(&self) -> Vec<FileId> {
        self.indexed_file_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_resolve::SymbolIndex;

    #[test]
    fn index_service_is_a_symbol_index() {
        let svc = IndexService::new();
        assert!(svc.all_symbols().is_empty());
        assert!(svc.symbols_in(&FileId::new("x")).is_empty());
        let shared = SharedIndex::new(IndexService::new());
        assert!(shared.all_symbols().is_empty());
        let again = SharedIndex::from_arc(shared.arc());
        assert!(again.symbols_in(&FileId::new("x")).is_empty());
        let _ = again.lock().generation();
    }
}
