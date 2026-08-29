//! Process-scoped prefix layout. Tests inject the path; never assume `$HOME`.

use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::ids::WorkspaceId;

/// Directory name of the prefix and of the optional project overlay.
pub const PREFIX_DIR_NAME: &str = ".progressivelsp";

const ENV_PREFIX: &str = "PROGRESSIVE_LSP_HOME";

/// On-disk layout under the process prefix (`bin`, `engines`, `cache`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixLayout {
    root: PathBuf,
}

impl PrefixLayout {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }

    /// `--prefix` wins, then `PROGRESSIVE_LSP_HOME`, else `$HOME/.progressivelsp`.
    pub fn resolve(cli_prefix: Option<&Path>) -> Result<Self, ConfigError> {
        if let Some(path) = cli_prefix {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::Prefix("--prefix is empty".into()));
            }
            return Ok(Self::from_path(path));
        }
        match std::env::var(ENV_PREFIX) {
            Ok(value) if !value.is_empty() => Ok(Self::from_path(value)),
            Ok(_) => Err(ConfigError::Prefix(
                "PROGRESSIVE_LSP_HOME is empty".into(),
            )),
            Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Prefix(
                "PROGRESSIVE_LSP_HOME is not valid unicode".into(),
            )),
            Err(std::env::VarError::NotPresent) => {
                let home = std::env::var("HOME").map_err(|e| match e {
                    std::env::VarError::NotPresent => ConfigError::HomeUnset,
                    std::env::VarError::NotUnicode(_) => {
                        ConfigError::Prefix("HOME is not valid unicode".into())
                    }
                })?;
                if home.is_empty() {
                    return Err(ConfigError::HomeUnset);
                }
                Ok(Self::from_path(PathBuf::from(home).join(PREFIX_DIR_NAME)))
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn engines_dir(&self) -> PathBuf {
        self.root.join("engines")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.root.join("log")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.root.join("scripts")
    }

    pub fn workspaces_dir(&self) -> PathBuf {
        self.root.join("workspaces")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn control_socket_path(&self) -> PathBuf {
        self.run_dir().join("control.sock")
    }

    pub fn workspace_dir(&self, id: &WorkspaceId) -> PathBuf {
        self.workspaces_dir().join(id.to_hex())
    }

    pub fn ensure_dirs(&self) -> Result<(), ConfigError> {
        for dir in [
            self.bin_dir(),
            self.engines_dir(),
            self.cache_dir(),
            self.log_dir(),
            self.run_dir(),
            self.scripts_dir(),
            self.workspaces_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| ConfigError::Io(format!("mkdir {}: {e}", dir.display())))?;
        }
        if !self.config_path().exists() {
            std::fs::write(self.config_path(), crate::config::EMPTY_CONFIG_TOML).map_err(|e| {
                ConfigError::Io(format!("write {}: {e}", self.config_path().display()))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn injected_prefix_paths_are_exact() {
        let layout = PrefixLayout::from_path("/tmp/plsp-prefix");
        assert_eq!(layout.root(), Path::new("/tmp/plsp-prefix"));
        assert_eq!(layout.bin_dir(), Path::new("/tmp/plsp-prefix/bin"));
        assert_eq!(layout.engines_dir(), Path::new("/tmp/plsp-prefix/engines"));
        assert_eq!(layout.cache_dir(), Path::new("/tmp/plsp-prefix/cache"));
        assert_eq!(layout.log_dir(), Path::new("/tmp/plsp-prefix/log"));
        assert_eq!(layout.run_dir(), Path::new("/tmp/plsp-prefix/run"));
        assert_eq!(layout.scripts_dir(), Path::new("/tmp/plsp-prefix/scripts"));
        assert_eq!(
            layout.workspaces_dir(),
            Path::new("/tmp/plsp-prefix/workspaces")
        );
        assert_eq!(
            layout.config_path(),
            Path::new("/tmp/plsp-prefix/config.toml")
        );
        assert_eq!(
            layout.control_socket_path(),
            Path::new("/tmp/plsp-prefix/run/control.sock")
        );
        let id = WorkspaceId::from_canonical_bytes(b"abc");
        assert_eq!(
            layout.workspace_dir(&id),
            Path::new("/tmp/plsp-prefix/workspaces").join(id.to_hex())
        );
    }

    #[test]
    fn resolve_cli_prefix_wins_over_env() {
        let _g = lock_env();
        std::env::set_var(ENV_PREFIX, "/from-env");
        let layout = PrefixLayout::resolve(Some(Path::new("/from-cli"))).unwrap();
        assert_eq!(layout.root(), Path::new("/from-cli"));
        std::env::remove_var(ENV_PREFIX);
    }

    #[test]
    fn resolve_empty_cli_prefix_is_error() {
        let _g = lock_env();
        let err = PrefixLayout::resolve(Some(Path::new(""))).unwrap_err();
        assert!(matches!(err, ConfigError::Prefix(_)));
    }

    #[test]
    fn resolve_uses_progressivelsp_home() {
        let _g = lock_env();
        std::env::set_var(ENV_PREFIX, "/home/u/custom");
        let layout = PrefixLayout::resolve(None).unwrap();
        assert_eq!(layout.root(), Path::new("/home/u/custom"));
        std::env::remove_var(ENV_PREFIX);
    }

    #[test]
    fn resolve_empty_progressivelsp_home_is_error() {
        let _g = lock_env();
        std::env::set_var(ENV_PREFIX, "");
        let err = PrefixLayout::resolve(None).unwrap_err();
        assert!(matches!(err, ConfigError::Prefix(_)));
        std::env::remove_var(ENV_PREFIX);
    }

    #[test]
    fn resolve_defaults_to_home_dot_dir() {
        let _g = lock_env();
        std::env::remove_var(ENV_PREFIX);
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/fixture");
        let layout = PrefixLayout::resolve(None).unwrap();
        assert_eq!(
            layout.root(),
            Path::new("/Users/fixture").join(PREFIX_DIR_NAME)
        );
        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn resolve_missing_home_is_error() {
        let _g = lock_env();
        std::env::remove_var(ENV_PREFIX);
        let old_home = std::env::var("HOME").ok();
        std::env::remove_var("HOME");
        let err = PrefixLayout::resolve(None).unwrap_err();
        assert!(matches!(err, ConfigError::HomeUnset));
        std::env::set_var("HOME", "");
        let err = PrefixLayout::resolve(None).unwrap_err();
        assert!(matches!(err, ConfigError::HomeUnset));
        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn ensure_dirs_creates_layout_and_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(dir.path().join("pfx"));
        layout.ensure_dirs().unwrap();
        for p in [
            layout.bin_dir(),
            layout.engines_dir(),
            layout.cache_dir(),
            layout.log_dir(),
            layout.run_dir(),
            layout.scripts_dir(),
            layout.workspaces_dir(),
        ] {
            assert!(p.is_dir(), "{}", p.display());
        }
        let cfg = std::fs::read_to_string(layout.config_path()).unwrap();
        assert_eq!(cfg, crate::config::EMPTY_CONFIG_TOML);
        layout.ensure_dirs().unwrap();
        assert_eq!(
            std::fs::read_to_string(layout.config_path()).unwrap(),
            crate::config::EMPTY_CONFIG_TOML
        );
    }

    #[test]
    fn ensure_dirs_does_not_overwrite_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(dir.path());
        std::fs::write(layout.config_path(), "packs = [\"rust\"]\n").unwrap();
        layout.ensure_dirs().unwrap();
        assert_eq!(
            std::fs::read_to_string(layout.config_path()).unwrap(),
            "packs = [\"rust\"]\n"
        );
    }
}
