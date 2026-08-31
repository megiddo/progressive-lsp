//! [`ServeLogPath`] — Value object for the per-process WAL file name.

use std::path::{Path, PathBuf};

/// Env override for the sqlite file. Empty / unset → [`ServeLogPath::new`].
pub const ENV_LOG_PATH: &str = "PROGRESSIVE_LSP_LOG";

/// Value object. `{log_dir}/serve-{unix_ms}-{pid}.sqlite`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeLogPath {
    path: PathBuf,
}

impl ServeLogPath {
    pub fn new(log_dir: impl AsRef<Path>, unix_ms: u64, pid: u32) -> Self {
        let name = format!("serve-{unix_ms}-{pid}.sqlite");
        Self {
            path: log_dir.as_ref().join(name),
        }
    }

    /// Empty / unset override → default name under `log_dir`.
    pub fn resolve(
        log_dir: impl AsRef<Path>,
        unix_ms: u64,
        pid: u32,
        override_path: Option<&str>,
    ) -> Self {
        match override_path {
            Some(p) if !p.is_empty() => Self {
                path: PathBuf::from(p),
            },
            _ => Self::new(log_dir, unix_ms, pid),
        }
    }

    /// `PROGRESSIVE_LSP_LOG` wins over `[log].path`. Tests inject both; no `$HOME`.
    pub fn from_env_or_config(
        log_dir: impl AsRef<Path>,
        unix_ms: u64,
        pid: u32,
        config_path: Option<&str>,
    ) -> Self {
        let env = std::env::var(ENV_LOG_PATH).ok();
        let env = env.as_deref().filter(|s| !s.is_empty());
        let cfg = config_path.filter(|s| !s.is_empty());
        Self::resolve(log_dir, unix_ms, pid, env.or(cfg))
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Same-dir fallback when the primary WAL cannot open. Never `$HOME`.
    pub fn fallback(log_dir: impl AsRef<Path>, unix_ms: u64, pid: u32) -> Self {
        let name = format!("serve-fallback-{unix_ms}-{pid}.sqlite");
        Self {
            path: log_dir.as_ref().join(name),
        }
    }

    /// Temp-dir WAL when primary and same-dir fallback cannot open. Tests inject
    /// `temp_dir`; production passes `std::env::temp_dir()`. Never `$HOME`.
    pub fn in_temp(temp_dir: impl AsRef<Path>, unix_ms: u64, pid: u32) -> Self {
        let name = format!("progressive-lsp-serve-{unix_ms}-{pid}.sqlite");
        Self {
            path: temp_dir.as_ref().join(name),
        }
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
    fn serve_log_path_format_is_value_object() {
        let p = ServeLogPath::new("/pfx/log", 1_700_000_000_000, 4242);
        assert_eq!(
            p.as_path(),
            Path::new("/pfx/log/serve-1700000000000-4242.sqlite")
        );
        let other = ServeLogPath::new("/pfx/log", 99, 1);
        assert_eq!(other.as_path(), Path::new("/pfx/log/serve-99-1.sqlite"));
        assert_ne!(p, other);
        assert_eq!(
            p.clone().into_path(),
            PathBuf::from("/pfx/log/serve-1700000000000-4242.sqlite")
        );
    }

    #[test]
    fn empty_or_unset_override_uses_default_name() {
        let d = ServeLogPath::new("/l", 5, 7);
        assert_eq!(ServeLogPath::resolve("/l", 5, 7, None), d);
        assert_eq!(ServeLogPath::resolve("/l", 5, 7, Some("")), d);
        let over = ServeLogPath::resolve("/l", 5, 7, Some("/abs/custom.sqlite"));
        assert_eq!(over.as_path(), Path::new("/abs/custom.sqlite"));
    }

    #[test]
    fn env_overrides_config_path_value_object() {
        let _g = lock_env();
        let old = std::env::var(ENV_LOG_PATH).ok();
        std::env::remove_var(ENV_LOG_PATH);
        let from_cfg = ServeLogPath::from_env_or_config("/l", 1, 2, Some("/from-cfg.sqlite"));
        assert_eq!(from_cfg.as_path(), Path::new("/from-cfg.sqlite"));
        let unset = ServeLogPath::from_env_or_config("/l", 1, 2, None);
        assert_eq!(unset, ServeLogPath::new("/l", 1, 2));
        std::env::set_var(ENV_LOG_PATH, "");
        let empty_env = ServeLogPath::from_env_or_config("/l", 1, 2, Some("/from-cfg.sqlite"));
        assert_eq!(empty_env.as_path(), Path::new("/from-cfg.sqlite"));
        std::env::set_var(ENV_LOG_PATH, "/from-env.sqlite");
        let env_wins = ServeLogPath::from_env_or_config("/l", 1, 2, Some("/from-cfg.sqlite"));
        assert_eq!(env_wins.as_path(), Path::new("/from-env.sqlite"));
        std::env::set_var(ENV_LOG_PATH, "/from-env.sqlite");
        let env_only = ServeLogPath::from_env_or_config("/l", 1, 2, None);
        assert_eq!(env_only.as_path(), Path::new("/from-env.sqlite"));
        match old {
            Some(v) => std::env::set_var(ENV_LOG_PATH, v),
            None => std::env::remove_var(ENV_LOG_PATH),
        }
    }

    #[test]
    fn fallback_and_in_temp_are_value_objects_never_home() {
        let fb = ServeLogPath::fallback("/injected/log", 1_700_000_000_000, 4242);
        assert_eq!(
            fb.as_path(),
            Path::new("/injected/log/serve-fallback-1700000000000-4242.sqlite")
        );
        let tmp = ServeLogPath::in_temp("/injected/tmp", 1_700_000_000_000, 4242);
        assert_eq!(
            tmp.as_path(),
            Path::new("/injected/tmp/progressive-lsp-serve-1700000000000-4242.sqlite")
        );
        assert_ne!(fb, tmp);
        assert_eq!(
            fb.clone().into_path(),
            PathBuf::from("/injected/log/serve-fallback-1700000000000-4242.sqlite")
        );
        if let Ok(home) = std::env::var("HOME") {
            assert!(
                !fb.as_path().starts_with(&home),
                "fallback must not use $HOME: {}",
                fb.as_path().display()
            );
            assert!(
                !tmp.as_path().starts_with(&home),
                "in_temp must not use $HOME: {}",
                tmp.as_path().display()
            );
        }
    }
}
