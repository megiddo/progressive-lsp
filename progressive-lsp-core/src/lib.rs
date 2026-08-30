//! Ids, typed errors, `ClockPort`, `LogPort`, prefix layout, and the config.toml stub.

pub mod clock;
pub mod config;
pub mod error;
pub mod git_exclude;
pub mod ids;
pub mod log;
pub mod prefix;
pub mod rss;

pub use clock::{ClockPort, FakeClock, SystemClock};
pub use config::{Config, ConfigLoad, ConfigOverlay, T2Backend, T2Table};
pub use error::{
    ConfigError, EngineError, EngineNotReady, InitializeFailed, InstallError, ScriptAbort,
    ScriptSandbox, StaticLinkError, UnsupportedLanguage, WatchOverflow,
};
pub use git_exclude::{
    apply_worktree_excludes, belt_gitignore_body, git_exclude_lines, OVERLAY_DIR_NAME,
};
pub use ids::{FileId, LanguageId, LanguageVersion, PackageId, Tier, WorkspaceId};
pub use log::{
    sanitize_extras, FakeLog, LogComponent, LogLevel, LogOrigin, LogPort, LogRecord, LogScope,
    LogScopeGuard, LogSink, MemoryLog, NeverFailLog, NullLog, MEMORY_LOG_CAP, MESSAGE_MAX_BYTES,
};
pub use prefix::{PrefixLayout, PREFIX_DIR_NAME};
pub use rss::{
    parse_proc_status_vmrss, parse_ps_rss_kb, rss_from_ps_output, rss_sample_label,
    sample_rss_bytes,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_resolve() {
        let _ = LanguageId::new("java");
        let _ = FakeClock::at_unix_ms(1);
        let _ = Config::empty();
        let _ = NullLog;
        let log = FakeLog::new();
        log.info("reexport");
        assert_eq!(log.records().len(), 1);
        assert_eq!(MEMORY_LOG_CAP, 4096);
        assert_eq!(MESSAGE_MAX_BYTES, 64 * 1024);
        let _ = PrefixLayout::from_path("/tmp/prefix");
        assert_eq!(PREFIX_DIR_NAME, OVERLAY_DIR_NAME);
        assert!(rss_sample_label().contains("allocator"));
        assert_eq!(parse_ps_rss_kb("1"), Some(1024));
    }
}
