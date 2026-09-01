//! [`LogOpenPlan`] — Command. Ordered WAL open: primary → fallback → temp.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use progressive_lsp_core::{ClockPort, LogComponent, LogPort, LogScope, MemoryLog, NeverFailLog};

use crate::path::ServeLogPath;
use crate::repository::SqliteLogRepository;
use crate::{BATCH_MAX, BATCH_MS};

/// Command. First successful WAL wins; replay [`MemoryLog`]; `emit` still `()`.
pub struct LogOpenPlan {
    primary: ServeLogPath,
    fallback: ServeLogPath,
    temp: ServeLogPath,
    clock: Arc<dyn ClockPort>,
    batch_max: usize,
    batch_ms: u64,
}

impl LogOpenPlan {
    /// Tests inject both directories. Production passes `std::env::temp_dir()`.
    /// Never `$HOME`.
    pub fn new(
        log_dir: impl AsRef<Path>,
        temp_dir: impl AsRef<Path>,
        unix_ms: u64,
        pid: u32,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            primary: ServeLogPath::new(&log_dir, unix_ms, pid),
            fallback: ServeLogPath::fallback(&log_dir, unix_ms, pid),
            temp: ServeLogPath::in_temp(temp_dir, unix_ms, pid),
            clock,
            batch_max: BATCH_MAX,
            batch_ms: BATCH_MS,
        }
    }

    /// Env / `[log].path` override for the primary file only.
    pub fn with_primary(mut self, primary: ServeLogPath) -> Self {
        self.primary = primary;
        self
    }

    /// Tests set `BATCH_MAX = 1` so join is immediate. No `thread::sleep`.
    pub fn with_batch(mut self, batch_max: usize, batch_ms: u64) -> Self {
        self.batch_max = batch_max;
        self.batch_ms = batch_ms;
        self
    }

    /// Execute the Command. Replay the ring into the WAL that opened. Warn when
    /// a later path won; `error` `operation=log` only if all three fail.
    pub fn execute(
        self,
        mem: MemoryLog,
    ) -> (
        Arc<dyn LogPort>,
        Option<Arc<NeverFailLog<SqliteLogRepository>>>,
    ) {
        let candidates: [(&ServeLogPath, &'static str); 3] = [
            (&self.primary, "primary"),
            (&self.fallback, "fallback"),
            (&self.temp, "temp"),
        ];
        let mut failures: Vec<(PathBuf, String)> = Vec::new();
        for (named, label) in candidates {
            match SqliteLogRepository::open_with_batch(
                named.as_path(),
                Arc::clone(&self.clock),
                self.batch_max,
                self.batch_ms,
            ) {
                Ok(repo) => {
                    return finish_opened(repo, mem, label, &failures);
                }
                Err(e) => failures.push((named.as_path().to_path_buf(), e)),
            }
        }
        keep_memory(mem, &failures)
    }
}

fn finish_opened(
    repo: SqliteLogRepository,
    mem: MemoryLog,
    label: &str,
    failures: &[(PathBuf, String)],
) -> (
    Arc<dyn LogPort>,
    Option<Arc<NeverFailLog<SqliteLogRepository>>>,
) {
    for rec in mem.drain() {
        repo.emit(rec);
    }
    if !failures.is_empty() {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("log")
                .component(LogComponent::core()),
        );
        let why = format_failures(failures);
        let opened = repo.path().display();
        repo.warn(&format!(
            "opened {label} WAL {opened} after previous failed ({why})"
        ));
    }
    repo.flush();
    let durable = Arc::new(NeverFailLog::new(repo));
    let log: Arc<dyn LogPort> = Arc::clone(&durable) as Arc<dyn LogPort>;
    (log, Some(durable))
}

fn keep_memory(
    mem: MemoryLog,
    failures: &[(PathBuf, String)],
) -> (
    Arc<dyn LogPort>,
    Option<Arc<NeverFailLog<SqliteLogRepository>>>,
) {
    let _g = LogScope::enter(
        LogScope::new()
            .operation("log")
            .component(LogComponent::core()),
    );
    let why = format_failures(failures);
    mem.error(&format!("all WAL opens failed; keeping MemoryLog ({why})"));
    (Arc::new(mem), None)
}

fn format_failures(failures: &[(PathBuf, String)]) -> String {
    failures
        .iter()
        .map(|(p, e)| format!("{}: {e}", p.display()))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{read_levels, read_messages, read_operations};
    use progressive_lsp_core::{FakeClock, LogLevel};
    use std::env;

    fn clock(ms: u64) -> Arc<FakeClock> {
        Arc::new(FakeClock::at_unix_ms(ms))
    }

    fn block_as_dir(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
    }

    fn block_parent_as_file(path: &Path) {
        let parent = path.parent().expect("parent");
        if let Some(grand) = parent.parent() {
            std::fs::create_dir_all(grand).unwrap();
        }
        if parent.is_dir() {
            std::fs::remove_dir_all(parent).unwrap();
        }
        std::fs::write(parent, b"x").unwrap();
    }

    #[test]
    fn log_open_plan_is_command_primary_then_fallback_then_temp() {
        let log_dir = tempfile::tempdir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let plan = LogOpenPlan::new(log_dir.path(), temp_dir.path(), 11, 22, clock(11));
        assert_eq!(
            plan.primary.as_path(),
            &log_dir.path().join("serve-11-22.sqlite")
        );
        assert_eq!(
            plan.fallback.as_path(),
            &log_dir.path().join("serve-fallback-11-22.sqlite")
        );
        assert_eq!(
            plan.temp.as_path(),
            &temp_dir.path().join("progressive-lsp-serve-11-22.sqlite")
        );
        if let Ok(home) = env::var("HOME") {
            assert!(!plan.primary.as_path().starts_with(&home));
            assert!(!plan.fallback.as_path().starts_with(&home));
            assert!(!plan.temp.as_path().starts_with(&home));
        }
    }

    #[test]
    fn primary_success_replays_ring_without_fallback_or_temp() {
        let log_dir = tempfile::tempdir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let mem = MemoryLog::new();
        mem.info("bootstrap-ring");
        let plan =
            LogOpenPlan::new(log_dir.path(), temp_dir.path(), 99, 7, clock(99)).with_batch(1, 0);
        let (log, durable) = plan.execute(mem);
        let durable = durable.expect("primary WAL");
        let path = durable.inner().path().to_path_buf();
        assert_eq!(path, log_dir.path().join("serve-99-7.sqlite"));
        assert!(!log_dir.path().join("serve-fallback-99-7.sqlite").exists());
        assert!(!temp_dir
            .path()
            .join("progressive-lsp-serve-99-7.sqlite")
            .exists());
        durable.inner().flush();
        let msgs = read_messages(&path).unwrap();
        assert!(
            msgs.iter().any(|m| m.contains("bootstrap-ring")),
            "{msgs:?}"
        );
        assert!(
            !msgs.iter().any(|m| m.contains("after previous failed")),
            "{msgs:?}"
        );
        drop(log);
        drop(durable);
    }

    #[test]
    fn with_primary_override_opens_that_path() {
        let log_dir = tempfile::tempdir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let custom = log_dir.path().join("custom-primary.sqlite");
        let mem = MemoryLog::new();
        mem.info("override-ring");
        let plan = LogOpenPlan::new(log_dir.path(), temp_dir.path(), 1, 2, clock(1))
            .with_primary(ServeLogPath::resolve(
                log_dir.path(),
                1,
                2,
                Some(custom.to_str().unwrap()),
            ))
            .with_batch(1, 0);
        let (_log, durable) = plan.execute(mem);
        let durable = durable.expect("custom primary");
        assert_eq!(durable.inner().path(), custom.as_path());
        durable.inner().flush();
        assert!(read_messages(&custom)
            .unwrap()
            .iter()
            .any(|m| m.contains("override-ring")));
        assert!(!log_dir.path().join("serve-1-2.sqlite").exists());
    }

    #[test]
    fn primary_dir_as_file_opens_fallback_with_warn_and_replay() {
        let log_dir = tempfile::tempdir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let unix_ms = 1_700_000_000_000u64;
        let pid = 4242u32;
        let primary = ServeLogPath::new(log_dir.path(), unix_ms, pid);
        block_as_dir(primary.as_path());
        let mem = MemoryLog::new();
        mem.info("bootstrap-ring");
        let plan = LogOpenPlan::new(
            log_dir.path(),
            temp_dir.path(),
            unix_ms,
            pid,
            clock(unix_ms),
        )
        .with_batch(1, 0);
        let (log, durable) = plan.execute(mem);
        let durable = durable.expect("fallback WAL");
        let path = durable.inner().path().to_path_buf();
        assert_eq!(
            path,
            log_dir
                .path()
                .join("serve-fallback-1700000000000-4242.sqlite")
        );
        assert!(path.is_file(), "{}", path.display());
        assert!(!temp_dir
            .path()
            .join("progressive-lsp-serve-1700000000000-4242.sqlite")
            .exists());
        durable.inner().flush();
        let msgs = read_messages(&path).unwrap();
        let levels = read_levels(&path).unwrap();
        let ops = read_operations(&path).unwrap();
        assert!(
            msgs.iter().any(|m| m.contains("bootstrap-ring")),
            "replayed ring: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("opened fallback WAL")
                && m.contains("serve-fallback-1700000000000-4242.sqlite")
                && m.contains("after previous failed")),
            "{msgs:?}"
        );
        assert!(levels.iter().any(|l| l == "warn"), "{levels:?}");
        assert!(ops.iter().any(|o| o.as_deref() == Some("log")), "{ops:?}");
        if let Ok(home) = env::var("HOME") {
            assert!(!path.starts_with(&home), "{}", path.display());
        }
        drop(log);
        drop(durable);
    }

    #[test]
    fn fallback_parent_as_file_opens_temp_wal_with_warn() {
        let root = tempfile::tempdir().unwrap();
        let log_as_file = root.path().join("log-as-file");
        std::fs::write(&log_as_file, b"x").unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let unix_ms = 50u64;
        let pid = 9u32;
        let mem = MemoryLog::new();
        mem.info("temp-ring");
        let plan = LogOpenPlan::new(&log_as_file, temp_dir.path(), unix_ms, pid, clock(50))
            .with_batch(1, 0);
        let (log, durable) = plan.execute(mem);
        let durable = durable.expect("temp WAL");
        let path = durable.inner().path().to_path_buf();
        assert_eq!(
            path,
            temp_dir.path().join("progressive-lsp-serve-50-9.sqlite")
        );
        assert!(path.is_file(), "{}", path.display());
        durable.inner().flush();
        let msgs = read_messages(&path).unwrap();
        assert!(msgs.iter().any(|m| m.contains("temp-ring")), "{msgs:?}");
        assert!(
            msgs.iter().any(|m| m.contains("opened temp WAL")
                && m.contains("progressive-lsp-serve-50-9.sqlite")
                && m.contains("after previous failed")),
            "{msgs:?}"
        );
        assert!(read_levels(&path).unwrap().iter().any(|l| l == "warn"));
        assert!(read_operations(&path)
            .unwrap()
            .iter()
            .any(|o| o.as_deref() == Some("log")));
        drop(log);
        drop(durable);
    }

    #[test]
    fn all_three_fail_keeps_memory_log_and_emits_error() {
        let root = tempfile::tempdir().unwrap();
        let log_as_file = root.path().join("log-as-file");
        std::fs::write(&log_as_file, b"x").unwrap();
        let temp_as_file = root.path().join("temp-as-file");
        std::fs::write(&temp_as_file, b"x").unwrap();
        let mem = MemoryLog::new();
        mem.info("residual-ring");
        let watch = mem.clone();
        let plan = LogOpenPlan::new(&log_as_file, &temp_as_file, 3, 4, clock(3)).with_batch(1, 0);
        let (log, durable) = plan.execute(mem);
        assert!(durable.is_none(), "MemoryLog residual must not open a WAL");
        let snap = watch.snapshot();
        assert!(
            snap.iter().any(|r| r.message.contains("residual-ring")),
            "{snap:?}"
        );
        assert!(
            snap.iter().any(|r| r.level == LogLevel::Error
                && r.operation.as_deref() == Some("log")
                && r.component.as_ref().map(|c| c.as_str()) == Some("core")
                && r.message.contains("all WAL opens failed")
                && r.message.contains("keeping MemoryLog")),
            "{snap:?}"
        );
        let _: () = log.warn("still-emits");
        assert!(watch
            .snapshot()
            .iter()
            .any(|r| r.message.contains("still-emits")));
    }

    #[test]
    fn with_primary_blocked_falls_back_in_log_dir() {
        let log_dir = tempfile::tempdir().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let blocked = log_dir.path().join("blocked-primary");
        block_as_dir(&blocked);
        let mem = MemoryLog::new();
        mem.info("env-ring");
        let plan = LogOpenPlan::new(log_dir.path(), temp_dir.path(), 8, 1, clock(8))
            .with_primary(ServeLogPath::resolve(
                log_dir.path(),
                8,
                1,
                Some(blocked.to_str().unwrap()),
            ))
            .with_batch(1, 0);
        let (_log, durable) = plan.execute(mem);
        let durable = durable.expect("fallback");
        let path = durable.inner().path().to_path_buf();
        assert_eq!(path, log_dir.path().join("serve-fallback-8-1.sqlite"));
        durable.inner().flush();
        let msgs = read_messages(&path).unwrap();
        assert!(msgs.iter().any(|m| m.contains("env-ring")), "{msgs:?}");
        assert!(
            msgs.iter().any(|m| m.contains("opened fallback WAL")),
            "{msgs:?}"
        );
    }

    #[test]
    fn format_failures_names_path_and_reason() {
        let s = format_failures(&[(PathBuf::from("/p.sqlite"), "io: denied".into())]);
        assert!(s.contains("/p.sqlite"));
        assert!(s.contains("io: denied"));
        let two = format_failures(&[
            (PathBuf::from("/a"), "one".into()),
            (PathBuf::from("/b"), "two".into()),
        ]);
        assert!(two.contains("/a: one; /b: two"), "{two}");
    }

    #[test]
    fn block_parent_as_file_is_used_for_open_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parent").join("x.sqlite");
        block_parent_as_file(&path);
        assert!(path.parent().unwrap().is_file());
        assert!(SqliteLogRepository::open(&path, clock(1)).is_err());
    }
}
