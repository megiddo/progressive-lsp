//! Value objects: cache key, generation, entry metadata.

use progressive_lsp_core::{FileId, PackageId};
use progressive_lsp_resolve::{Position, QueryKind, ResolveResult};

/// Monotonic generation for a file or workspace slice. Tied to index dirty state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CacheGeneration(u64);

impl CacheGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn zero() -> Self {
        Self(0)
    }
}

/// Identity for a cached LSP-shaped answer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypesCacheKey {
    pub kind: QueryKind,
    pub file: FileId,
    pub position: Position,
    pub generation: CacheGeneration,
    pub package: Option<PackageId>,
}

impl TypesCacheKey {
    pub fn new(
        kind: QueryKind,
        file: FileId,
        position: Position,
        generation: CacheGeneration,
    ) -> Self {
        Self {
            kind,
            file,
            position,
            generation,
            package: None,
        }
    }

    pub fn with_package(mut self, package: PackageId) -> Self {
        self.package = Some(package);
        self
    }

    pub fn without_generation(
        kind: QueryKind,
        file: FileId,
        position: Position,
    ) -> Self {
        Self {
            kind,
            file,
            position,
            generation: CacheGeneration::zero(),
            package: None,
        }
    }

    pub fn with_generation(mut self, generation: CacheGeneration) -> Self {
        self.generation = generation;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheEntrySource {
    Engine,
    Promoted,
}

impl CacheEntrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Promoted => "promoted",
        }
    }
}

/// Cached payload plus invalidation metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypesCacheEntry {
    pub result: ResolveResult,
    pub filled_at_unix_ms: u64,
    pub source: CacheEntrySource,
    pub engine_generation: u64,
}

impl TypesCacheEntry {
    pub fn new(result: ResolveResult, filled_at_unix_ms: u64, source: CacheEntrySource) -> Self {
        Self {
            result,
            filled_at_unix_ms,
            source,
            engine_generation: 0,
        }
    }

    pub fn with_engine_generation(mut self, gen: u64) -> Self {
        self.engine_generation = gen;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::Tier;
    use progressive_lsp_resolve::ResolveResult;

    #[test]
    fn cache_generation_orders_monotonically() {
        assert!(CacheGeneration::new(1) < CacheGeneration::new(2));
        assert_eq!(CacheGeneration::new(42).as_u64(), 42);
        assert_eq!(CacheGeneration::zero().as_u64(), 0);
        assert_ne!(CacheGeneration::zero(), CacheGeneration::new(1));
    }

    #[test]
    fn types_cache_key_equality_includes_generation() {
        let a = TypesCacheKey::new(
            QueryKind::Definition,
            FileId::new("A.java"),
            Position::new(1, 2),
            CacheGeneration::new(3),
        );
        let b = a.clone().with_generation(CacheGeneration::new(4));
        assert_ne!(a, b);
        let with_pkg = a.clone().with_package(PackageId::new("pkg"));
        assert_eq!(with_pkg.package.as_ref().map(|p| p.as_str()), Some("pkg"));
    }

    #[test]
    fn entry_source_labels() {
        assert_eq!(CacheEntrySource::Engine.as_str(), "engine");
        assert_eq!(CacheEntrySource::Promoted.as_str(), "promoted");
    }

    #[test]
    fn entry_holds_result_and_engine_generation() {
        let entry = TypesCacheEntry::new(
            ResolveResult::empty(Tier::Types),
            1_700_000_000_000,
            CacheEntrySource::Engine,
        )
        .with_engine_generation(7);
        assert_eq!(entry.engine_generation, 7);
        assert_eq!(entry.filled_at_unix_ms, 1_700_000_000_000);
    }
}
