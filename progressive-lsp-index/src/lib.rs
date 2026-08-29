//! Index facade: DirtySet, PriorityIndex, IndexCache, incremental parse.

pub mod cache;
pub mod dirty;
pub mod priority;
pub mod service;

pub use cache::{CacheKey, IndexCache};
pub use dirty::DirtySet;
pub use priority::{IndexClass, PriorityIndex};
pub use service::{IndexedFile, InputChange, LanguageIndexer, IndexService, SharedIndex};

use progressive_lsp_core::FileId;
use progressive_lsp_resolve::{IndexedSymbol, SymbolIndex};

impl SymbolIndex for IndexService {
    fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol> {
        self.symbols_for(file)
    }

    fn all_symbols(&self) -> Vec<IndexedSymbol> {
        self.all_indexed_symbols()
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
