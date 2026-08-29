//! `IndexService` facade. Owns dirty set, priority, cache, and incremental trees.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use progressive_lsp_core::{FileId, LanguageId};
use progressive_lsp_resolve::IndexedSymbol;
use progressive_lsp_watch::{WatchBatch, WatchFilter};
use sha2::{Digest, Sha256};
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::cache::{CacheKey, IndexCache};
use crate::dirty::DirtySet;
use crate::priority::PriorityIndex;

/// Incremental text change (LSP `didChange` / InputEdit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputChange {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_row: usize,
    pub start_column: usize,
    pub old_end_row: usize,
    pub old_end_column: usize,
    pub new_end_row: usize,
    pub new_end_column: usize,
    pub new_text: String,
}

impl InputChange {
    pub fn to_input_edit(&self) -> InputEdit {
        InputEdit {
            start_byte: self.start_byte,
            old_end_byte: self.old_end_byte,
            new_end_byte: self.new_end_byte,
            start_position: Point {
                row: self.start_row,
                column: self.start_column,
            },
            old_end_position: Point {
                row: self.old_end_row,
                column: self.old_end_column,
            },
            new_end_position: Point {
                row: self.new_end_row,
                column: self.new_end_column,
            },
        }
    }

    /// Full-document replacement (still goes through the incremental API).
    pub fn replace_all(old: &str, new: &str) -> Self {
        let old_end = end_point(old);
        let new_end = end_point(new);
        Self {
            start_byte: 0,
            old_end_byte: old.len(),
            new_end_byte: new.len(),
            start_row: 0,
            start_column: 0,
            old_end_row: old_end.0,
            old_end_column: old_end.1,
            new_end_row: new_end.0,
            new_end_column: new_end.1,
            new_text: new.to_string(),
        }
    }
}

fn end_point(text: &str) -> (usize, usize) {
    let mut row = 0usize;
    let mut col = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += ch.len_utf8();
        }
    }
    (row, col)
}

pub fn content_hash(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Language-specific extract + grammar. Implemented by `progressive-lsp-lang-*`.
pub trait LanguageIndexer: Send + Sync {
    fn language_id(&self) -> LanguageId;
    fn grammar_id(&self) -> &'static str;
    fn tree_sitter_language(&self) -> tree_sitter::Language;
    fn extract(&self, file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol>;
}

#[derive(Clone, Debug)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub language: LanguageId,
    pub grammar: String,
    pub source: String,
    pub hash: [u8; 32],
    pub generation: u64,
    pub last_parse_us: u128,
    pub incremental: bool,
}

/// Facade over dirty + priority + cache + trees. Not a god server.
pub struct IndexService {
    pub dirty: DirtySet,
    pub priority: PriorityIndex,
    pub cache: IndexCache,
    files: HashMap<PathBuf, IndexedFile>,
    trees: HashMap<PathBuf, Tree>,
    symbols: HashMap<PathBuf, Vec<IndexedSymbol>>,
    parsers: HashMap<String, Parser>,
    generation: u64,
}

impl Default for IndexService {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexService {
    pub fn new() -> Self {
        Self {
            dirty: DirtySet::new(),
            priority: PriorityIndex::new(),
            cache: IndexCache::new(),
            files: HashMap::new(),
            trees: HashMap::new(),
            symbols: HashMap::new(),
            parsers: HashMap::new(),
            generation: 0,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn is_open(&self, path: &Path) -> bool {
        self.priority.is_open(path)
    }

    pub fn open_buffer(&mut self, path: impl Into<PathBuf>) {
        self.priority.mark_open(path);
    }

    pub fn close_buffer(&mut self, path: &Path) {
        self.priority.mark_closed(path);
    }

    pub fn apply_watch_batch(&mut self, batch: &WatchBatch, filter: &dyn WatchFilter) {
        let filtered = filter.filter(batch.clone());
        if filtered.generation > self.generation {
            self.generation = filtered.generation;
        }
        for ev in &filtered.events {
            let path = PathBuf::from(&ev.path);
            self.dirty.mark(&path, filtered.generation);
        }
    }

    pub fn source(&self, path: &Path) -> Option<&str> {
        self.files.get(path).map(|f| f.source.as_str())
    }

    pub fn indexed(&self, path: &Path) -> Option<&IndexedFile> {
        self.files.get(path)
    }

    pub fn symbols_for(&self, file: &FileId) -> Vec<IndexedSymbol> {
        self.symbols
            .values()
            .flatten()
            .filter(|s| s.file == *file)
            .cloned()
            .collect()
    }

    pub fn all_indexed_symbols(&self) -> Vec<IndexedSymbol> {
        self.symbols.values().flatten().cloned().collect()
    }

    pub fn reindex_dirty(&mut self, indexer: &dyn LanguageIndexer) -> usize {
        let paths: Vec<PathBuf> = self.dirty.paths().cloned().collect();
        let ordered = self.priority.order(paths);
        let mut n = 0usize;
        for path in ordered {
            if let Some(src) = std::fs::read_to_string(&path).ok() {
                self.index_text(&path, &src, indexer, false);
                n += 1;
            }
            self.dirty.take(&path);
        }
        n
    }

    pub fn index_text(
        &mut self,
        path: &Path,
        source: &str,
        indexer: &dyn LanguageIndexer,
        incremental: bool,
    ) -> u128 {
        let started = Instant::now();
        let grammar = indexer.grammar_id();
        let language = indexer.language_id();
        let hash = content_hash(source.as_bytes());
        let key = CacheKey::new(grammar, language.clone(), source.as_bytes());
        let gen = self.generation.saturating_add(1);
        self.generation = gen;

        if self.cache.contains(&key) {
            if let Some(existing) = self.files.get_mut(path) {
                existing.generation = gen;
                existing.last_parse_us = 0;
                existing.incremental = incremental;
            }
            let elapsed = started.elapsed().as_micros();
            return elapsed;
        }

        let old_tree = if incremental {
            self.trees.get(path).cloned()
        } else {
            None
        };
        if !self.parsers.contains_key(grammar) {
            let mut p = Parser::new();
            p.set_language(&indexer.tree_sitter_language())
                .expect("tree-sitter language");
            self.parsers.insert(grammar.to_string(), p);
        }
        let tree = self
            .parsers
            .get_mut(grammar)
            .expect("parser")
            .parse(source, old_tree.as_ref())
            .expect("tree-sitter parse");
        let file_id = FileId::new(path.to_string_lossy().as_ref());
        let uri = path_to_uri(path);
        let extracted = indexer.extract(&file_id, &uri, source, &tree);
        self.trees.insert(path.to_path_buf(), tree);
        self.symbols.insert(path.to_path_buf(), extracted);
        let elapsed = started.elapsed().as_micros();
        self.files.insert(
            path.to_path_buf(),
            IndexedFile {
                path: path.to_path_buf(),
                language,
                grammar: grammar.to_string(),
                source: source.to_string(),
                hash,
                generation: gen,
                last_parse_us: elapsed,
                incremental,
            },
        );
        self.cache.remember(key, gen);
        elapsed
    }

    pub fn apply_change(
        &mut self,
        path: &Path,
        change: &InputChange,
        indexer: &dyn LanguageIndexer,
    ) -> u128 {
        let old = self
            .files
            .get(path)
            .map(|f| f.source.clone())
            .unwrap_or_default();
        if let Some(tree) = self.trees.get_mut(path) {
            tree.edit(&change.to_input_edit());
        }
        let mut new_source = old;
        let start = change.start_byte.min(new_source.len());
        let end = change.old_end_byte.min(new_source.len());
        if start <= end {
            new_source.replace_range(start..end, &change.new_text);
        }
        self.index_text(path, &new_source, indexer, true)
    }

    pub fn last_parse_us(&self, path: &Path) -> Option<u128> {
        self.files.get(path).map(|f| f.last_parse_us)
    }
}

/// Mutex-backed `SymbolIndex` so resolvers can share the facade.
#[derive(Clone)]
pub struct SharedIndex {
    inner: std::sync::Arc<std::sync::Mutex<IndexService>>,
}

impl SharedIndex {
    pub fn new(svc: IndexService) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(svc)),
        }
    }

    pub fn from_arc(inner: std::sync::Arc<std::sync::Mutex<IndexService>>) -> Self {
        Self { inner }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, IndexService> {
        self.inner.lock().expect("index lock")
    }

    pub fn arc(&self) -> std::sync::Arc<std::sync::Mutex<IndexService>> {
        self.inner.clone()
    }
}

impl progressive_lsp_resolve::SymbolIndex for SharedIndex {
    fn symbols_in(&self, file: &FileId) -> Vec<IndexedSymbol> {
        self.lock().symbols_for(file)
    }

    fn all_symbols(&self) -> Vec<IndexedSymbol> {
        self.lock().all_indexed_symbols()
    }
}

pub fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.starts_with("file:") {
        s.into_owned()
    } else {
        format!("file://{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_resolve::{Range, SymbolKind};
    use progressive_lsp_watch::{IdentityWatchFilter, WatchEvent, WatchKind};
    use tree_sitter_java::LANGUAGE;

    struct JavaIndexer;

    impl LanguageIndexer for JavaIndexer {
        fn language_id(&self) -> LanguageId {
            LanguageId::new("java")
        }
        fn grammar_id(&self) -> &'static str {
            "tree-sitter-java"
        }
        fn tree_sitter_language(&self) -> tree_sitter::Language {
            LANGUAGE.into()
        }
        fn extract(&self, file: &FileId, uri: &str, source: &str, tree: &Tree) -> Vec<IndexedSymbol> {
            let mut out = Vec::new();
            walk(tree.root_node(), source.as_bytes(), file, uri, &mut out);
            out
        }
    }

    fn walk(
        node: tree_sitter::Node,
        src: &[u8],
        file: &FileId,
        uri: &str,
        out: &mut Vec<IndexedSymbol>,
    ) {
        if node.kind() == "identifier" || node.kind() == "type_identifier" {
            let name = node.utf8_text(src).unwrap_or("").to_string();
            if !name.is_empty() {
                let start = node.start_position();
                let end = node.end_position();
                let range = Range::new(
                    progressive_lsp_resolve::Position::new(start.row as u32, start.column as u32),
                    progressive_lsp_resolve::Position::new(end.row as u32, end.column as u32),
                );
                out.push(IndexedSymbol {
                    file: file.clone(),
                    uri: uri.to_string(),
                    name,
                    kind: SymbolKind::Variable,
                    range,
                    selection_range: range,
                    arity: None,
                    fqn: String::new(),
                    container: None,
                });
            }
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            walk(child, src, file, uri, out);
        }
    }

    #[test]
    fn watch_batch_marks_dirty_and_filter_drops() {
        let mut svc = IndexService::new();
        assert_eq!(svc.generation(), 0);
        let batch = WatchBatch {
            events: vec![
                WatchEvent::new("keep.java", WatchKind::Modify),
                WatchEvent::new("drop.java", WatchKind::Modify),
            ],
            overflow: false,
            need_rescan: false,
            generation: 4,
        };
        svc.apply_watch_batch(&batch, &IdentityWatchFilter);
        assert!(svc.dirty.contains(Path::new("keep.java")));
        assert!(svc.dirty.contains(Path::new("drop.java")));
        assert_eq!(svc.generation(), 4);
        let deny = progressive_lsp_watch::DenyListFilter {
            denied: vec!["drop.java".into()],
        };
        let mut svc2 = IndexService::new();
        svc2.apply_watch_batch(&batch, &deny);
        assert!(svc2.dirty.contains(Path::new("keep.java")));
        assert!(!svc2.dirty.contains(Path::new("drop.java")));
    }

    #[test]
    fn incremental_reparse_is_in_10ms_class() {
        let mut svc = IndexService::new();
        let path = Path::new("Buf.java");
        let src = "class Buf { int x = 1; void m() { x = 2; } }\n";
        svc.open_buffer(path);
        assert!(svc.is_open(path));
        let first = svc.index_text(path, src, &JavaIndexer, false);
        assert!(svc.indexed(path).is_some());
        assert!(!svc.indexed(path).unwrap().incremental);
        let change = InputChange {
            start_byte: src.find('1').unwrap(),
            old_end_byte: src.find('1').unwrap() + 1,
            new_end_byte: src.find('1').unwrap() + 1,
            start_row: 0,
            start_column: src.find('1').unwrap(),
            old_end_row: 0,
            old_end_column: src.find('1').unwrap() + 1,
            new_end_row: 0,
            new_end_column: src.find('1').unwrap() + 1,
            new_text: "3".into(),
        };
        let inc = svc.apply_change(path, &change, &JavaIndexer);
        assert!(svc.indexed(path).unwrap().incremental);
        assert!(svc.source(path).unwrap().contains("int x = 3"));
        let file = FileId::new("Buf.java");
        assert!(!svc.symbols_for(&file).is_empty());
        assert!(!svc.all_indexed_symbols().is_empty());
        assert_eq!(svc.file_count(), 1);
        use progressive_lsp_resolve::SymbolIndex;
        assert!(!SymbolIndex::symbols_in(&svc, &file).is_empty());
        assert!(!SymbolIndex::all_symbols(&svc).is_empty());
        let shared = SharedIndex::new(IndexService::new());
        {
            let mut locked = shared.lock();
            locked.index_text(path, src, &JavaIndexer, false);
        }
        assert!(!SymbolIndex::symbols_in(&shared, &file).is_empty());
        assert!(!SymbolIndex::all_symbols(&shared).is_empty());
        assert!(
            inc < 10_000,
            "incremental parse {inc}µs exceeds ~10ms class (first={first}µs)"
        );
        assert_eq!(svc.last_parse_us(path), Some(svc.indexed(path).unwrap().last_parse_us));
        svc.close_buffer(path);
        assert!(!svc.is_open(path));
    }

    #[test]
    fn cache_hit_skips_parse_work() {
        let mut svc = IndexService::new();
        let path = Path::new("A.java");
        let src = "class A {}";
        svc.index_text(path, src, &JavaIndexer, false);
        let gen = svc.generation();
        svc.index_text(path, src, &JavaIndexer, false);
        assert!(svc.generation() > gen);
        assert_eq!(svc.cache.len(), 1);
        assert_eq!(IndexService::default().file_count(), 0);
        assert_eq!(content_hash(b"a"), content_hash(b"a"));
        assert_ne!(content_hash(b"a"), content_hash(b"b"));
        assert_eq!(path_to_uri(Path::new("file://x")), "file://x");
        assert!(path_to_uri(Path::new("/tmp/a")).starts_with("file://"));
        let _ = InputChange::replace_all("ab", "xyz");
        let edit = InputChange::replace_all("a", "bb").to_input_edit();
        assert_eq!(edit.start_byte, 0);
        assert_eq!(edit.new_end_byte, 2);
        let multiline = InputChange::replace_all("ab\ncd", "x").to_input_edit();
        assert_eq!(multiline.old_end_position.row, 1);
        assert_eq!(multiline.old_end_position.column, 2);
        assert_eq!(multiline.new_end_position.row, 0);
        assert_eq!(multiline.new_end_position.column, 1);
    }

    #[test]
    fn reindex_dirty_reads_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("OnDisk.java");
        std::fs::write(&path, "class OnDisk {}").unwrap();
        let mut svc = IndexService::new();
        svc.dirty.mark(&path, 1);
        let n = svc.reindex_dirty(&JavaIndexer);
        assert_eq!(n, 1);
        assert!(svc.source(&path).unwrap().contains("OnDisk"));
        assert!(!svc.dirty.contains(&path));
        svc.dirty.mark(dir.path().join("missing.java"), 1);
        assert_eq!(svc.reindex_dirty(&JavaIndexer), 0);
    }
}
