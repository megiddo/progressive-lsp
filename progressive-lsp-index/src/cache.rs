//! In-memory index cache. Key = (grammar, lang, hash). Disk cache is M5.

use std::collections::HashMap;

use progressive_lsp_core::LanguageId;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub grammar: String,
    pub language: LanguageId,
    pub hash: [u8; 32],
}

impl CacheKey {
    pub fn new(grammar: impl Into<String>, language: LanguageId, bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        Self {
            grammar: grammar.into(),
            language,
            hash,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct IndexCache {
    hits: HashMap<CacheKey, u64>,
}

impl IndexCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn remember(&mut self, key: CacheKey, generation: u64) {
        self.hits.insert(key, generation);
    }

    pub fn get(&self, key: &CacheKey) -> Option<u64> {
        self.hits.get(key).copied()
    }

    pub fn contains(&self, key: &CacheKey) -> bool {
        self.hits.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_triple_hits_and_different_hash_misses() {
        let mut c = IndexCache::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        let a = CacheKey::new("tree-sitter-java", LanguageId::new("java"), b"class A {}");
        let b = CacheKey::new("tree-sitter-java", LanguageId::new("java"), b"class A {}");
        let d = CacheKey::new("tree-sitter-java", LanguageId::new("java"), b"class B {}");
        let e = CacheKey::new("other", LanguageId::new("java"), b"class A {}");
        assert_eq!(a, b);
        assert_ne!(a, d);
        assert_ne!(a, e);
        c.remember(a.clone(), 7);
        assert!(!c.is_empty());
        assert_eq!(c.get(&b), Some(7));
        assert!(c.contains(&a));
        assert!(!c.contains(&d));
        assert_eq!(c.get(&d), None);
        assert_eq!(c.len(), 1);
    }
}
