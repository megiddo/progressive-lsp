//! WAL sqlite log crate plus capture Adapters. Core stays sqlite-free.
//! Serve/install sqlite bootstrap is LOG-4 — this crate is not wired as the
//! durable sink from `run` yet. MemoryLog bootstrap is OK on LOG-3.

pub mod actor;
pub mod batch;
pub mod bridges;
pub mod child_stderr;
pub mod cli_usage;
pub mod config_warn;
pub mod log_file_tail;
pub mod lsp_message;
pub mod path;
pub mod reentrancy;
pub mod repository;
pub mod stderr_emit;
pub mod stdio_adapters;

pub use actor::{WriterActor, CHANNEL_CAP};
pub use batch::{CrashSafeBatch, BATCH_MAX, BATCH_MS, RETRY_CAP};
pub use bridges::{LogCrateBridge, TracingBridge};
pub use child_stderr::{ChildStderrAdapter, FakeChildStderr, STDERR_DRAIN_CAP};
pub use cli_usage::CliUsageAdapter;
pub use config_warn::ConfigWarnAdapter;
pub use log_file_tail::LogFileTailAdapter;
pub use lsp_message::LspLogMessageAdapter;
pub use path::{ServeLogPath, ENV_LOG_PATH};
pub use reentrancy::ReentrancyGuard;
pub use repository::SqliteLogRepository;
pub use stderr_emit::StderrEmitAdapter;
pub use stdio_adapters::{InheritStderrAdapter, NullStderrAdapter};

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FakeClock, LogPort};
    use std::sync::Arc;

    #[test]
    fn public_reexports_resolve() {
        assert_eq!(BATCH_MAX, 32);
        assert_eq!(BATCH_MS, 50);
        assert_eq!(RETRY_CAP, 1024);
        assert_eq!(CHANNEL_CAP, 4096);
        assert_eq!(ENV_LOG_PATH, "PROGRESSIVE_LSP_LOG");
        assert_eq!(STDERR_DRAIN_CAP, 1024);
        let _ = ServeLogPath::new("/l", 1, 2);
        let _ = CrashSafeBatch::new(0);
        let _ = ReentrancyGuard::enter();
        let clock = Arc::new(FakeClock::at_unix_ms(1));
        let repo = SqliteLogRepository::open_memory_with_batch(clock, 1, 0).unwrap();
        repo.info("reexport");
        repo.flush();
        assert_eq!(
            crate::actor::read_messages(repo.path()).unwrap(),
            vec!["reexport".to_string()]
        );
        let _ = NullStderrAdapter::forbidden_on_prod_spawn();
        let _ = InheritStderrAdapter::allowed_on_serve();
    }
}
