//! [`SqliteLogRepository`] — Adapter / Repository. One WAL file per process.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use progressive_lsp_core::{ClockPort, LogPort, LogRecord, LogSink};

use crate::actor::WriterActor;
use crate::batch::CrashSafeBatch;
use crate::reentrancy::ReentrancyGuard;
use crate::{BATCH_MAX, BATCH_MS};

static MEMORY_SEQ: AtomicU64 = AtomicU64::new(1);

/// Adapter / Repository. `emit` never returns `Result`. Drop sends Shutdown and joins.
pub struct SqliteLogRepository {
    actor: WriterActor,
    clock: Arc<dyn ClockPort>,
    path: PathBuf,
    commit_faults: Arc<AtomicUsize>,
}

impl SqliteLogRepository {
    pub fn open(path: impl AsRef<Path>, clock: Arc<dyn ClockPort>) -> Result<Self, String> {
        Self::open_with_batch(path, clock, BATCH_MAX, BATCH_MS)
    }

    pub fn open_memory(clock: Arc<dyn ClockPort>) -> Result<Self, String> {
        Self::open_memory_with_batch(clock, BATCH_MAX, BATCH_MS)
    }

    pub fn open_memory_with_batch(
        clock: Arc<dyn ClockPort>,
        batch_max: usize,
        batch_ms: u64,
    ) -> Result<Self, String> {
        let n = MEMORY_SEQ.fetch_add(1, Ordering::Relaxed);
        let uri = format!("file:plsp-log-{n}?mode=memory&cache=shared");
        Self::open_with_batch(uri, clock, batch_max, batch_ms)
    }

    pub fn open_with_batch(
        path: impl AsRef<Path>,
        clock: Arc<dyn ClockPort>,
        batch_max: usize,
        batch_ms: u64,
    ) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let commit_faults = Arc::new(AtomicUsize::new(0));
        let batch = CrashSafeBatch::with_limits(batch_max, batch_ms, clock.unix_ms());
        let actor =
            WriterActor::spawn(&path, Arc::clone(&clock), batch, Arc::clone(&commit_faults))?;
        Ok(Self {
            actor,
            clock,
            path,
            commit_faults,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn flush(&self) {
        self.actor.flush();
    }

    /// Test injection: next `n` COMMIT attempts fail, then retry succeeds.
    pub fn inject_commit_failures(&self, n: usize) {
        self.commit_faults.store(n, Ordering::SeqCst);
    }
}

impl LogPort for SqliteLogRepository {
    fn emit(&self, record: LogRecord) {
        let _g = ReentrancyGuard::enter();
        let mut rec = record.prepared();
        if rec.ts_unix_ms == 0 {
            rec.ts_unix_ms = self.clock.unix_ms();
        }
        self.actor.enqueue_record(rec);
    }
}

impl LogSink for SqliteLogRepository {
    fn append(&self, record: LogRecord) -> Result<(), String> {
        self.emit(record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{
        compile_options_contain_omit_load_extension, index_names, journal_mode,
        load_extension_sql_is_omitted, read_levels, read_messages, read_operations, row_count,
    };
    use crate::ServeLogPath;
    use progressive_lsp_core::{
        FakeClock, FakeLog, LogComponent, LogLevel, LogScope, NeverFailLog, PrefixLayout,
    };
    use std::collections::BTreeMap;

    fn clock(ms: u64) -> Arc<FakeClock> {
        Arc::new(FakeClock::at_unix_ms(ms))
    }

    fn tempfile_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = ServeLogPath::new(dir.path(), 11, 22).into_path();
        (dir, path)
    }

    #[test]
    fn emit_returns_unit_and_flush_writes_adapter() {
        let (_dir, path) = tempfile_path();
        let c = clock(1_000);
        let repo = SqliteLogRepository::open_with_batch(&path, c, 1, 0).unwrap();
        let _: () = repo.emit(LogRecord::at_caller(LogLevel::Info, "hello"));
        repo.flush();
        assert_eq!(read_messages(&path).unwrap(), vec!["hello".to_string()]);
        assert_eq!(read_levels(&path).unwrap(), vec!["info".to_string()]);
        assert!(LogSink::append(&repo, LogRecord::at_caller(LogLevel::Warn, "s")).is_ok());
        repo.flush();
        assert_eq!(read_messages(&path).unwrap().len(), 2);
    }

    #[test]
    fn tempfile_wal_pragma_returns_wal_repository() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open(&path, clock(5)).unwrap();
        repo.info("wal");
        repo.flush();
        assert_eq!(journal_mode(&path).unwrap().to_lowercase(), "wal");
        let names = index_names(&path).unwrap();
        assert!(names.iter().any(|n| n == "idx_log_ts"), "{names:?}");
        assert!(names.iter().any(|n| n == "idx_log_level"), "{names:?}");
        assert!(names.iter().any(|n| n == "idx_log_component"), "{names:?}");
        assert!(
            names.iter().any(|n| n == "idx_log_content_path"),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n == "idx_log_source_repo"),
            "{names:?}"
        );
    }

    #[test]
    fn drop_flushes_without_sleep_repository() {
        let (_dir, path) = tempfile_path();
        {
            let repo = SqliteLogRepository::open_with_batch(&path, clock(3), 32, 0).unwrap();
            repo.warn("dropped");
        }
        assert_eq!(read_messages(&path).unwrap(), vec!["dropped".to_string()]);
    }

    #[test]
    fn batch_max_one_commits_each_record_unit_of_work() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        repo.info("one");
        repo.info("two");
        repo.flush();
        assert_eq!(
            read_messages(&path).unwrap(),
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn fake_clock_elapsed_commits_batch_unit_of_work() {
        let (_dir, path) = tempfile_path();
        let c = clock(1_000);
        let repo = SqliteLogRepository::open_with_batch(
            &path,
            Arc::clone(&c) as Arc<dyn ClockPort>,
            32,
            50,
        )
        .unwrap();
        repo.info("first");
        c.advance_ms(50);
        repo.info("second");
        repo.flush();
        assert_eq!(
            read_messages(&path).unwrap(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn error_commits_immediately_including_that_record() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 32, 0).unwrap();
        repo.info("hold");
        repo.error("boom");
        repo.flush();
        assert_eq!(
            read_messages(&path).unwrap(),
            vec!["hold".to_string(), "boom".to_string()]
        );
        assert_eq!(
            read_levels(&path).unwrap(),
            vec!["info".to_string(), "error".to_string()]
        );
    }

    #[test]
    fn injected_commit_failure_retries_without_panic_actor() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 32, 0).unwrap();
        repo.inject_commit_failures(1);
        repo.info("retry-me");
        repo.flush();
        assert_eq!(
            row_count(&path).unwrap(),
            0,
            "injected COMMIT failure must not persist the row"
        );
        repo.flush();
        assert_eq!(read_messages(&path).unwrap(), vec!["retry-me".to_string()]);
    }

    #[test]
    fn nested_emit_does_not_deadlock_reentrancy_guard() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        repo.emit(LogRecord::at_caller(LogLevel::Info, "outer"));
        {
            let _g = ReentrancyGuard::enter();
            assert!(ReentrancyGuard::in_emit());
            repo.emit(LogRecord::at_caller(LogLevel::Info, "nested"));
        }
        repo.flush();
        let msgs = read_messages(&path).unwrap();
        assert!(msgs.contains(&"outer".to_string()), "{msgs:?}");
        assert!(msgs.contains(&"nested".to_string()), "{msgs:?}");
        assert_ne!(
            std::thread::current().name(),
            Some("plsp-log-writer"),
            "caller is not the writer"
        );
    }

    #[test]
    fn writer_thread_never_calls_log_port_actor() {
        let fake = FakeLog::new();
        fake.info("outside");
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        repo.info("sqlite");
        repo.flush();
        assert_eq!(fake.records().len(), 1);
        assert_eq!(read_messages(&path).unwrap(), vec!["sqlite".to_string()]);
    }

    #[test]
    fn shared_cache_memory_uri_is_readable() {
        let repo = SqliteLogRepository::open_memory_with_batch(clock(8), 1, 0).unwrap();
        repo.info("mem");
        repo.flush();
        assert_eq!(read_messages(repo.path()).unwrap(), vec!["mem".to_string()]);
        assert!(row_count(repo.path()).unwrap() >= 1);
    }

    #[test]
    fn serve_log_path_default_under_prefix_log_dir() {
        let dir = tempfile::tempdir().unwrap();
        let layout = PrefixLayout::from_path(dir.path());
        layout.ensure_dirs().unwrap();
        let named = ServeLogPath::new(layout.log_dir(), 42, 9);
        let repo = SqliteLogRepository::open_with_batch(named.as_path(), clock(42), 1, 0).unwrap();
        repo.info("named");
        repo.flush();
        assert!(named.as_path().ends_with("serve-42-9.sqlite"));
        assert_eq!(read_messages(named.as_path()).unwrap().len(), 1);
    }

    #[test]
    fn emit_stamps_clock_and_scope_and_sanitizes() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(77), 1, 0).unwrap();
        let _g = LogScope::new()
            .path("/ws/A.java")
            .operation("index")
            .component(LogComponent::index())
            .enter();
        let mut rec = LogRecord::at_caller(LogLevel::Info, "scoped");
        rec.extras = Some(BTreeMap::from([
            ("token".into(), "leak".into()),
            ("ok".into(), "1".into()),
        ]));
        repo.emit(rec);
        repo.flush();
        let conn = rusqlite::Connection::open(&path).unwrap();
        let (ts, op, extras, file): (i64, String, Option<String>, String) = conn
            .query_row(
                "SELECT ts_unix_ms, operation, extras, content_file FROM log",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(ts, 77);
        assert_eq!(op, "index");
        assert_eq!(file, "A.java");
        let extras = extras.unwrap();
        assert!(extras.contains("ok"));
        assert!(!extras.contains("token"));
    }

    #[test]
    fn never_fail_log_wraps_sqlite_sink_decorator() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        let wrapped = NeverFailLog::new(repo);
        wrapped.error("via-decorator");
        wrapped.inner().flush();
        assert_eq!(
            read_messages(wrapped.inner().path()).unwrap(),
            vec!["via-decorator".to_string()]
        );
    }

    #[test]
    fn omit_load_extension_is_compiled_in() {
        assert!(
            compile_options_contain_omit_load_extension().unwrap(),
            "SQLITE_OMIT_LOAD_EXTENSION must be set (LIBSQLITE3_FLAGS)"
        );
        assert!(load_extension_sql_is_omitted().unwrap());
    }

    #[test]
    fn convenience_methods_and_operations() {
        let (_dir, path) = tempfile_path();
        let repo = SqliteLogRepository::open_memory_with_batch(clock(1), 1, 0);
        assert!(repo.is_ok());
        drop(repo);
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        repo.debug("d");
        repo.trace("t");
        repo.flush();
        let ops = read_operations(&path).unwrap();
        assert!(ops.iter().all(|o| o.is_none()));
        assert_eq!(read_messages(&path).unwrap().len(), 2);
    }

    #[test]
    fn open_missing_parent_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("log.sqlite");
        let repo = SqliteLogRepository::open_with_batch(&path, clock(1), 1, 0).unwrap();
        repo.info("nested");
        repo.flush();
        assert_eq!(row_count(&path).unwrap(), 1);
        let mem = SqliteLogRepository::open_memory(clock(2)).unwrap();
        mem.info("default-batch");
        mem.flush();
        assert!(row_count(mem.path()).unwrap() >= 1);
    }

    #[test]
    fn open_fails_when_parent_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let path = file.join("log.sqlite");
        assert!(SqliteLogRepository::open(&path, clock(1)).is_err());
    }

    #[test]
    fn writer_actor_spawn_is_actor() {
        use crate::actor::WriterActor;
        use crate::CrashSafeBatch;
        use std::sync::atomic::AtomicUsize;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("actor.sqlite");
        let actor = WriterActor::spawn(
            &path,
            clock(1),
            CrashSafeBatch::with_limits(32, 0, 1),
            Arc::new(AtomicUsize::new(0)),
        )
        .unwrap();
        actor.enqueue_record(LogRecord::at_caller(LogLevel::Info, "via-actor"));
        drop(actor);
        assert_eq!(
            crate::actor::read_messages(&path).unwrap(),
            vec!["via-actor".to_string()]
        );
    }
}
