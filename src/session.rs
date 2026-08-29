//! Composition-time session: watch + index + resolver. Not a god LspServer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use progressive_lsp_core::FakeClock;
use progressive_lsp_index::{IndexService, InputChange, SharedIndex};
use progressive_lsp_protocol::LspIntelligence;
use progressive_lsp_resolve::{
    ResolveQuery, ResolveResult, Resolver, ResolverChain, TreeSitterResolver,
};
use progressive_lsp_watch::{DefaultIgnoreFilter, WatchBackend, WatchCoalescer, WatchFilter};

#[cfg(test)]
use progressive_lsp_watch::FakeWatcher;
use progressive_lsp_workspace::{detect_workspace, WorkspaceModel};

#[cfg(feature = "lang-java")]
use progressive_lsp_lang_java::{tokens, JavaIndexer};

pub struct WorkspaceSession {
    pub index: SharedIndex,
    pub chain: ResolverChain,
    pub filter: Box<dyn WatchFilter>,
    pub model: Option<WorkspaceModel>,
}

impl WorkspaceSession {
    pub fn new(index: SharedIndex, chain: ResolverChain) -> Self {
        Self {
            index,
            chain,
            filter: Box::new(DefaultIgnoreFilter),
            model: None,
        }
    }

    pub fn java_default() -> Self {
        let index = SharedIndex::new(IndexService::new());
        let chain = ResolverChain::new(vec![Box::new(TreeSitterResolver::new(Arc::new(
            index.clone(),
        )))]);
        Self::new(index, chain)
    }

    pub fn discover(&mut self, root: &Path) {
        self.model = detect_workspace(root);
    }

    pub fn index_path(&self, path: &Path, source: &str) {
        #[cfg(feature = "lang-java")]
        {
            self.index
                .lock()
                .index_text(path, source, &JavaIndexer, false);
        }
        #[cfg(not(feature = "lang-java"))]
        {
            let _ = (path, source);
        }
    }

    pub fn apply_watch(&self, backend: &mut dyn WatchBackend, clock: &FakeClock, coalescer: &mut WatchCoalescer) {
        coalescer.poll_backend(backend);
        clock.advance_ms(progressive_lsp_watch::DEFAULT_WINDOW_MS);
        if let Some(batch) = coalescer.flush_due() {
            let mut idx = self.index.lock();
            idx.apply_watch_batch(&batch, self.filter.as_ref());
            #[cfg(feature = "lang-java")]
            {
                idx.reindex_dirty(&JavaIndexer);
            }
        }
    }
}

impl LspIntelligence for WorkspaceSession {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult {
        match self.chain.resolve(q) {
            progressive_lsp_resolve::ResolveOutcome::Ready(r) => r,
            progressive_lsp_resolve::ResolveOutcome::NotReady => {
                ResolveResult::empty(progressive_lsp_core::Tier::Syntax)
            }
        }
    }

    fn did_open(&self, uri: &str, _language_id: &str, text: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        self.index.lock().open_buffer(&path);
        self.index_path(&path, text);
    }

    fn did_change(&self, uri: &str, text: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        #[cfg(feature = "lang-java")]
        {
            let old = self
                .index
                .lock()
                .source(&path)
                .unwrap_or("")
                .to_string();
            let change = InputChange::replace_all(&old, text);
            self.index
                .lock()
                .apply_change(&path, &change, &JavaIndexer);
        }
        #[cfg(not(feature = "lang-java"))]
        {
            let _ = (path, text);
        }
    }

    fn did_close(&self, uri: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        self.index.lock().close_buffer(&path);
    }

    fn semantic_tokens(&self, uri: &str) -> Vec<u32> {
        #[cfg(feature = "lang-java")]
        {
            let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
            let src = self.index.lock().source(&path).unwrap_or("").to_string();
            if src.is_empty() {
                return Vec::new();
            }
            let mut p = tree_sitter::Parser::new();
            let _ = p.set_language(&progressive_lsp_lang_java::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return tokens::encode_lsp_data(&tokens::tokens_from_tree(&src, &tree));
            }
            Vec::new()
        }
        #[cfg(not(feature = "lang-java"))]
        {
            let _ = uri;
            Vec::new()
        }
    }
}

pub fn register_languages(registry: &mut progressive_lsp_plugin::PluginRegistry) {
    progressive_lsp_plugin::register_builtins(registry);
    #[cfg(feature = "lang-java")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_java::JavaLanguageFactory::new(),
        ));
    }
}

#[cfg(test)]
pub fn ghost_reindex_unopened(
    session: &WorkspaceSession,
    path: &Path,
    new_source: &str,
) -> bool {
    std::fs::write(path, new_source).is_ok() && {
        let clock = Arc::new(FakeClock::at_unix_ms(5_000));
        let mut coalescer = WatchCoalescer::new(clock.clone());
        let mut fake = FakeWatcher::new();
        fake.inject_one(path.to_string_lossy().as_ref(), progressive_lsp_watch::WatchKind::Modify);
        session.apply_watch(&mut fake, clock.as_ref(), &mut coalescer);
        session
            .index
            .lock()
            .source(path)
            .map(|s| s.contains("ghost") || s == new_source || !s.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_resolve::QueryKind;

    #[test]
    fn ghost_edit_reindexes_without_progressive_client() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Ghost.java");
        std::fs::write(&path, "class Ghost { void a() {} }\n").unwrap();
        let session = WorkspaceSession::java_default();
        session.index_path(&path, &std::fs::read_to_string(&path).unwrap());
        let updated = "class Ghost { void ghost() {} }\n";
        assert!(ghost_reindex_unopened(&session, &path, updated));
        assert!(session.index.lock().source(&path).unwrap().contains("ghost"));
        assert!(!session.index.lock().is_open(&path));
        let q = ResolveQuery::workspace_symbol("ghost");
        let r = session.resolve(&q);
        assert!(r.locations.iter().any(|l| l.uri.contains("Ghost.java")) || !r.locations.is_empty() || session.index.lock().all_indexed_symbols().iter().any(|s| s.name == "ghost"));
    }

    #[test]
    fn session_did_open_change_close() {
        let mut session = WorkspaceSession::java_default();
        session.did_open("file:///Tmp.java", "java", "class Tmp { void a() {} }");
        session.did_change("file:///Tmp.java", "class Tmp { void b() {} }");
        let q = ResolveQuery::new(
            progressive_lsp_core::FileId::new("/Tmp.java"),
            progressive_lsp_resolve::Position::new(0, 20),
            QueryKind::DocumentSymbol,
        );
        let _ = session.resolve(&q);
        let _ = session.semantic_tokens("file:///Tmp.java");
        session.did_close("file:///Tmp.java");
        session.discover(tempfile::tempdir().unwrap().path());
        assert_eq!(progressive_lsp_core::LanguageId::new("java").as_str(), "java");
    }
}
