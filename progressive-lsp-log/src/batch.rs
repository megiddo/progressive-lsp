//! [`CrashSafeBatch`] — Unit of Work for WAL commits.

use progressive_lsp_core::{ClockPort, LogLevel, LogOrigin, LogRecord};

/// Default commit size. Tests set `BATCH_MAX = 1` or call `Flush`.
pub const BATCH_MAX: usize = 32;
/// Production idle flush (ms). Tests use [`FakeClock`](progressive_lsp_core::FakeClock) or `Flush`.
pub const BATCH_MS: u64 = 50;
/// COMMIT-failure retry queue cap. Overflow drops oldest.
pub const RETRY_CAP: usize = 1024;

/// Unit of Work. Commit when len, elapsed, Error, Flush, Shutdown, or Drop.
#[derive(Debug)]
pub struct CrashSafeBatch {
    pending: Vec<LogRecord>,
    retry: Vec<LogRecord>,
    last_commit_ms: u64,
    dropped_count: u64,
    batch_max: usize,
    batch_ms: u64,
}

impl CrashSafeBatch {
    pub fn new(now_ms: u64) -> Self {
        Self::with_limits(BATCH_MAX, BATCH_MS, now_ms)
    }

    pub fn with_limits(batch_max: usize, batch_ms: u64, now_ms: u64) -> Self {
        Self {
            pending: Vec::new(),
            retry: Vec::new(),
            last_commit_ms: now_ms,
            dropped_count: 0,
            batch_max: batch_max.max(1),
            batch_ms,
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.retry.is_empty()
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    pub fn batch_max(&self) -> usize {
        self.batch_max
    }

    pub fn batch_ms(&self) -> u64 {
        self.batch_ms
    }

    /// Push `record`. Returns true when a commit should run (including this row).
    pub fn push(&mut self, record: LogRecord, now_ms: u64) -> bool {
        let error = record.level == LogLevel::Error;
        self.pending.push(record);
        error || self.should_commit(now_ms)
    }

    /// Len or elapsed (not Error — that is checked in [`Self::push`]).
    pub fn should_commit(&self, now_ms: u64) -> bool {
        if self.pending.len() >= self.batch_max {
            return true;
        }
        self.elapsed(now_ms)
    }

    fn elapsed(&self, now_ms: u64) -> bool {
        self.batch_ms > 0
            && !self.pending.is_empty()
            && now_ms.saturating_sub(self.last_commit_ms) >= self.batch_ms
    }

    /// Records to write: retry queue first, then pending.
    pub fn take_for_commit(&mut self) -> Vec<LogRecord> {
        let mut out = std::mem::take(&mut self.retry);
        out.append(&mut self.pending);
        out
    }

    pub fn on_commit_ok(&mut self, now_ms: u64) {
        self.last_commit_ms = now_ms;
    }

    /// Keep rows for the next COMMIT. Cap [`RETRY_CAP`]; overflow drops oldest.
    pub fn on_commit_fail(&mut self, records: Vec<LogRecord>) {
        for r in records {
            if self.retry.len() >= RETRY_CAP {
                self.retry.remove(0);
                self.dropped_count = self.dropped_count.saturating_add(1);
            }
            self.retry.push(r);
        }
    }

    /// One `warn` meta row after a successful commit if any records were dropped.
    pub fn take_meta_row(&mut self, now_ms: u64) -> Option<LogRecord> {
        if self.dropped_count == 0 {
            return None;
        }
        let n = self.dropped_count;
        self.dropped_count = 0;
        let mut rec = LogRecord::at_caller(
            LogLevel::Warn,
            format!("dropped {n} log records (channel or commit overflow)"),
        );
        rec.ts_unix_ms = now_ms;
        rec.operation = Some("log".into());
        rec.source_repo = LogOrigin::FirstParty;
        Some(rec.prepared())
    }

    pub fn note_channel_drops(&mut self, n: u64) {
        self.dropped_count = self.dropped_count.saturating_add(n);
    }

    /// Commit helper for tests: inject `write` failure without a live Connection.
    pub fn commit_with<F>(&mut self, now_ms: u64, write: F)
    where
        F: FnOnce(&[LogRecord]) -> Result<(), String>,
    {
        if self.is_empty() {
            return;
        }
        let rows = self.take_for_commit();
        match write(&rows) {
            Ok(()) => {
                self.on_commit_ok(now_ms);
            }
            Err(_) => {
                self.on_commit_fail(rows);
            }
        }
    }

    pub fn now_ms(clock: &dyn ClockPort) -> u64 {
        clock.unix_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::{FakeClock, LogPort};

    fn rec(level: LogLevel, msg: &str) -> LogRecord {
        LogRecord::at_caller(level, msg)
    }

    #[test]
    fn crash_safe_batch_commits_at_max_or_error_unit_of_work() {
        let mut b = CrashSafeBatch::with_limits(2, 0, 100);
        assert_eq!(b.batch_max(), 2);
        assert_eq!(b.batch_ms(), 0);
        assert!(b.is_empty());
        assert!(!b.push(rec(LogLevel::Info, "a"), 100));
        assert_eq!(b.pending_len(), 1);
        assert!(b.push(rec(LogLevel::Info, "b"), 100));
        assert_eq!(b.pending_len(), 2);
        let rows = b.take_for_commit();
        assert_eq!(rows.len(), 2);
        assert!(b.is_empty());
        b.on_commit_ok(100);
        assert!(b.push(rec(LogLevel::Error, "e"), 100));
        assert_eq!(b.pending_len(), 1);
        assert_eq!(b.take_for_commit()[0].level, LogLevel::Error);
    }

    #[test]
    fn crash_safe_batch_elapsed_uses_fake_clock_unit_of_work() {
        let clock = FakeClock::at_unix_ms(1_000);
        let mut b = CrashSafeBatch::with_limits(32, BATCH_MS, CrashSafeBatch::now_ms(&clock));
        assert!(!b.push(rec(LogLevel::Info, "a"), clock.unix_ms()));
        clock.advance_ms(49);
        assert!(!b.should_commit(clock.unix_ms()));
        clock.advance_ms(1);
        assert_eq!(clock.unix_ms(), 1_050);
        assert!(b.should_commit(clock.unix_ms()));
        assert!(b.push(rec(LogLevel::Debug, "b"), clock.unix_ms()));
        let mut zero = CrashSafeBatch::with_limits(32, 0, 1_000);
        zero.push(rec(LogLevel::Info, "z"), 1_000);
        assert!(!zero.should_commit(1_000 + 10_000));
        let mut after = CrashSafeBatch::with_limits(32, 50, 0);
        after.push(rec(LogLevel::Info, "c"), 0);
        after.commit_with(1_000, |_| Ok(()));
        after.push(rec(LogLevel::Info, "d"), 1_049);
        assert!(
            !after.should_commit(1_049),
            "on_commit_ok must move last_commit_ms"
        );
        assert!(after.should_commit(1_050));
        let defaults = CrashSafeBatch::new(0);
        assert_eq!(defaults.batch_max(), BATCH_MAX);
        assert_eq!(defaults.batch_ms(), BATCH_MS);
    }

    #[test]
    fn injected_commit_failure_retries_without_panic_unit_of_work() {
        let mut b = CrashSafeBatch::with_limits(1, 0, 0);
        b.push(rec(LogLevel::Info, "keep"), 0);
        b.commit_with(0, |_| Err("injected COMMIT failure".into()));
        assert_eq!(b.dropped_count(), 0);
        assert!(!b.is_empty());
        let mut ok = 0;
        b.commit_with(1, |rows| {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].message, "keep");
            ok += 1;
            Ok(())
        });
        assert_eq!(ok, 1);
        assert!(b.is_empty());
        b.commit_with(1, |_| panic!("empty batch must not write"));
    }

    #[test]
    fn commit_overflow_drops_oldest_and_emits_log_meta_unit_of_work() {
        let mut b = CrashSafeBatch::with_limits(1, 0, 0);
        let mut rows = Vec::new();
        for i in 0..=RETRY_CAP {
            rows.push(rec(LogLevel::Info, &format!("r{i}")));
        }
        b.on_commit_fail(rows);
        assert_eq!(b.dropped_count(), 1);
        assert_eq!(b.take_for_commit().len(), RETRY_CAP);
        b.on_commit_fail(vec![rec(LogLevel::Info, "again")]);
        assert_eq!(b.dropped_count(), 1);
        b.note_channel_drops(3);
        assert_eq!(b.dropped_count(), 4);
        let meta = b.take_meta_row(9).expect("meta");
        assert_eq!(meta.level, LogLevel::Warn);
        assert_eq!(meta.operation.as_deref(), Some("log"));
        assert_eq!(meta.ts_unix_ms, 9);
        assert!(meta.message.contains("dropped 4"));
        assert!(b.take_meta_row(9).is_none());
        assert_eq!(b.dropped_count(), 0);
        let log = progressive_lsp_core::FakeLog::new();
        log.warn("not the writer");
        assert_eq!(log.records().len(), 1);
    }

    #[test]
    fn zero_batch_max_clamps_to_one_unit_of_work() {
        let mut b = CrashSafeBatch::with_limits(0, 0, 0);
        assert_eq!(b.batch_max(), 1);
        assert!(b.push(rec(LogLevel::Info, "x"), 0));
    }
}
