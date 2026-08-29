//! Content-addressed `IndexCache` (Repository).
//!
//! Key = `(grammar_ver, language_id, file_hash)` under `$PREFIX/cache/`.
//! Tests inject the prefix. Never write cache into a git worktree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use progressive_lsp_core::LanguageId;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"PLI1";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Grammar pin / version identity (first path component).
    pub grammar: String,
    pub language: LanguageId,
    pub hash: [u8; 32],
}

impl CacheKey {
    pub fn new(grammar: impl Into<String>, language: LanguageId, bytes: &[u8]) -> Self {
        Self {
            grammar: grammar.into(),
            language,
            hash: content_digest(bytes),
        }
    }

    pub fn with_hash(grammar: impl Into<String>, language: LanguageId, hash: [u8; 32]) -> Self {
        Self {
            grammar: grammar.into(),
            language,
            hash,
        }
    }

    /// Relative path under `$PREFIX/cache/`: `grammar_ver/language_id/file_hash`.
    pub fn rel_path(&self) -> PathBuf {
        PathBuf::from(sanitize_component(&self.grammar))
            .join(sanitize_component(self.language.as_str()))
            .join(hex_lower(&self.hash))
    }
}

fn content_digest(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest);
    hash
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Path-safe component. Rejects `.` / `..` so cache cannot escape `$PREFIX/cache/`.
pub fn sanitize_component(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out == "." || out == ".." || out.contains("..") {
        "_".to_string()
    } else {
        out
    }
}

/// Repository: same `(grammar, lang, hash)` → skip parse.
/// Optional disk dir is `$PREFIX/cache/` (injected in tests).
#[derive(Clone, Debug)]
pub struct IndexCache {
    hits: HashMap<CacheKey, u64>,
    disk_dir: Option<PathBuf>,
}

impl Default for IndexCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexCache {
    /// In-memory only (unit tests that do not inject a prefix).
    pub fn new() -> Self {
        Self {
            hits: HashMap::new(),
            disk_dir: None,
        }
    }

    /// Persist under `dir` (normally [`PrefixLayout::cache_dir`](progressive_lsp_core::PrefixLayout::cache_dir)).
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        Self {
            hits: HashMap::new(),
            disk_dir: Some(dir),
        }
    }

    pub fn disk_dir(&self) -> Option<&Path> {
        self.disk_dir.as_deref()
    }

    pub fn disk_path(&self, key: &CacheKey) -> Option<PathBuf> {
        self.disk_dir.as_ref().map(|d| d.join(key.rel_path()))
    }

    pub fn remember(&mut self, key: CacheKey, generation: u64) {
        self.write_disk(&key, generation);
        self.hits.insert(key, generation);
    }

    pub fn get(&self, key: &CacheKey) -> Option<u64> {
        if let Some(g) = self.hits.get(key).copied() {
            return Some(g);
        }
        self.read_disk(key)
    }

    pub fn contains(&self, key: &CacheKey) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    fn write_disk(&self, key: &CacheKey, generation: u64) {
        let Some(path) = self.disk_path(key) else {
            return;
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let mut bytes = Vec::with_capacity(12);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&generation.to_le_bytes());
        let _ = std::fs::write(path, bytes);
    }

    fn read_disk(&self, key: &CacheKey) -> Option<u64> {
        let path = self.disk_path(key)?;
        let bytes = std::fs::read(path).ok()?;
        parse_cache_record(&bytes)
    }
}

pub fn parse_cache_record(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 12 {
        return None;
    }
    if &bytes[0..4] != MAGIC {
        return None;
    }
    let mut gen = [0u8; 8];
    gen.copy_from_slice(&bytes[4..12]);
    Some(u64::from_le_bytes(gen))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_triple_hits_and_different_hash_misses() {
        let mut c = IndexCache::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        assert!(c.disk_dir().is_none());
        assert!(c.disk_path(&CacheKey::new("g", LanguageId::new("java"), b"x")).is_none());
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
        assert_eq!(IndexCache::default().len(), 0);
    }

    #[test]
    fn disk_cold_start_hits_same_triple() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let key = CacheKey::new("tree-sitter-java@0.23", LanguageId::new("java"), b"class A {}");
        {
            let mut c = IndexCache::open(&cache_dir);
            assert_eq!(c.disk_dir().unwrap(), cache_dir.as_path());
            assert!(cache_dir.is_dir());
            c.remember(key.clone(), 11);
            let on_disk = c.disk_path(&key).unwrap();
            assert!(on_disk.is_file());
            assert!(on_disk.starts_with(&cache_dir));
            assert_eq!(c.get(&key), Some(11));
        }
        let cold = IndexCache::open(&cache_dir);
        assert!(cold.is_empty(), "cold start must not preload memory");
        assert!(cold.contains(&key));
        assert_eq!(cold.get(&key), Some(11));
        let miss = CacheKey::new("tree-sitter-java@0.23", LanguageId::new("java"), b"class B {}");
        assert!(!cold.contains(&miss));
    }

    #[test]
    fn disk_path_is_grammar_lang_hash_and_never_escapes() {
        let key = CacheKey::with_hash(
            "tree-sitter-java",
            LanguageId::new("java"),
            [0xab; 32],
        );
        let rel = key.rel_path();
        assert_eq!(
            rel,
            PathBuf::from("tree-sitter-java")
                .join("java")
                .join(hex_lower(&[0xab; 32]))
        );
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(sanitize_component("."), "_");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component("foo/bar"), "foo_bar");
        assert_eq!(sanitize_component("a..b"), "_");
        assert_eq!(sanitize_component("ok-id_1.2"), "ok-id_1.2");
        let escape = CacheKey::new("..", LanguageId::new("java"), b"x");
        assert_eq!(escape.rel_path().components().next().unwrap().as_os_str(), "_");
    }

    #[test]
    fn corrupt_or_short_disk_record_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = IndexCache::open(dir.path());
        let key = CacheKey::new("g", LanguageId::new("java"), b"src");
        c.remember(key.clone(), 3);
        let path = c.disk_path(&key).unwrap();
        std::fs::write(&path, b"NOPE").unwrap();
        let cold = IndexCache::open(dir.path());
        assert!(!cold.contains(&key));
        std::fs::write(&path, b"PLI").unwrap();
        assert!(!IndexCache::open(dir.path()).contains(&key));
        std::fs::write(&path, b"XXXX\x01\x00\x00\x00\x00\x00\x00\x00").unwrap();
        assert!(!IndexCache::open(dir.path()).contains(&key));
        assert_eq!(parse_cache_record(b""), None);
        assert_eq!(parse_cache_record(b"PLI1"), None);
        assert_eq!(
            parse_cache_record(&[b'P', b'L', b'I', b'1', 5, 0, 0, 0, 0, 0, 0, 0]),
            Some(5)
        );
    }

    #[test]
    fn write_failure_keeps_memory_hit() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, b"file").unwrap();
        let mut c = IndexCache::open(&blocker);
        let key = CacheKey::new("g", LanguageId::new("java"), b"x");
        c.remember(key.clone(), 2);
        assert_eq!(c.get(&key), Some(2));
        assert!(!IndexCache::open(&blocker).contains(&key));
    }

    #[test]
    fn memory_only_does_not_create_workspace_cache() {
        let workspace = tempfile::tempdir().unwrap();
        let mut c = IndexCache::new();
        c.remember(
            CacheKey::new("g", LanguageId::new("java"), b"x"),
            1,
        );
        assert!(!workspace.path().join(".progressivelsp").exists());
        assert!(!workspace.path().join("cache").exists());
    }
}
