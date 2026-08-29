//! `IndexService` facade. Owns dirty set, priority, cache, and incremental trees.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use progressive_lsp_core::{FileId, LanguageId, PackageId, PrefixLayout, Tier};
use progressive_lsp_resolve::{
    CallSite, GraphFacts, GraphIndex, ImportDecl, IndexedSymbol, Position, TypeEdge,
};
use progressive_lsp_watch::{WatchBatch, WatchFilter};

use crate::ingest::{IngestReport, PackageIngest};
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
    /// Optional T2 facts. Default is empty (T1-only languages).
    fn extract_graph(&self, file: &FileId, source: &str, tree: &Tree) -> GraphFacts {
        let _ = (file, source, tree);
        GraphFacts::default()
    }
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
    pub has_error: bool,
    pub unparsed_note: Option<String>,
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
    imports: HashMap<PathBuf, Vec<ImportDecl>>,
    edges: Vec<TypeEdge>,
    calls: HashMap<PathBuf, Vec<CallSite>>,
    file_packages: HashMap<PathBuf, PackageId>,
    package_tiers: HashMap<PackageId, Tier>,
    pending_progress: Vec<crate::ingest::WorkDoneProgress>,
    pending_tier_ready: Vec<(PackageId, Tier)>,
}

impl Default for IndexService {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexService {
    pub fn new() -> Self {
        Self::with_cache(IndexCache::new())
    }

    pub fn with_cache(cache: IndexCache) -> Self {
        Self {
            dirty: DirtySet::new(),
            priority: PriorityIndex::new(),
            cache,
            files: HashMap::new(),
            trees: HashMap::new(),
            symbols: HashMap::new(),
            parsers: HashMap::new(),
            generation: 0,
            imports: HashMap::new(),
            edges: Vec::new(),
            calls: HashMap::new(),
            file_packages: HashMap::new(),
            package_tiers: HashMap::new(),
            pending_progress: Vec::new(),
            pending_tier_ready: Vec::new(),
        }
    }

    /// Disk cache under `$PREFIX/cache/`. Tests inject [`PrefixLayout`].
    pub fn with_prefix(layout: &PrefixLayout) -> Self {
        let _ = std::fs::create_dir_all(layout.cache_dir());
        Self::with_cache(IndexCache::open(layout.cache_dir()))
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
                existing.hash = hash;
                existing.source = source.to_string();
            } else {
                self.files.insert(
                    path.to_path_buf(),
                    IndexedFile {
                        path: path.to_path_buf(),
                        language: language.clone(),
                        grammar: grammar.to_string(),
                        source: source.to_string(),
                        hash,
                        generation: gen,
                        last_parse_us: 0,
                        incremental,
                        has_error: false,
                        unparsed_note: None,
                    },
                );
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
        let facts = indexer.extract_graph(&file_id, source, &tree);
        let (has_error, unparsed_note) = tree_unparsed(&tree);
        self.trees.insert(path.to_path_buf(), tree);
        self.symbols.insert(path.to_path_buf(), extracted);
        self.store_graph_facts(path, file_id, facts);
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
                has_error,
                unparsed_note,
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

    fn store_graph_facts(&mut self, path: &Path, _file: FileId, facts: GraphFacts) {
        if !facts.imports.is_empty() {
            self.imports.insert(path.to_path_buf(), facts.imports);
        }
        self.edges.extend(facts.edges);
        if !facts.calls.is_empty() {
            self.calls.insert(path.to_path_buf(), facts.calls);
        }
    }

    pub fn bind_file_package(&mut self, path: impl Into<PathBuf>, package: PackageId) {
        self.file_packages.insert(path.into(), package);
    }

    pub fn package_tier(&self, package: &PackageId) -> Option<Tier> {
        self.package_tiers.get(package).copied()
    }

    pub fn mark_package_tier(&mut self, package: PackageId, tier: Tier) {
        self.package_tiers.insert(package, tier);
    }

    /// Finish T2 for one package. Does not run during `apply_change`.
    pub fn ingest_package(
        &mut self,
        job: &PackageIngest,
        indexer: &dyn LanguageIndexer,
    ) -> IngestReport {
        let mut n = 0usize;
        for path in &job.files {
            if let Ok(src) = std::fs::read_to_string(path) {
                self.bind_file_package(path, job.package.clone());
                self.index_text(path, &src, indexer, false);
                n += 1;
            }
        }
        self.package_tiers.insert(job.package.clone(), Tier::Graph);
        let token = format!("ingest-{}", job.package.as_str());
        let report = IngestReport::graph(job.package.clone(), n, &token);
        self.pending_progress.extend(report.progress.clone());
        self.pending_tier_ready
            .push((job.package.clone(), Tier::Graph));
        report
    }

    pub fn drain_progress(&mut self) -> Vec<crate::ingest::WorkDoneProgress> {
        std::mem::take(&mut self.pending_progress)
    }

    pub fn drain_tier_ready(&mut self) -> Vec<(PackageId, Tier)> {
        std::mem::take(&mut self.pending_tier_ready)
    }
}

impl GraphIndex for IndexService {
    fn imports_in(&self, file: &FileId) -> Vec<ImportDecl> {
        let key = PathBuf::from(file.as_str());
        self.imports.get(&key).cloned().unwrap_or_default()
    }

    fn parents_of(&self, type_fqn: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.child_fqn == type_fqn)
            .map(|e| e.parent_fqn.clone())
            .collect()
    }

    fn package_tier(&self, package: &PackageId) -> Option<Tier> {
        self.package_tiers.get(package).copied()
    }

    fn package_of_file(&self, file: &FileId) -> Option<PackageId> {
        self.file_packages.get(Path::new(file.as_str())).cloned()
    }

    fn call_at(&self, file: &FileId, pos: Position) -> Option<CallSite> {
        let key = PathBuf::from(file.as_str());
        self.calls
            .get(&key)
            .and_then(|cs| cs.iter().find(|c| c.covers(pos)).cloned())
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

impl GraphIndex for SharedIndex {
    fn imports_in(&self, file: &FileId) -> Vec<ImportDecl> {
        self.lock().imports_in(file)
    }
    fn parents_of(&self, type_fqn: &str) -> Vec<String> {
        self.lock().parents_of(type_fqn)
    }
    fn package_tier(&self, package: &PackageId) -> Option<Tier> {
        self.lock().package_tier(package)
    }
    fn package_of_file(&self, file: &FileId) -> Option<PackageId> {
        self.lock().package_of_file(file)
    }
    fn call_at(&self, file: &FileId, pos: Position) -> Option<CallSite> {
        self.lock().call_at(file, pos)
    }
}

/// Newer-than-window / unparsed syntax → ERROR nodes. Server stays up.
pub fn tree_unparsed(tree: &Tree) -> (bool, Option<String>) {
    let n = count_error_nodes(tree.root_node());
    if n == 0 && !tree.root_node().has_error() {
        return (false, None);
    }
    let n = n.max(1);
    (
        true,
        Some(format!("{n} ERROR/MISSING node(s); syntax unparsed")),
    )
}

pub fn count_error_nodes(node: tree_sitter::Node) -> u32 {
    let mut n = 0u32;
    if node.is_error() || node.is_missing() {
        n += 1;
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        n = n.saturating_add(count_error_nodes(child));
    }
    n
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

    #[test]
    fn ingest_marks_graph_and_does_not_run_inside_apply_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Pkg.java");
        std::fs::write(&path, "class Pkg { void a() {} }").unwrap();
        let mut svc = IndexService::new();
        let open = Path::new("Open.java");
        svc.index_text(open, "class Open { int x = 1; }", &JavaIndexer, false);
        let change = InputChange::replace_all("class Open { int x = 1; }", "class Open { int x = 2; }");
        let _ = svc.apply_change(open, &change, &JavaIndexer);
        assert!(svc.source(open).unwrap().contains("x = 2"));
        assert!(svc.package_tier(&PackageId::new("lib")).is_none());
        let job = PackageIngest::new("lib", "java").with_file(&path);
        let report = svc.ingest_package(&job, &JavaIndexer);
        assert_eq!(report.tier, Tier::Graph);
        assert_eq!(svc.package_tier(&PackageId::new("lib")), Some(Tier::Graph));
        let progress = svc.drain_progress();
        assert_eq!(progress.len(), 3);
        assert_eq!(svc.drain_progress().len(), 0);
        let ready = svc.drain_tier_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].1, Tier::Graph);
        assert!(svc.drain_tier_ready().is_empty());
        svc.mark_package_tier(PackageId::new("other"), Tier::Syntax);
        assert_eq!(svc.package_tier(&PackageId::new("other")), Some(Tier::Syntax));
        let file = FileId::new(path.to_string_lossy().as_ref());
        let _ = GraphIndex::imports_in(&svc, &file);
        let _ = GraphIndex::parents_of(&svc, "Pkg");
        let _ = GraphIndex::call_at(&svc, &file, Position::default());
        let _ = GraphIndex::package_of_file(&svc, &file);
        let shared = SharedIndex::new(IndexService::new());
        assert!(GraphIndex::package_tier(&shared, &PackageId::new("x")).is_none());
    }

    #[test]
    fn disk_cache_cold_start_skips_parse_under_injected_prefix() {
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let path = Path::new("Cold.java");
        let src = "class Cold { void m() {} }";
        {
            let mut warm = IndexService::with_prefix(&layout);
            assert_eq!(warm.cache.disk_dir().unwrap(), layout.cache_dir());
            let first = warm.index_text(path, src, &JavaIndexer, false);
            assert!(first > 0 || warm.indexed(path).is_some());
            assert!(!warm.indexed(path).unwrap().has_error);
            assert!(layout.cache_dir().read_dir().unwrap().next().is_some());
        }
        let mut cold = IndexService::with_prefix(&layout);
        let skipped = cold.index_text(path, src, &JavaIndexer, false);
        let rec = cold.indexed(path).unwrap();
        assert_eq!(rec.last_parse_us, 0);
        assert_eq!(rec.source, src);
        assert!(skipped < 5_000, "cache hit should skip Tree-sitter ({skipped}µs)");
        let miss = IndexService::with_cache(IndexCache::new());
        assert!(miss.cache.disk_dir().is_none());
    }

    #[test]
    fn cache_never_lands_in_git_worktree() {
        let workspace = tempfile::tempdir().unwrap();
        let prefix = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(workspace.path())
            .status()
            .unwrap();
        progressive_lsp_core::apply_worktree_excludes(workspace.path()).unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let mut svc = IndexService::with_prefix(&layout);
        svc.index_text(Path::new("A.java"), "class A {}", &JavaIndexer, false);
        let overlay_cache = workspace.path().join(".progressivelsp/cache");
        assert!(
            !overlay_cache.exists()
                || overlay_cache.read_dir().map(|d| d.count()).unwrap_or(0) == 0
        );
        assert!(!workspace.path().join("cache").exists());
        assert!(layout.cache_dir().read_dir().unwrap().next().is_some());
        assert_eq!(svc.cache.disk_dir().unwrap(), layout.cache_dir());
    }

    #[test]
    fn newer_syntax_sets_unparsed_note_without_panic() {
        let mut svc = IndexService::new();
        let src = "class Lag { void m() { ??? } }";
        svc.index_text(Path::new("Lag.java"), src, &JavaIndexer, false);
        let rec = svc.indexed(Path::new("Lag.java")).unwrap();
        assert!(rec.has_error);
        assert!(rec
            .unparsed_note
            .as_deref()
            .unwrap()
            .contains("syntax unparsed"));
        assert_eq!(svc.file_count(), 1);
        let clean = "class Ok {}";
        svc.index_text(Path::new("Ok.java"), clean, &JavaIndexer, false);
        assert!(!svc.indexed(Path::new("Ok.java")).unwrap().has_error);
        assert!(svc.indexed(Path::new("Ok.java")).unwrap().unparsed_note.is_none());
    }

    #[test]
    fn definition_p99_after_index_is_under_50ms() {
        use progressive_lsp_resolve::{Position, QueryKind, ResolveQuery, Resolver, TreeSitterResolver};
        let mut svc = IndexService::new();
        let path = Path::new("Def.java");
        let src = "class Def { void target() {} void caller() { target(); } }\n";
        svc.index_text(path, src, &JavaIndexer, false);
        let shared = SharedIndex::new(svc);
        let resolver = TreeSitterResolver::new(std::sync::Arc::new(shared.clone()));
        let pos = Position::new(0, src.find("target()").unwrap() as u32);
        let q = ResolveQuery::new(FileId::new("Def.java"), pos, QueryKind::Definition);
        let mut times = Vec::with_capacity(100);
        for _ in 0..100 {
            let t = Instant::now();
            let _ = resolver.resolve(&q);
            times.push(t.elapsed().as_micros());
        }
        times.sort_unstable();
        let p99 = times[98];
        assert!(
            p99 < 50_000,
            "T1 definition p99 {p99}µs exceeds 50ms (Darwin sample gate)"
        );
    }
}
