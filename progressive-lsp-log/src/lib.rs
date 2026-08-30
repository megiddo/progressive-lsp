//! WAL sqlite log crate. Core stays sqlite-free. The bin is not wired (LOG-4).
//! No capture bridges (LOG-3).

pub mod actor;
pub mod batch;
pub mod path;
pub mod reentrancy;
pub mod repository;

pub use actor::{WriterActor, CHANNEL_CAP};
pub use batch::{CrashSafeBatch, BATCH_MAX, BATCH_MS, RETRY_CAP};
pub use path::{ServeLogPath, ENV_LOG_PATH};
pub use reentrancy::ReentrancyGuard;
pub use repository::SqliteLogRepository;

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
    }
}
