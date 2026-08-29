//! Ids, typed errors, `ClockPort`, prefix layout, and the config.toml stub.

pub mod clock;
pub mod config;
pub mod error;
pub mod git_exclude;
pub mod ids;
pub mod prefix;

pub use clock::{ClockPort, FakeClock, SystemClock};
pub use config::{Config, ConfigLoad, ConfigOverlay};
pub use error::{
    ConfigError, EngineError, EngineNotReady, InitializeFailed, InstallError, ScriptAbort,
    ScriptSandbox, StaticLinkError, UnsupportedLanguage, WatchOverflow,
};
pub use git_exclude::{
    apply_worktree_excludes, belt_gitignore_body, git_exclude_lines, OVERLAY_DIR_NAME,
};
pub use ids::{FileId, LanguageId, LanguageVersion, PackageId, Tier, WorkspaceId};
pub use prefix::{PrefixLayout, PREFIX_DIR_NAME};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_resolve() {
        let _ = LanguageId::new("java");
        let _ = FakeClock::at_unix_ms(1);
        let _ = Config::empty();
        let _ = PrefixLayout::from_path("/tmp/prefix");
        assert_eq!(PREFIX_DIR_NAME, OVERLAY_DIR_NAME);
    }
}
