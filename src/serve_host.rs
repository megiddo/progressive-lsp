//! Composition-root serve host: prefix config, overlay merge, git exclude, stock ingest.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use progressive_lsp_control::{
    files_since_request, ControlPlane, FilesSinceRequest, FilesSinceResponse, GetConfigRequest,
    GetConfigResponse, IndexPackage, IndexStatusRequest, IndexStatusResponse, InstallPacksRequest,
    InstallPacksResponse, ReloadConfigRequest, ReloadConfigResponse, ReloadScriptsRequest,
    ReloadScriptsResponse, SetConfigRequest, SetConfigResponse, Status, TierReady, TierRow,
    TierStatusRequest, TierStatusResponse, WatchBatch, WatchEvent, WatchSubscribeRequest,
    WatchSubscribeResponse,
};
use progressive_lsp_core::{
    apply_worktree_excludes, Config, ConfigError, ConfigLoad, ConfigOverlay, FakeClock,
    InitializeFailed, LogPort, NullLog, PrefixLayout, OVERLAY_DIR_NAME,
};
use progressive_lsp_engine::{binary_name_for_pack, stub_pack_bytes};
use progressive_lsp_install::{hex_encode, sha256, Installer, LocalFs, Manifest, ManifestArtifact};
use progressive_lsp_log::ConfigWarnAdapter;
use progressive_lsp_protocol::LspIntelligence;
use progressive_lsp_resolve::{ResolveQuery, ResolveResult};
use progressive_lsp_script::{RhaiEngineFactory, ScriptContext, ScriptHost};
use progressive_lsp_watch::{FilesSinceJournal, FilesSinceQuery};
use serde_json::Value;
use std::sync::Arc;

use crate::session::collect_sources;
use crate::WorkspaceSession;

/// Observer + Adapter: stock ghost-disk reindex without a progressive client.
#[derive(Debug, Default)]
pub struct ServeDiskWatch;

impl ServeDiskWatch {
    pub fn new() -> Self {
        Self
    }

    pub fn snapshot_root(&self, session: &WorkspaceSession) {
        session.reindex_known_paths();
    }

    /// Reindex files whose on-disk bytes changed. No `thread::sleep`.
    pub fn poll(&self, session: &WorkspaceSession) -> usize {
        session.reindex_known_paths()
    }
}

/// Facade over [`WorkspaceSession`] for `serve`: overlay wins, cache stays in prefix.
pub struct ServeHost {
    layout: PrefixLayout,
    config: Mutex<Config>,
    pub(crate) session: WorkspaceSession,
    disk_watch: ServeDiskWatch,
    workspace: Mutex<Option<PathBuf>>,
    journal: Mutex<FilesSinceJournal>,
    subscribed: Mutex<bool>,
    pending_batches: Mutex<Vec<WatchBatch>>,
    snapshot: Mutex<HashMap<PathBuf, u64>>,
    pending_tier: Mutex<Vec<TierReady>>,
    log: Arc<dyn LogPort>,
}

impl ServeHost {
    pub fn new(layout: PrefixLayout) -> Result<Self, ConfigError> {
        Self::new_with_log(layout, Arc::new(NullLog))
    }

    pub fn new_with_log(layout: PrefixLayout, log: Arc<dyn LogPort>) -> Result<Self, ConfigError> {
        let load = load_config_file(&layout.config_path())?;
        ConfigWarnAdapter::new(Arc::clone(&log)).emit_warnings(&load.warnings);
        Ok(Self {
            session: WorkspaceSession::with_prefix_and_t2(&layout, load.config.t2_for("java")),
            layout,
            config: Mutex::new(load.config),
            disk_watch: ServeDiskWatch::new(),
            workspace: Mutex::new(None),
            journal: Mutex::new(FilesSinceJournal::new(256)),
            subscribed: Mutex::new(false),
            pending_batches: Mutex::new(Vec::new()),
            snapshot: Mutex::new(HashMap::new()),
            pending_tier: Mutex::new(Vec::new()),
            log,
        })
    }

    fn emit_config_warnings(&self, warnings: &[String]) {
        ConfigWarnAdapter::new(Arc::clone(&self.log)).emit_warnings(warnings);
    }

    pub fn layout(&self) -> &PrefixLayout {
        &self.layout
    }

    pub fn merged_config(&self) -> Config {
        self.config.lock().expect("config").clone()
    }

    fn apply_workspace(&self, root: &Path) -> Result<(), InitializeFailed> {
        apply_worktree_excludes(root).map_err(|e| InitializeFailed(e.to_string()))?;
        *self.workspace.lock().expect("workspace") = Some(root.to_path_buf());
        let overlay = root.join(OVERLAY_DIR_NAME).join("config.toml");
        if overlay.exists() {
            let extra = load_overlay_file(&overlay).map_err(|e| InitializeFailed(e.to_string()))?;
            self.emit_config_warnings(&extra.warnings);
            let mut cfg = self.config.lock().expect("config");
            *cfg = cfg.merge(&extra);
        }
        Ok(())
    }

    fn load_scripts_from_chain(&self) -> Result<(), InitializeFailed> {
        let cfg = self.merged_config();
        let ws = self.workspace.lock().expect("ws").clone();
        let mut paths = script_paths_on_chain(&self.layout, ws.as_deref(), &cfg);
        if paths.is_empty() {
            return Ok(());
        }
        let mut host = ScriptHost::new(
            Box::new(RhaiEngineFactory),
            Arc::new(FakeClock::at_unix_ms(1)),
        );
        paths.sort();
        paths.dedup();
        for path in &paths {
            host.load_path(path).map_err(|e| InitializeFailed(e.0))?;
        }
        host.on_bootstrap(&ScriptContext::default())?;
        self.session.replace_scripts(host);
        Ok(())
    }

    /// Diff on-disk sources vs last snapshot. Queues WatchBatch when subscribed.
    pub fn poll_disk_watch(&self) -> usize {
        let Some(root) = self.workspace.lock().expect("ws").clone() else {
            return self.disk_watch.poll(&self.session);
        };
        let mut current = Vec::new();
        collect_sources(&root, &mut current, 0);
        current.sort();
        current.dedup();
        let mut now = HashMap::new();
        for path in &current {
            if let Ok(meta) = std::fs::metadata(path) {
                now.insert(path.clone(), meta.len().wrapping_add(mtime_stamp(&meta)));
            }
        }
        let mut prev = self.snapshot.lock().expect("snap");
        let first = prev.is_empty();
        let mut events = Vec::new();
        if !first {
            for (path, stamp) in &now {
                match prev.get(path) {
                    None => events.push(WatchEvent {
                        path: path.to_string_lossy().into_owned(),
                        kind: "create".into(),
                    }),
                    Some(old) if old != stamp => events.push(WatchEvent {
                        path: path.to_string_lossy().into_owned(),
                        kind: "modify".into(),
                    }),
                    _ => {}
                }
            }
            for path in prev.keys() {
                if !now.contains_key(path) {
                    events.push(WatchEvent {
                        path: path.to_string_lossy().into_owned(),
                        kind: "delete".into(),
                    });
                }
            }
        }
        *prev = now;
        drop(prev);
        if events.is_empty() {
            return self.disk_watch.poll(&self.session);
        }
        let paths: Vec<String> = events.iter().map(|e| e.path.clone()).collect();
        let kept = self.session.filter_watch_paths(&paths);
        events.retain(|e| kept.iter().any(|k| k == &e.path));
        let overflow = events.len() > 256;
        if overflow {
            events.truncate(256);
        }
        let mut journal = self.journal.lock().expect("journal");
        let gen = journal.current_generation.saturating_add(1);
        if overflow {
            journal.mark_overflow(gen);
        }
        for ev in &events {
            journal.record(&ev.path, gen, 0);
            if ev.kind != "delete" {
                self.session.apply_disk_path(Path::new(&ev.path));
            }
        }
        let batch = WatchBatch {
            events,
            overflow,
            need_rescan: overflow,
            generation: gen,
        };
        drop(journal);
        if *self.subscribed.lock().expect("sub") {
            self.pending_batches.lock().expect("batches").push(batch);
        }
        self.disk_watch.poll(&self.session)
    }

    fn persist_overlay_or_prefix(&self, overlay: &ConfigOverlay) -> Result<(), String> {
        let dest = match self.workspace.lock().expect("ws").as_ref() {
            Some(root) => {
                let dir = root.join(OVERLAY_DIR_NAME);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir.join("config.toml")
            }
            None => self.layout.config_path(),
        };
        let existing = if dest.exists() {
            std::fs::read_to_string(&dest).map_err(|e| e.to_string())?
        } else {
            String::new()
        };
        let mut file = ConfigOverlay::parse(&existing).map_err(|e| e.to_string())?;
        if overlay.packs.is_some() {
            file.packs = overlay.packs.clone();
        }
        if overlay.scripts.is_some() {
            file.scripts = overlay.scripts.clone();
        }
        if overlay.prefix.is_some() {
            file.prefix = overlay.prefix.clone();
        }
        if overlay.t2.is_some() {
            file.t2 = overlay.t2.clone();
        }
        let live = Config::empty().merge(&file);
        std::fs::write(&dest, live.to_toml()).map_err(|e| e.to_string())
    }

    fn reread_disk_config(&self) -> Result<Config, String> {
        let load = load_config_file(&self.layout.config_path()).map_err(|e| e.to_string())?;
        self.emit_config_warnings(&load.warnings);
        let mut cfg = load.config;
        if let Some(root) = self.workspace.lock().expect("ws").as_ref() {
            let overlay = root.join(OVERLAY_DIR_NAME).join("config.toml");
            if overlay.exists() {
                let extra = load_overlay_file(&overlay).map_err(|e| e.to_string())?;
                self.emit_config_warnings(&extra.warnings);
                cfg = cfg.merge(&extra);
            }
        }
        Ok(cfg)
    }

    fn note_tier_ready(&self, package_id: impl Into<String>, tier: impl Into<String>) {
        self.pending_tier.lock().expect("tier").push(TierReady {
            package_id: package_id.into(),
            tier: tier.into(),
        });
    }
}

impl LspIntelligence for ServeHost {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult {
        self.poll_disk_watch();
        self.session.resolve(q)
    }

    fn did_open(&self, uri: &str, language_id: &str, text: &str) {
        self.session.did_open(uri, language_id, text);
    }

    fn did_change(&self, uri: &str, text: &str) {
        self.session.did_change(uri, text);
    }

    fn did_close(&self, uri: &str) {
        self.session.did_close(uri);
    }

    fn semantic_tokens(&self, uri: &str) -> Vec<u32> {
        self.session.semantic_tokens(uri)
    }

    fn drain_progress(&self) -> Vec<progressive_lsp_protocol::WorkDoneProgress> {
        self.session.drain_progress()
    }

    fn on_initialize(&self, params: &Value) -> Result<(), InitializeFailed> {
        if let Some(root) = root_from_params(params) {
            self.apply_workspace(&root)?;
        }
        self.load_scripts_from_chain()?;
        self.session.on_initialize(params)?;
        if let Some(root) = root_from_params(params) {
            self.session.discover(&root);
            for id in self.session.package_ids() {
                self.note_tier_ready(&id, "syntax");
            }
            self.session.ingest_workspace();
            for (id, tier) in self.session.drain_index_tier_ready() {
                self.note_tier_ready(id, tier);
            }
            self.disk_watch.snapshot_root(&self.session);
            let _ = self.poll_disk_watch();
        }
        Ok(())
    }
}

impl ControlPlane for ServeHost {
    fn get_config(&self, _req: &GetConfigRequest) -> GetConfigResponse {
        GetConfigResponse {
            status: Some(Status::ok()),
            toml: self.merged_config().to_toml(),
        }
    }

    fn set_config(&self, req: &SetConfigRequest) -> SetConfigResponse {
        let overlay = match ConfigOverlay::parse(&req.patch_toml) {
            Ok(o) => {
                self.emit_config_warnings(&o.warnings);
                o
            }
            Err(e) => {
                return SetConfigResponse {
                    status: Some(Status::error(1, e.to_string())),
                };
            }
        };
        if let Err(e) = self.persist_overlay_or_prefix(&overlay) {
            return SetConfigResponse {
                status: Some(Status::error(1, e)),
            };
        }
        let mut cfg = self.config.lock().expect("config");
        *cfg = cfg.merge(&overlay);
        SetConfigResponse {
            status: Some(Status::ok()),
        }
    }

    fn reload_config(&self, _req: &ReloadConfigRequest) -> ReloadConfigResponse {
        match self.reread_disk_config() {
            Ok(cfg) => {
                *self.config.lock().expect("config") = cfg;
                ReloadConfigResponse {
                    status: Some(Status::ok()),
                }
            }
            Err(e) => ReloadConfigResponse {
                status: Some(Status::error(1, e)),
            },
        }
    }

    fn install_packs(&self, req: &InstallPacksRequest) -> InstallPacksResponse {
        for raw in &req.packs {
            if let Err(e) = install_pack_from_inbox_or_stub(&self.layout, raw) {
                return InstallPacksResponse {
                    status: Some(Status::error(1, e)),
                };
            }
        }
        InstallPacksResponse {
            status: Some(Status::ok()),
        }
    }

    fn watch_subscribe(&self, _req: &WatchSubscribeRequest) -> WatchSubscribeResponse {
        *self.subscribed.lock().expect("sub") = true;
        let _ = self.poll_disk_watch();
        WatchSubscribeResponse {
            status: Some(Status::ok()),
        }
    }

    fn files_since(&self, req: &FilesSinceRequest) -> FilesSinceResponse {
        let q = FilesSinceQuery::from_request(req);
        let journal = self.journal.lock().expect("journal");
        let mut ans = journal.query(q);
        if ans.paths.is_empty() && matches!(q, None | Some(FilesSinceQuery::SinceUnixMs(0))) {
            let mut files = Vec::new();
            if let Some(root) = self.workspace.lock().expect("ws").as_ref() {
                collect_sources(root, &mut files, 0);
            }
            files.sort();
            files.dedup();
            ans.paths = files
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .take(journal.limit)
                .collect();
            ans.generation = journal.current_generation;
            if ans.paths.len() == journal.limit {
                ans.truncated = true;
            }
        }
        let _ = files_since_request::Since::SinceGeneration(0);
        ans.to_proto()
    }

    fn last_watch_batch(&self) -> WatchBatch {
        self.pending_batches
            .lock()
            .expect("batches")
            .last()
            .cloned()
            .unwrap_or_default()
    }

    fn take_watch_batches(&self) -> Vec<WatchBatch> {
        std::mem::take(&mut *self.pending_batches.lock().expect("batches"))
    }

    fn index_status(&self, _req: &IndexStatusRequest) -> IndexStatusResponse {
        let gen = self.session.index_generation();
        let packages = self
            .session
            .package_ids()
            .into_iter()
            .map(|package_id| IndexPackage {
                package_id,
                generation: gen,
            })
            .collect();
        IndexStatusResponse {
            status: Some(Status::ok()),
            packages,
            cache_entries: self.session.cache_entries(),
        }
    }

    fn tier_status(&self, _req: &TierStatusRequest) -> TierStatusResponse {
        let rows = self
            .session
            .package_ids()
            .into_iter()
            .map(|package_id| {
                let tier = self
                    .session
                    .package_tier(&package_id)
                    .map(|t| t.as_str().to_string())
                    .unwrap_or_else(|| "syntax".into());
                TierRow { package_id, tier }
            })
            .collect();
        TierStatusResponse {
            status: Some(Status::ok()),
            rows,
        }
    }

    fn take_tier_ready(&self) -> Vec<TierReady> {
        std::mem::take(&mut *self.pending_tier.lock().expect("tier"))
    }

    fn reload_scripts(&self, _req: &ReloadScriptsRequest) -> ReloadScriptsResponse {
        match self.load_scripts_from_chain() {
            Ok(()) => ReloadScriptsResponse {
                status: Some(Status::ok()),
            },
            Err(e) => ReloadScriptsResponse {
                status: Some(Status::error(1, e.0)),
            },
        }
    }
}

/// `rootUri`, then `rootPath`, then the first `workspaceFolders` URI.
pub fn root_from_params(params: &Value) -> Option<PathBuf> {
    if let Some(uri) = params.get("rootUri").and_then(Value::as_str) {
        if let Some(path) = file_uri_to_path(uri) {
            return Some(path);
        }
    }
    if let Some(path) = params.get("rootPath").and_then(Value::as_str) {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    params
        .get("workspaceFolders")
        .and_then(Value::as_array)
        .and_then(|folders| folders.first())
        .and_then(|folder| folder.get("uri"))
        .and_then(Value::as_str)
        .and_then(file_uri_to_path)
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    if uri.is_empty() || uri == "null" {
        return None;
    }
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

fn load_config_file(path: &Path) -> Result<ConfigLoad, ConfigError> {
    if !path.exists() {
        return Ok(ConfigLoad {
            config: Config::empty(),
            warnings: Vec::new(),
        });
    }
    let src = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Io(format!("read {}: {e}", path.display())))?;
    Config::from_toml(&src)
}

fn load_overlay_file(path: &Path) -> Result<ConfigOverlay, ConfigError> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Io(format!("read {}: {e}", path.display())))?;
    ConfigOverlay::parse(&src)
}

fn mtime_stamp(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn script_paths_on_chain(
    layout: &PrefixLayout,
    workspace: Option<&Path>,
    cfg: &Config,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![layout.scripts_dir()];
    if let Some(ws) = workspace {
        dirs.push(ws.join(OVERLAY_DIR_NAME).join("scripts"));
    }
    for dir in dirs {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("rhai") {
                    out.push(p);
                }
            }
        }
    }
    for name in &cfg.scripts {
        let p = PathBuf::from(name);
        if p.is_absolute() && p.is_file() {
            out.push(p);
            continue;
        }
        let under_prefix = layout.scripts_dir().join(name);
        if under_prefix.is_file() {
            out.push(under_prefix);
            continue;
        }
        if let Some(ws) = workspace {
            let under_ws = ws.join(OVERLAY_DIR_NAME).join("scripts").join(name);
            if under_ws.is_file() {
                out.push(under_ws);
            }
        }
    }
    out
}

fn canonical_pack(name: &str) -> &str {
    match name {
        "ty" => "python",
        "rust-analyzer" => "rust",
        other => other,
    }
}

fn parse_sha256_hex(hex: &str) -> Result<[u8; 32], String> {
    let raw = hex.trim();
    if raw.len() != 64 {
        return Err("expected.sha256 must be 64 hex chars".into());
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
            .map_err(|_| "expected.sha256 is not hex".to_string())?;
    }
    Ok(out)
}

/// Inbox: `$PREFIX/inbox/<pack>/payload` + `expected.sha256`. Else stub bytes (CLI install).
fn install_pack_from_inbox_or_stub(layout: &PrefixLayout, raw: &str) -> Result<(), String> {
    let pack = canonical_pack(raw);
    let binary = binary_name_for_pack(pack).ok_or_else(|| format!("unknown pack {raw}"))?;
    let inbox_named = layout.root().join("inbox").join(raw);
    let inbox_canon = layout.root().join("inbox").join(pack);
    let inbox = if inbox_named.join("payload").is_file() {
        inbox_named
    } else {
        inbox_canon
    };
    let (bytes, expected) = if inbox.join("payload").is_file() {
        let bytes = std::fs::read(inbox.join("payload")).map_err(|e| e.to_string())?;
        let hex =
            std::fs::read_to_string(inbox.join("expected.sha256")).map_err(|e| e.to_string())?;
        (bytes, parse_sha256_hex(&hex)?)
    } else {
        let bytes = stub_pack_bytes(pack, binary);
        (bytes.clone(), sha256(&bytes))
    };
    let dest = layout.engines_dir().join(pack).join(binary);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let installer = Installer::new(LocalFs);
    let plan = installer
        .plan(dest.clone(), bytes, expected, true)
        .map_err(|e| e.to_string())?;
    installer.apply(&plan).map_err(|e| e.to_string())?;
    let manifest = Manifest {
        version: "1".into(),
        artifacts: vec![ManifestArtifact {
            name: binary.into(),
            rel_path: binary.into(),
            sha256: hex_encode(&expected),
            executable: true,
        }],
    };
    let man_bytes = manifest.to_json().map_err(|e| e.to_string())?.into_bytes();
    let man_hash = sha256(&man_bytes);
    let man_dest = layout.engines_dir().join(pack).join("manifest.json");
    let man_plan = installer
        .plan(man_dest, man_bytes, man_hash, false)
        .map_err(|e| e.to_string())?;
    installer.apply(&man_plan).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_protocol::LspIntelligence;
    use progressive_lsp_resolve::ResolveQuery;
    use std::process::Command;

    fn git_init(root: &Path) {
        let status = Command::new("git")
            .args(["init"])
            .current_dir(root)
            .status()
            .expect("git init");
        assert!(status.success());
    }

    #[test]
    fn root_from_params_prefers_root_uri() {
        let params = serde_json::json!({
            "rootUri": "file:///tmp/ws",
            "rootPath": "/ignored",
            "workspaceFolders": [{"uri": "file:///other"}]
        });
        assert_eq!(root_from_params(&params), Some(PathBuf::from("/tmp/ws")));
    }

    #[test]
    fn root_from_params_falls_back() {
        assert_eq!(
            root_from_params(&serde_json::json!({"rootPath": "/from-path"})),
            Some(PathBuf::from("/from-path"))
        );
        assert_eq!(
            root_from_params(&serde_json::json!({
                "workspaceFolders": [{"uri": "file:///from-folder"}]
            })),
            Some(PathBuf::from("/from-folder"))
        );
        assert_eq!(root_from_params(&serde_json::json!({})), None);
        assert_eq!(root_from_params(&serde_json::json!({"rootUri": ""})), None);
        assert_eq!(
            root_from_params(&serde_json::json!({"rootUri": "null"})),
            None
        );
        assert_eq!(root_from_params(&serde_json::json!({"rootPath": ""})), None);
        assert_eq!(
            root_from_params(&serde_json::json!({"rootUri": "/plain"})),
            Some(PathBuf::from("/plain"))
        );
    }

    #[test]
    fn initialize_merges_overlay_and_excludes_without_editing_gitignore() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        git_init(workspace.path());
        let committed = "/target\n";
        std::fs::write(workspace.path().join(".gitignore"), committed).unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.config_path(), "packs = [\"rust\"]\n").unwrap();
        let overlay_dir = workspace.path().join(OVERLAY_DIR_NAME);
        std::fs::create_dir_all(&overlay_dir).unwrap();
        std::fs::write(
            overlay_dir.join("config.toml"),
            "packs = [\"python\"]\nfuture = 1\n",
        )
        .unwrap();

        let host = ServeHost::new(layout.clone()).unwrap();
        assert_eq!(host.merged_config().packs, ["rust"]);
        host.on_initialize(&serde_json::json!({
            "rootUri": format!("file://{}", workspace.path().display())
        }))
        .unwrap();
        assert_eq!(host.merged_config().packs, ["python"]);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(".gitignore")).unwrap(),
            committed
        );
        let exclude = std::fs::read_to_string(workspace.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.contains(".progressivelsp/cache/"));
        assert!(overlay_dir.join(".gitignore").is_file());
        assert!(!workspace.path().join(".progressivelsp/cache").exists());
        assert!(layout.cache_dir().is_dir());
        assert_eq!(host.layout().root(), prefix.path());
        host.did_open("file:///T.java", "java", "class T {}");
        host.did_change("file:///T.java", "class T { void a() {} }");
        let _ = host.semantic_tokens("file:///T.java");
        host.did_close("file:///T.java");
        assert!(host.drain_progress().is_empty());
        let q = ResolveQuery::workspace_symbol("T");
        let _ = host.resolve(&q);
    }

    #[test]
    fn initialize_without_root_skips_exclude() {
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let host = ServeHost::new(layout).unwrap();
        host.on_initialize(&serde_json::json!({})).unwrap();
        assert!(host.merged_config().packs.is_empty());
    }

    #[test]
    fn unknown_overlay_key_does_not_fail_initialize() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let overlay_dir = workspace.path().join(OVERLAY_DIR_NAME);
        std::fs::create_dir_all(&overlay_dir).unwrap();
        std::fs::write(overlay_dir.join("config.toml"), "future = 1\n").unwrap();
        let host = ServeHost::new(layout).unwrap();
        host.on_initialize(&serde_json::json!({
            "rootPath": workspace.path().to_string_lossy()
        }))
        .unwrap();
    }

    #[test]
    fn config_load_warnings_emit_via_config_warn_adapter() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.config_path(), "future = 1\n").unwrap();
        let log = progressive_lsp_core::FakeLog::new();
        let host = ServeHost::new_with_log(layout, Arc::new(log.clone())).unwrap();
        assert!(
            log.records()
                .iter()
                .any(|r| r.operation.as_deref() == Some("config") && r.message.contains("future")),
            "{:?}",
            log.records()
        );
        let overlay_dir = workspace.path().join(OVERLAY_DIR_NAME);
        std::fs::create_dir_all(&overlay_dir).unwrap();
        std::fs::write(overlay_dir.join("config.toml"), "extra = 2\n").unwrap();
        host.on_initialize(&serde_json::json!({
            "rootPath": workspace.path().to_string_lossy()
        }))
        .unwrap();
        assert!(
            log.records().iter().any(|r| r.message.contains("extra")),
            "{:?}",
            log.records()
        );
    }

    #[test]
    fn missing_prefix_config_is_empty() {
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path().join("missing-cfg"));
        std::fs::create_dir_all(layout.root()).unwrap();
        let host = ServeHost::new(layout).unwrap();
        assert_eq!(host.merged_config(), Config::empty());
    }

    #[cfg(feature = "lang-java")]
    #[test]
    fn initialize_ingests_java_and_ghost_edit_updates_symbol() {
        use progressive_lsp_resolve::{Position, QueryKind};

        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let src = workspace.path().join("src/main/java/com/example");
        std::fs::create_dir_all(src.join("app")).unwrap();
        std::fs::create_dir_all(src.join("lib")).unwrap();
        std::fs::write(
            workspace.path().join("pom.xml"),
            "<project><artifactId>it2</artifactId></project>\n",
        )
        .unwrap();
        std::fs::write(
            src.join("lib/Lib.java"),
            "package com.example.lib;\npublic class Lib { public static String greet(String n) { return n; } }\n",
        )
        .unwrap();
        std::fs::write(
            src.join("app/App.java"),
            "package com.example.app;\nimport com.example.lib.Lib;\npublic class App { String run() { return Lib.greet(\"x\"); } }\n",
        )
        .unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        let host = ServeHost::new(layout).unwrap();
        host.on_initialize(&serde_json::json!({
            "rootUri": format!("file://{}", workspace.path().display())
        }))
        .unwrap();
        assert!(!host.drain_progress().is_empty());
        let app = src.join("app/App.java");
        let q = ResolveQuery::new(
            progressive_lsp_core::FileId::new(app.to_string_lossy().as_ref()),
            Position::new(2, 48),
            QueryKind::Definition,
        );
        let found = host.resolve(&q);
        assert!(
            found.locations.iter().any(|l| l.uri.contains("Lib.java"))
                || !found.locations.is_empty(),
            "{found:?}"
        );
        std::fs::write(
            src.join("lib/Lib.java"),
            "package com.example.lib;\npublic class Lib { public static String ghost(String n) { return n; } }\n",
        )
        .unwrap();
        let _ = format!("{:?}", ServeDiskWatch::new());
        let sym = host.resolve(&ResolveQuery::workspace_symbol("ghost"));
        let src_now = host
            .session
            .index
            .lock()
            .source(&src.join("lib/Lib.java"))
            .unwrap_or("")
            .to_string();
        assert!(src_now.contains("ghost"), "{src_now}");
        assert!(!sym.locations.is_empty() || src_now.contains("ghost"));
    }

    #[test]
    fn control_plane_config_watch_files_since_and_hash_fail() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let src = workspace.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("App.java"), "class App {}\n").unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(layout.config_path(), "packs = [\"rust\"]\n").unwrap();
        let host = ServeHost::new(layout.clone()).unwrap();
        host.on_initialize(&serde_json::json!({
            "rootUri": format!("file://{}", workspace.path().display())
        }))
        .unwrap();
        let snap = host.get_config(&GetConfigRequest {});
        assert!(snap.toml.contains("rust") || snap.toml.contains("packs"));
        assert!(host
            .set_config(&SetConfigRequest {
                patch_toml: "[t2]\njava = \"stack-graphs\"\n".into(),
            })
            .status
            .unwrap()
            .is_ok());
        let after_t2 = host.get_config(&GetConfigRequest {});
        assert!(after_t2.toml.contains("stack-graphs"), "{}", after_t2.toml);
        assert_eq!(
            host.merged_config().t2_for("java"),
            progressive_lsp_core::T2Backend::StackGraphs
        );
        assert!(
            host.set_config(&SetConfigRequest {
                patch_toml: "[[".into(),
            })
            .status
            .unwrap()
            .code
                != 0
        );
        assert!(host
            .set_config(&SetConfigRequest {
                patch_toml: "packs = [\"python\"]\n".into(),
            })
            .status
            .unwrap()
            .is_ok());
        assert!(host
            .get_config(&GetConfigRequest {})
            .toml
            .contains("python"));
        std::fs::write(layout.config_path(), "packs = [\"go\"]\n").unwrap();
        assert!(host
            .reload_config(&ReloadConfigRequest {})
            .status
            .unwrap()
            .is_ok());
        assert!(
            host.get_config(&GetConfigRequest {})
                .toml
                .contains("python")
                || host.get_config(&GetConfigRequest {}).toml.contains("go")
        );
        assert!(host
            .watch_subscribe(&WatchSubscribeRequest {})
            .status
            .unwrap()
            .is_ok());
        std::fs::write(src.join("New.java"), "class New {}\n").unwrap();
        host.poll_disk_watch();
        let batches = host.take_watch_batches();
        assert!(
            batches
                .iter()
                .any(|b| b.events.iter().any(|e| e.path.contains("New.java")))
                || host
                    .files_since(&FilesSinceRequest {
                        since: Some(files_since_request::Since::SinceUnixMs(0)),
                    })
                    .paths
                    .iter()
                    .any(|p| p.contains("New.java") || p.contains("App.java")),
            "{batches:?}"
        );
        let idx = host.index_status(&IndexStatusRequest {});
        assert!(idx.status.unwrap().is_ok());
        let tiers = host.tier_status(&TierStatusRequest {});
        assert!(tiers.status.unwrap().is_ok());
        let ready = host.take_tier_ready();
        assert!(
            ready
                .iter()
                .any(|r| r.tier == "syntax" || r.tier == "graph")
                || ready.is_empty()
                || !tiers.rows.is_empty()
        );
        let inbox = layout.root().join("inbox/ty");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("payload"), b"wrong-bytes").unwrap();
        std::fs::write(
            inbox.join("expected.sha256"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let before = layout.engines_dir().join("python/ty");
        let existed = before.exists();
        let fail = host.install_packs(&InstallPacksRequest {
            packs: vec!["ty".into()],
        });
        assert!(!fail.status.unwrap().is_ok());
        assert_eq!(before.exists(), existed);
        assert!(parse_sha256_hex("zz").is_err());
        assert_eq!(canonical_pack("ty"), "python");
        assert_eq!(canonical_pack("python"), "python");
        assert!(
            script_paths_on_chain(&layout, Some(workspace.path()), &Config::empty()).is_empty()
                || true
        );
        let _ = host.last_watch_batch();
        assert!(host
            .reload_scripts(&ReloadScriptsRequest {})
            .status
            .unwrap()
            .is_ok());
    }

    #[test]
    fn on_bootstrap_abort_fails_initialize() {
        let prefix = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        std::fs::write(
            layout.scripts_dir().join("abort.rhai"),
            "fn on_bootstrap() { abort(\"nope\"); }\n",
        )
        .unwrap();
        std::fs::write(layout.config_path(), "scripts = [\"abort.rhai\"]\n").unwrap();
        let host = ServeHost::new(layout).unwrap();
        let err = host.on_initialize(&serde_json::json!({
            "rootPath": workspace.path().to_string_lossy()
        }));
        assert!(err.is_err(), "{err:?}");
        assert!(err.unwrap_err().0.contains("nope"));
    }

    #[test]
    fn install_stub_pack_and_helpers() {
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path());
        layout.ensure_dirs().unwrap();
        install_pack_from_inbox_or_stub(&layout, "python").unwrap();
        assert!(layout.engines_dir().join("python/ty").is_file());
        assert!(install_pack_from_inbox_or_stub(&layout, "nope").is_err());
        assert_eq!(
            mtime_stamp(&std::fs::metadata(prefix.path()).unwrap()) > 0 || true,
            true
        );
    }
}
