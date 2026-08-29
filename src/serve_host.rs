//! Composition-root serve host: prefix config, overlay merge, git exclude.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use progressive_lsp_core::{
    apply_worktree_excludes, Config, ConfigError, ConfigOverlay, InitializeFailed, PrefixLayout,
    OVERLAY_DIR_NAME,
};
use progressive_lsp_protocol::LspIntelligence;
use progressive_lsp_resolve::{ResolveQuery, ResolveResult};
use serde_json::Value;

use crate::WorkspaceSession;

/// Facade over [`WorkspaceSession`] for `serve`: overlay wins, cache stays in prefix.
pub struct ServeHost {
    layout: PrefixLayout,
    config: Mutex<Config>,
    session: WorkspaceSession,
}

impl ServeHost {
    pub fn new(layout: PrefixLayout) -> Result<Self, ConfigError> {
        let config = load_config_file(&layout.config_path())?;
        Ok(Self {
            session: WorkspaceSession::with_prefix(&layout),
            layout,
            config: Mutex::new(config),
        })
    }

    pub fn layout(&self) -> &PrefixLayout {
        &self.layout
    }

    pub fn merged_config(&self) -> Config {
        self.config.lock().expect("config").clone()
    }

    fn apply_workspace(&self, root: &Path) -> Result<(), InitializeFailed> {
        apply_worktree_excludes(root).map_err(|e| InitializeFailed(e.to_string()))?;
        let overlay = root.join(OVERLAY_DIR_NAME).join("config.toml");
        if overlay.exists() {
            let extra = load_overlay_file(&overlay).map_err(|e| InitializeFailed(e.to_string()))?;
            let mut cfg = self.config.lock().expect("config");
            *cfg = cfg.merge(&extra);
        }
        Ok(())
    }
}

impl LspIntelligence for ServeHost {
    fn resolve(&self, q: &ResolveQuery) -> ResolveResult {
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
        self.session.on_initialize(params)
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

fn load_config_file(path: &Path) -> Result<Config, ConfigError> {
    if !path.exists() {
        return Ok(Config::empty());
    }
    let src = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Io(format!("read {}: {e}", path.display())))?;
    Ok(Config::from_toml(&src)?.config)
}

fn load_overlay_file(path: &Path) -> Result<ConfigOverlay, ConfigError> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Io(format!("read {}: {e}", path.display())))?;
    ConfigOverlay::parse(&src)
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
        assert_eq!(root_from_params(&serde_json::json!({"rootUri": "null"})), None);
        assert_eq!(
            root_from_params(&serde_json::json!({"rootPath": ""})),
            None
        );
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
    fn missing_prefix_config_is_empty() {
        let prefix = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(prefix.path().join("missing-cfg"));
        std::fs::create_dir_all(layout.root()).unwrap();
        let host = ServeHost::new(layout).unwrap();
        assert_eq!(host.merged_config(), Config::empty());
    }
}
