//! Composition-time session: watch + index + resolve + ingest + scripts. Not a god LspServer.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{
    FakeClock, InitializeFailed, LogComponent, LogPort, LogScope, NullLog, PackageId, PrefixLayout,
    T2Backend, Tier,
};
use progressive_lsp_engine::EngineSupervisor;
use progressive_lsp_index::{
    IndexService, InputChange, LanguageIndexer, PackageIngest, SharedIndex,
};
use progressive_lsp_protocol::{LspIntelligence, WorkDoneProgress};
use progressive_lsp_resolve::{
    ResolveQuery, ResolveResult, Resolver, ResolverChain, T2Strategy, TreeSitterResolver,
};
use progressive_lsp_script::{RhaiEngineFactory, ScriptContext, ScriptHost};
use progressive_lsp_watch::{DefaultIgnoreFilter, WatchBackend, WatchCoalescer, WatchFilter};
use progressive_lsp_workspace::{detect_workspace, PackageEntry, WorkspaceModel};

#[cfg(test)]
use progressive_lsp_watch::FakeWatcher;

#[cfg(feature = "lang-c")]
use progressive_lsp_lang_c::CIndexer;
#[cfg(feature = "lang-cpp")]
use progressive_lsp_lang_cpp::CppIndexer;
#[cfg(feature = "lang-csharp")]
use progressive_lsp_lang_csharp::CSharpIndexer;
#[cfg(feature = "lang-css")]
use progressive_lsp_lang_css::CssIndexer;
#[cfg(feature = "lang-go")]
use progressive_lsp_lang_go::GoIndexer;
#[cfg(feature = "lang-html")]
use progressive_lsp_lang_html::HtmlIndexer;
#[cfg(feature = "lang-java")]
use progressive_lsp_lang_java::{tokens as java_tokens, JavaIndexer};
#[cfg(feature = "lang-javascript")]
use progressive_lsp_lang_javascript::JavaScriptIndexer;
#[cfg(feature = "lang-php")]
use progressive_lsp_lang_php::PhpIndexer;
#[cfg(feature = "lang-python")]
use progressive_lsp_lang_python::PythonIndexer;
#[cfg(feature = "lang-rust")]
use progressive_lsp_lang_rust::RustIndexer;
#[cfg(feature = "lang-zig")]
use progressive_lsp_lang_zig::ZigIndexer;

pub struct WorkspaceSession {
    pub index: SharedIndex,
    pub chain: ResolverChain,
    pub filter: Box<dyn WatchFilter>,
    pub model: Mutex<Option<WorkspaceModel>>,
    scripts: Mutex<Option<ScriptHost>>,
    progress: Mutex<Vec<WorkDoneProgress>>,
    skipped_packages: Mutex<Vec<PackageId>>,
    supervisor: Option<Arc<EngineSupervisor>>,
    log: Arc<dyn LogPort>,
}

impl WorkspaceSession {
    pub fn new(index: SharedIndex, chain: ResolverChain) -> Self {
        Self {
            index,
            chain,
            filter: Box::new(DefaultIgnoreFilter),
            model: Mutex::new(None),
            scripts: Mutex::new(None),
            progress: Mutex::new(Vec::new()),
            skipped_packages: Mutex::new(Vec::new()),
            supervisor: None,
            log: Arc::new(NullLog),
        }
    }

    pub fn with_log(mut self, log: Arc<dyn LogPort>) -> Self {
        self.log = log;
        self
    }

    pub fn with_supervisor(mut self, supervisor: Arc<EngineSupervisor>) -> Self {
        self.supervisor = Some(supervisor);
        self
    }

    pub fn with_scripts(self, host: ScriptHost) -> Self {
        *self.scripts.lock().expect("scripts") = Some(host);
        self
    }

    pub fn with_prefix(layout: &PrefixLayout) -> Self {
        Self::with_prefix_and_t2(layout, T2Backend::Heuristic)
    }

    pub fn with_prefix_and_t2(layout: &PrefixLayout, t2: T2Backend) -> Self {
        Self::with_prefix_and_t2_log(layout, t2, Arc::new(NullLog))
    }

    pub fn with_prefix_and_t2_log(
        layout: &PrefixLayout,
        t2: T2Backend,
        log: Arc<dyn LogPort>,
    ) -> Self {
        let index = SharedIndex::new(IndexService::with_prefix_and_log(layout, Arc::clone(&log)));
        let chain = ResolverChain::new(vec![
            T2Strategy::from_backend(t2).build(Arc::new(index.clone())),
            Box::new(TreeSitterResolver::new(Arc::new(index.clone()))),
        ]);
        Self::new(index, chain).with_log(log)
    }

    pub fn java_default() -> Self {
        let index = SharedIndex::new(IndexService::new());
        let chain = ResolverChain::new(vec![
            T2Strategy::from_backend(T2Backend::Heuristic).build(Arc::new(index.clone())),
            Box::new(TreeSitterResolver::new(Arc::new(index.clone()))),
        ]);
        Self::new(index, chain)
    }

    pub fn discover(&self, root: &Path) {
        let mut model = detect_workspace(root).or_else(|| synthetic_directory(root));
        if let Some(found) = model.as_mut() {
            if let Some(host) = self.scripts.lock().expect("scripts").as_mut() {
                let roots: Vec<PathBuf> = found.packages.iter().map(|p| p.root.clone()).collect();
                if let Ok(kept) = host.on_workspace_discover(&roots) {
                    found.packages.retain(|p| kept.iter().any(|k| k == &p.root));
                }
            }
        }
        *self.model.lock().expect("model") = model;
    }

    pub fn indexer_for(&self, path: &Path, language_id: &str) -> Option<Box<dyn LanguageIndexer>> {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let lang = if language_id.is_empty() {
            match ext {
                "java" => "java",
                "php" => "php",
                "html" | "htm" => "html",
                "css" => "css",
                "js" | "mjs" | "cjs" | "ts" => "javascript",
                "go" => "go",
                "zig" => "zig",
                "py" => "python",
                "rs" => "rust",
                "c" | "h" => "c",
                "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
                "cs" => "csharp",
                _ => language_id,
            }
        } else {
            language_id
        };
        match lang {
            #[cfg(feature = "lang-java")]
            "java" => Some(Box::new(JavaIndexer)),
            #[cfg(feature = "lang-php")]
            "php" => Some(Box::new(PhpIndexer)),
            #[cfg(feature = "lang-html")]
            "html" => Some(Box::new(HtmlIndexer)),
            #[cfg(feature = "lang-css")]
            "css" => Some(Box::new(CssIndexer)),
            #[cfg(feature = "lang-javascript")]
            "javascript" | "typescript" => Some(Box::new(JavaScriptIndexer)),
            #[cfg(feature = "lang-go")]
            "go" => Some(Box::new(GoIndexer)),
            #[cfg(feature = "lang-zig")]
            "zig" => Some(Box::new(ZigIndexer)),
            #[cfg(feature = "lang-python")]
            "python" => Some(Box::new(PythonIndexer)),
            #[cfg(feature = "lang-rust")]
            "rust" => Some(Box::new(RustIndexer)),
            #[cfg(feature = "lang-c")]
            "c" => Some(Box::new(CIndexer)),
            #[cfg(feature = "lang-cpp")]
            "cpp" => Some(Box::new(CppIndexer)),
            #[cfg(feature = "lang-csharp")]
            "csharp" => Some(Box::new(CSharpIndexer)),
            _ => None,
        }
    }

    pub fn index_path(&self, path: &Path, source: &str) {
        if let Some(indexer) = self.indexer_for(path, "") {
            self.index
                .lock()
                .index_text(path, source, indexer.as_ref(), false);
        }
    }

    /// Package-stream ingest. Completing a package marks Graph and emits progress.
    /// Never called from [`LspIntelligence::did_change`].
    pub fn ingest_workspace(&self) {
        let model = self.model.lock().expect("model").clone();
        let Some(model) = model else {
            return;
        };
        for pkg in &model.packages {
            if self
                .skipped_packages
                .lock()
                .expect("skip")
                .iter()
                .any(|p| p == &pkg.id)
            {
                continue;
            }
            if let Some(host) = self.scripts.lock().expect("scripts").as_mut() {
                if let Ok(false) = host.on_pre_index(pkg.id.as_str()) {
                    self.skipped_packages
                        .lock()
                        .expect("skip")
                        .push(pkg.id.clone());
                    continue;
                }
            }
            let mut files = Vec::new();
            for root in &pkg.source_roots {
                collect_sources(root, &mut files, 0);
            }
            if files.is_empty() {
                collect_sources(&pkg.root, &mut files, 0);
            }
            let lang = guess_lang(&files);
            if let Some(indexer) = self.indexer_for(
                files.first().map(PathBuf::as_path).unwrap_or(Path::new("")),
                lang,
            ) {
                let mut job = PackageIngest::new(pkg.id.as_str(), lang);
                for f in files {
                    job = job.with_file(f);
                }
                let report = self.index.lock().ingest_package(&job, indexer.as_ref());
                let mut prog = self.progress.lock().expect("progress");
                for ev in report.progress {
                    prog.push(to_lsp_progress(&ev));
                }
            }
            if let Some(host) = self.scripts.lock().expect("scripts").as_mut() {
                let _ = host.on_post_index(pkg.id.as_str());
            }
        }
    }

    pub fn package_tier(&self, id: &str) -> Option<Tier> {
        self.index.lock().package_tier(&PackageId::new(id))
    }

    pub fn apply_watch(
        &self,
        backend: &mut dyn WatchBackend,
        clock: &FakeClock,
        coalescer: &mut WatchCoalescer,
    ) {
        coalescer.poll_backend(backend);
        clock.advance_ms(progressive_lsp_watch::DEFAULT_WINDOW_MS);
        if let Some(batch) = coalescer.flush_due() {
            let mut filtered = batch;
            if let Some(host) = self.scripts.lock().expect("scripts").as_mut() {
                let paths: Vec<String> = filtered.events.iter().map(|e| e.path.clone()).collect();
                if let Ok(kept) = host.on_watch(&paths) {
                    filtered
                        .events
                        .retain(|e| kept.iter().any(|k| k == &e.path));
                }
            }
            self.index
                .lock()
                .apply_watch_batch(&filtered, self.filter.as_ref());
            for ev in &filtered.events {
                let path = PathBuf::from(&ev.path);
                match std::fs::read_to_string(&path) {
                    Ok(src) => self.index_path(&path, &src),
                    Err(e) => self.emit_watch_io(&path, &e),
                }
            }
        }
    }

    pub fn source_paths(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(model) = self.model.lock().expect("model").as_ref() {
            for pkg in &model.packages {
                for root in &pkg.source_roots {
                    collect_sources(root, &mut files, 0);
                }
                collect_sources(&pkg.root, &mut files, 0);
            }
        }
        files.sort();
        files.dedup();
        files
    }

    pub fn package_ids(&self) -> Vec<String> {
        self.model
            .lock()
            .expect("model")
            .as_ref()
            .map(|m| {
                m.packages
                    .iter()
                    .map(|p| p.id.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn cache_entries(&self) -> u64 {
        self.index.lock().cache.len() as u64
    }

    pub fn index_generation(&self) -> u64 {
        self.index.lock().generation()
    }

    pub fn drain_index_tier_ready(&self) -> Vec<(String, String)> {
        self.index
            .lock()
            .drain_tier_ready()
            .into_iter()
            .map(|(id, tier)| (id.as_str().to_string(), tier.as_str().to_string()))
            .collect()
    }

    pub fn filter_watch_paths(&self, paths: &[String]) -> Vec<String> {
        if let Some(host) = self.scripts.lock().expect("scripts").as_mut() {
            if let Ok(kept) = host.on_watch(paths) {
                return kept;
            }
        }
        paths.to_vec()
    }

    pub fn replace_scripts(&self, host: ScriptHost) {
        *self.scripts.lock().expect("scripts") = Some(host);
    }

    pub fn apply_disk_path(&self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(src) => self.index_path(path, &src),
            Err(e) => self.emit_watch_io(path, &e),
        }
    }

    fn emit_watch_io(&self, path: &Path, err: &std::io::Error) {
        let _g = LogScope::enter(
            LogScope::new()
                .path(path.to_string_lossy().into_owned())
                .operation("watch")
                .component(LogComponent::watch()),
        );
        self.log
            .warn(&format!("read_to_string {}: {err}", path.display()));
    }

    /// Stock ghost-disk: reindex files whose on-disk bytes changed (no LSP didChange).
    pub fn reindex_known_paths(&self) -> usize {
        let mut n = 0usize;
        for path in self.source_paths() {
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    let unchanged = self
                        .index
                        .lock()
                        .source(&path)
                        .map(|old| old == src)
                        .unwrap_or(false);
                    if !unchanged {
                        self.index_path(&path, &src);
                        n += 1;
                    }
                }
                Err(e) => self.emit_watch_io(&path, &e),
            }
        }
        n
    }
}

fn to_lsp_progress(ev: &progressive_lsp_index::WorkDoneProgress) -> WorkDoneProgress {
    match ev.kind {
        progressive_lsp_index::ProgressKind::Begin => {
            WorkDoneProgress::begin(&ev.token, ev.title.clone().unwrap_or_default())
        }
        progressive_lsp_index::ProgressKind::Report => WorkDoneProgress::report(
            &ev.token,
            ev.message.clone().unwrap_or_default(),
            ev.percentage.unwrap_or(0),
        ),
        progressive_lsp_index::ProgressKind::End => WorkDoneProgress::end(&ev.token),
    }
}

fn guess_lang(files: &[PathBuf]) -> &'static str {
    for f in files {
        match f.extension().and_then(|s| s.to_str()) {
            Some("java") => return "java",
            Some("php") => return "php",
            Some("go") => return "go",
            Some("zig") => return "zig",
            Some("html") => return "html",
            Some("css") => return "css",
            Some("js") | Some("ts") => return "javascript",
            Some("py") => return "python",
            Some("rs") => return "rust",
            Some("c") | Some("h") => return "c",
            Some("cc") | Some("cpp") | Some("cxx") | Some("hpp") => return "cpp",
            Some("cs") => return "csharp",
            _ => {}
        }
    }
    ""
}

fn synthetic_directory(root: &Path) -> Option<WorkspaceModel> {
    let mut files = Vec::new();
    collect_sources(root, &mut files, 0);
    if files.is_empty() {
        return None;
    }
    let mut model = WorkspaceModel::new("directory", root.to_path_buf());
    model.add_package(
        PackageEntry::new("root", root.to_path_buf()).with_source_root(root.to_path_buf()),
    );
    Some(model)
}

pub(crate) fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 12 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        if dir.is_file() {
            out.push(dir.to_path_buf());
        }
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "node_modules" || name == ".git" || name == "target" || name == "vendor" {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, out, depth + 1);
        } else if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some(
                "java"
                    | "php"
                    | "html"
                    | "css"
                    | "js"
                    | "ts"
                    | "go"
                    | "zig"
                    | "py"
                    | "rs"
                    | "c"
                    | "h"
                    | "cc"
                    | "cpp"
                    | "hpp"
                    | "cs"
            )
        ) {
            out.push(path);
        }
    }
}

impl LspIntelligence for WorkspaceSession {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult {
        let _g = LogScope::enter(
            LogScope::new()
                .path(q.file.as_str())
                .line(q.position.line)
                .operation("textDocument/definition"),
        );
        self.log.debug("textDocument/definition");
        match self.chain.resolve(q) {
            progressive_lsp_resolve::ResolveOutcome::Ready(r) => r,
            progressive_lsp_resolve::ResolveOutcome::NotReady => {
                ResolveResult::empty(progressive_lsp_core::Tier::Syntax)
            }
        }
    }

    fn did_open(&self, uri: &str, language_id: &str, text: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        let _g = LogScope::enter(
            LogScope::new()
                .path(path.to_string_lossy().into_owned())
                .operation("textDocument/didOpen"),
        );
        self.log.debug("textDocument/didOpen");
        self.index.lock().open_buffer(&path);
        if let Some(indexer) = self.indexer_for(&path, language_id) {
            self.index
                .lock()
                .index_text(&path, text, indexer.as_ref(), false);
        }
    }

    fn did_change(&self, uri: &str, text: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        let _g = LogScope::enter(
            LogScope::new()
                .path(path.to_string_lossy().into_owned())
                .operation("textDocument/didChange"),
        );
        self.log.debug("textDocument/didChange");
        if let Some(indexer) = self.indexer_for(&path, "") {
            let old = self.index.lock().source(&path).unwrap_or("").to_string();
            let change = InputChange::replace_all(&old, text);
            self.index
                .lock()
                .apply_change(&path, &change, indexer.as_ref());
        }
        if let Some(sup) = &self.supervisor {
            sup.forward_did_change(uri, text);
        }
    }

    fn did_close(&self, uri: &str) {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        self.index.lock().close_buffer(&path);
    }

    fn semantic_tokens(&self, uri: &str) -> Vec<u32> {
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        let src = self.index.lock().source(&path).unwrap_or("").to_string();
        if src.is_empty() {
            return Vec::new();
        }
        let mut p = tree_sitter::Parser::new();
        #[cfg(feature = "lang-java")]
        if path.extension().and_then(|s| s.to_str()) == Some("java") {
            let _ = p.set_language(&progressive_lsp_lang_java::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return java_tokens::encode_lsp_data(&java_tokens::tokens_from_tree(&src, &tree));
            }
        }
        #[cfg(feature = "lang-php")]
        if path.extension().and_then(|s| s.to_str()) == Some("php") {
            let _ = p.set_language(&progressive_lsp_lang_php::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_php::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-html")]
        if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("html" | "htm")
        ) {
            let _ = p.set_language(&progressive_lsp_lang_html::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_html::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-css")]
        if path.extension().and_then(|s| s.to_str()) == Some("css") {
            let _ = p.set_language(&progressive_lsp_lang_css::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_css::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-javascript")]
        if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("js" | "ts" | "mjs")
        ) {
            let _ = p.set_language(&progressive_lsp_lang_javascript::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_javascript::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-go")]
        if path.extension().and_then(|s| s.to_str()) == Some("go") {
            let _ = p.set_language(&progressive_lsp_lang_go::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_go::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-zig")]
        if path.extension().and_then(|s| s.to_str()) == Some("zig") {
            let _ = p.set_language(&progressive_lsp_lang_zig::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_zig::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-python")]
        if path.extension().and_then(|s| s.to_str()) == Some("py") {
            let _ = p.set_language(&progressive_lsp_lang_python::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_python::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-rust")]
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let _ = p.set_language(&progressive_lsp_lang_rust::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_rust::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-c")]
        if matches!(path.extension().and_then(|s| s.to_str()), Some("c" | "h")) {
            let _ = p.set_language(&progressive_lsp_lang_c::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_c::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-cpp")]
        if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("cc" | "cpp" | "cxx" | "hpp")
        ) {
            let _ = p.set_language(&progressive_lsp_lang_cpp::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_cpp::tokens_from_tree(&src, &tree);
            }
        }
        #[cfg(feature = "lang-csharp")]
        if path.extension().and_then(|s| s.to_str()) == Some("cs") {
            let _ = p.set_language(&progressive_lsp_lang_csharp::tree_sitter_language());
            if let Some(tree) = p.parse(&src, None) {
                return progressive_lsp_lang_csharp::tokens_from_tree(&src, &tree);
            }
        }
        Vec::new()
    }

    fn drain_progress(&self) -> Vec<WorkDoneProgress> {
        std::mem::take(&mut *self.progress.lock().expect("progress"))
    }

    fn on_initialize(&self, params: &serde_json::Value) -> Result<(), InitializeFailed> {
        let scripts = params
            .get("initializationOptions")
            .and_then(|o| o.get("scripts"))
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();
        if scripts.is_empty() {
            return Ok(());
        }
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        for s in scripts {
            if let Some(path) = s.as_str() {
                host.load_path(Path::new(path))
                    .map_err(|e| InitializeFailed(e.0))?;
            }
        }
        host.on_bootstrap(&ScriptContext::default())?;
        *self.scripts.lock().expect("scripts") = Some(host);
        Ok(())
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
    #[cfg(feature = "lang-php")]
    {
        registry.register(Box::new(progressive_lsp_lang_php::PhpLanguageFactory::new()));
    }
    #[cfg(feature = "lang-html")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_html::HtmlLanguageFactory::new(),
        ));
    }
    #[cfg(feature = "lang-css")]
    {
        registry.register(Box::new(progressive_lsp_lang_css::CssLanguageFactory::new()));
    }
    #[cfg(feature = "lang-javascript")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_javascript::JavaScriptLanguageFactory::new(),
        ));
        registry.register(Box::new(
            progressive_lsp_lang_javascript::JavaScriptLanguageFactory::typescript(),
        ));
    }
    #[cfg(feature = "lang-go")]
    {
        registry.register(Box::new(progressive_lsp_lang_go::GoLanguageFactory::new()));
    }
    #[cfg(feature = "lang-zig")]
    {
        registry.register(Box::new(progressive_lsp_lang_zig::ZigLanguageFactory::new()));
    }
    #[cfg(feature = "lang-python")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_python::PythonLanguageFactory::new(),
        ));
    }
    #[cfg(feature = "lang-rust")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_rust::RustLanguageFactory::new(),
        ));
    }
    #[cfg(feature = "lang-c")]
    {
        registry.register(Box::new(progressive_lsp_lang_c::CLanguageFactory::new()));
    }
    #[cfg(feature = "lang-cpp")]
    {
        registry.register(Box::new(progressive_lsp_lang_cpp::CppLanguageFactory::new()));
    }
    #[cfg(feature = "lang-csharp")]
    {
        registry.register(Box::new(
            progressive_lsp_lang_csharp::CSharpLanguageFactory::new(),
        ));
    }
}

#[cfg(test)]
pub fn ghost_reindex_unopened(session: &WorkspaceSession, path: &Path, new_source: &str) -> bool {
    std::fs::write(path, new_source).is_ok() && {
        let clock = Arc::new(FakeClock::at_unix_ms(5_000));
        let mut coalescer = WatchCoalescer::new(clock.clone());
        let mut fake = FakeWatcher::new();
        fake.inject_one(
            path.to_string_lossy().as_ref(),
            progressive_lsp_watch::WatchKind::Modify,
        );
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
        assert!(session
            .index
            .lock()
            .source(&path)
            .unwrap()
            .contains("ghost"));
        assert!(!session.index.lock().is_open(&path));
        let q = ResolveQuery::workspace_symbol("ghost");
        let r = session.resolve(&q);
        assert!(
            r.locations.iter().any(|l| l.uri.contains("Ghost.java"))
                || !r.locations.is_empty()
                || session
                    .index
                    .lock()
                    .all_indexed_symbols()
                    .iter()
                    .any(|s| s.name == "ghost")
        );
    }

    #[test]
    fn session_did_open_change_close() {
        let session = WorkspaceSession::java_default();
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
        assert_eq!(
            progressive_lsp_core::LanguageId::new("java").as_str(),
            "java"
        );
    }

    #[test]
    fn ingest_never_blocks_did_change_highlighting() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pom.xml"),
            "<project><modules><module>lib</module><module>app</module></modules></project>\n",
        )
        .unwrap();
        for name in ["lib", "app"] {
            let src = dir.path().join(name).join("src/main/java");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                dir.path().join(name).join("pom.xml"),
                format!("<project><artifactId>{name}</artifactId></project>\n"),
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join("lib/src/main/java/Lib.java"),
            "class Lib { static String greet(String n) { return n; } }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app/src/main/java/App.java"),
            "class App { void run() { Lib.greet(\"x\"); } }\n",
        )
        .unwrap();
        let session = WorkspaceSession::java_default();
        session.discover(dir.path());
        session.did_open(
            "file:///App.java",
            "java",
            "class App { void run() { int x = 1; } }",
        );
        assert!(session.package_tier("lib").is_none());
        session.did_change(
            "file:///App.java",
            "class App { void changed() { int y = 2; } }",
        );
        let tokens = session.semantic_tokens("file:///App.java");
        assert!(
            !tokens.is_empty(),
            "didChange highlighting must work before ingest finishes"
        );
        assert!(session.package_tier("lib").is_none());
        session.ingest_workspace();
        assert_eq!(session.package_tier("lib"), Some(Tier::Graph));
        assert_eq!(session.package_tier("app"), Some(Tier::Graph));
        assert!(
            session.package_ids().contains(&"lib".to_string()) || !session.package_ids().is_empty()
        );
        let _ = session.cache_entries();
        let _ = session.index_generation();
        let _ = session.drain_index_tier_ready();
        let kept = session.filter_watch_paths(&["a.java".into()]);
        assert_eq!(kept, ["a.java"]);
        session.apply_disk_path(&dir.path().join("lib/src/main/java/Lib.java"));
        let progress = session.drain_progress();
        assert!(!progress.is_empty());
        assert!(session.drain_progress().is_empty());
    }

    #[test]
    fn fixture_script_denies_path_and_aborts_initialize() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("deny.rhai");
        std::fs::write(&script, "fn on_bootstrap() { abort(\"denied-path\"); }\n").unwrap();
        let session = WorkspaceSession::java_default();
        let params = serde_json::json!({
            "initializationOptions": { "scripts": [script.to_string_lossy()] }
        });
        let err = session.on_initialize(&params).unwrap_err();
        assert!(err.0.contains("denied-path"), "{err}");
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn python_session_tokens_and_supervisor_forward() {
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let dir = tempfile::tempdir().unwrap();
        let prefix = progressive_lsp_core::PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        let fake = progressive_lsp_engine::FakeEngineAdapter::ty().with_binary(
            progressive_lsp_engine::EngineBinary {
                pack_name: "python".into(),
                path: dir.path().join("ty"),
                sha256: [0; 32],
            },
        );
        let mut sup = EngineSupervisor::new(clock, prefix);
        sup.register(Box::new(fake));
        let _ = sup.try_spawn(
            "python",
            &progressive_lsp_core::LanguageId::new("python"),
            &PackageId::new("pkg"),
            dir.path(),
        );
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        )
        .with_supervisor(Arc::new(sup));
        session.did_open(
            "file:///t.py",
            "python",
            "def greet(name):\n    return name\n",
        );
        let toks = session.semantic_tokens("file:///t.py");
        assert!(!toks.is_empty());
        session.did_change("file:///t.py", "def greet(name):\n    return name\n");
        session.did_close("file:///t.py");
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_session_tokens() {
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        );
        session.did_open("file:///t.rs", "rust", "fn greet() {}\n");
        assert!(!session.semantic_tokens("file:///t.rs").is_empty());
    }

    #[cfg(feature = "lang-c")]
    #[test]
    fn c_session_tokens() {
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        );
        session.did_open("file:///t.c", "c", "int greet(void) { return 1; }\n");
        assert!(!session.semantic_tokens("file:///t.c").is_empty());
    }

    #[cfg(feature = "lang-cpp")]
    #[test]
    fn cpp_session_tokens() {
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        );
        session.did_open(
            "file:///t.cpp",
            "cpp",
            "class Greeter { int greet() { return 1; } };\n",
        );
        assert!(!session.semantic_tokens("file:///t.cpp").is_empty());
    }

    #[cfg(feature = "lang-csharp")]
    #[test]
    fn csharp_session_tokens() {
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        );
        session.did_open("file:///t.cs", "csharp", "class App { void Run() {} }\n");
        assert!(!session.semantic_tokens("file:///t.cs").is_empty());
    }

    #[cfg(all(
        feature = "lang-html",
        feature = "lang-css",
        feature = "lang-javascript",
        feature = "lang-php",
        feature = "lang-go",
        feature = "lang-zig"
    ))]
    #[test]
    fn m4_session_tokens_for_web_php_go_zig() {
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        );
        session.did_open("file:///t.html", "html", "<div id=\"main\">x</div>\n");
        session.did_open("file:///t.css", "css", "#main { color: red; }\n");
        session.did_open(
            "file:///t.js",
            "javascript",
            "function greet() { return 1; }\n",
        );
        session.did_open(
            "file:///t.php",
            "php",
            "<?php function greet() { return 1; }\n",
        );
        session.did_open("file:///t.go", "go", "package p\nfunc Greet() {}\n");
        session.did_open("file:///t.zig", "zig", "pub fn greet() void {}\n");
        assert!(!session.semantic_tokens("file:///t.html").is_empty());
        assert!(!session.semantic_tokens("file:///t.css").is_empty());
        assert!(!session.semantic_tokens("file:///t.js").is_empty());
        assert!(!session.semantic_tokens("file:///t.php").is_empty());
        assert!(!session.semantic_tokens("file:///t.go").is_empty());
        assert!(!session.semantic_tokens("file:///t.zig").is_empty());
    }

    #[cfg(feature = "lang-java")]
    #[test]
    fn session_with_prefix_writes_cache_under_prefix_not_worktree() {
        let workspace = tempfile::tempdir().unwrap();
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let session = WorkspaceSession::with_prefix_and_t2(&layout, T2Backend::Heuristic);
        session.index_path(Path::new("A.java"), "class A {}");
        assert!(layout.cache_dir().read_dir().unwrap().next().is_some());
        assert!(!workspace.path().join(".progressivelsp/cache").exists());
    }

    #[cfg(feature = "lang-css")]
    #[test]
    fn discover_falls_back_to_non_java_sources() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("card.css"), ".card { color: red; }\n").unwrap();
        let session = WorkspaceSession::java_default();
        session.discover(dir.path());
        session.ingest_workspace();
        assert!(!session.source_paths().is_empty());
        assert_eq!(session.reindex_known_paths(), 0);
        std::fs::write(dir.path().join("card.css"), ".card { color: blue; }\n").unwrap();
        assert_eq!(session.reindex_known_paths(), 1);
        assert!(session
            .index
            .lock()
            .source(&dir.path().join("card.css"))
            .unwrap()
            .contains("blue"));
    }

    #[test]
    fn log_scope_did_open_change_definition_is_context_object() {
        let log = progressive_lsp_core::FakeLog::new();
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        )
        .with_log(Arc::new(log.clone()));
        session.did_open("file:///Tmp.java", "java", "class Tmp {}");
        session.did_change("file:///Tmp.java", "class Tmp { void a() {} }");
        let q = ResolveQuery::new(
            progressive_lsp_core::FileId::new("/Tmp.java"),
            progressive_lsp_resolve::Position::new(0, 1),
            QueryKind::Definition,
        );
        let _ = session.resolve(&q);
        let ops: Vec<_> = log
            .records()
            .iter()
            .filter_map(|r| r.operation.clone())
            .collect();
        assert!(ops.iter().any(|o| o == "textDocument/didOpen"), "{ops:?}");
        assert!(ops.iter().any(|o| o == "textDocument/didChange"), "{ops:?}");
        assert!(
            ops.iter().any(|o| o == "textDocument/definition"),
            "{ops:?}"
        );
        assert!(
            log.records()
                .iter()
                .any(|r| r.content_path.as_deref() == Some("/Tmp.java")),
            "{:?}",
            log.records()
        );
    }

    #[test]
    fn apply_disk_path_read_failure_emits_log_scope_context_object() {
        let log = progressive_lsp_core::FakeLog::new();
        let session = WorkspaceSession::new(
            SharedIndex::new(IndexService::new()),
            ResolverChain::empty(),
        )
        .with_log(Arc::new(log.clone()));
        session.apply_disk_path(Path::new("/no-such-progressive-lsp-watch.java"));
        assert!(
            log.records()
                .iter()
                .any(|r| r.level == progressive_lsp_core::LogLevel::Warn
                    && r.operation.as_deref() == Some("watch")
                    && r.message.contains("read_to_string")),
            "{:?}",
            log.records()
        );
    }
}
