//! [`WriterActor`] — Actor. One thread owns the `rusqlite::Connection`.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use progressive_lsp_core::{ClockPort, LogRecord};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::batch::CrashSafeBatch;

/// Inbox cap. Full channel drops oldest [`ActorMsg::Record`].
pub const CHANNEL_CAP: usize = 4096;

pub(crate) enum ActorMsg {
    Record(LogRecord),
    Flush(std::sync::mpsc::Sender<()>),
    Shutdown,
}

struct Mailbox {
    q: Mutex<VecDeque<ActorMsg>>,
    cv: Condvar,
    cap: usize,
    dropped: AtomicU64,
}

impl Mailbox {
    fn new(cap: usize) -> Self {
        Self {
            q: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            cap: cap.max(1),
            dropped: AtomicU64::new(0),
        }
    }

    fn try_push(&self, msg: ActorMsg) {
        let mut q = self.q.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(msg, ActorMsg::Record(_)) {
            let recs = q
                .iter()
                .filter(|m| matches!(m, ActorMsg::Record(_)))
                .count();
            if recs >= self.cap {
                if let Some(i) = q.iter().position(|m| matches!(m, ActorMsg::Record(_))) {
                    q.remove(i);
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        q.push_back(msg);
        self.cv.notify_one();
    }

    fn recv(&self) -> ActorMsg {
        let mut q = self.q.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(m) = q.pop_front() {
                return m;
            }
            q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn recv_timeout(&self, d: Duration) -> Option<ActorMsg> {
        let mut q = self.q.lock().unwrap_or_else(|e| e.into_inner());
        let start = std::time::Instant::now();
        loop {
            if let Some(m) = q.pop_front() {
                return Some(m);
            }
            let left = d.saturating_sub(start.elapsed());
            if left.is_zero() {
                return None;
            }
            let (g, timed) = self
                .cv
                .wait_timeout(q, left)
                .unwrap_or_else(|e| e.into_inner());
            q = g;
            if timed.timed_out() && q.is_empty() {
                return None;
            }
        }
    }

    fn take_dropped(&self) -> u64 {
        self.dropped.swap(0, Ordering::Relaxed)
    }
}

/// Actor. Owns the Connection on its thread (`check_same_thread` stays true).
pub struct WriterActor {
    mailbox: Arc<Mailbox>,
    join: Option<JoinHandle<()>>,
}

impl WriterActor {
    pub fn spawn(
        path: impl AsRef<Path>,
        clock: Arc<dyn ClockPort>,
        batch: CrashSafeBatch,
        commit_faults: Arc<AtomicUsize>,
    ) -> Result<Self, String> {
        Self::spawn_with_cap(path, clock, batch, commit_faults, CHANNEL_CAP)
    }

    pub fn spawn_with_cap(
        path: impl AsRef<Path>,
        clock: Arc<dyn ClockPort>,
        batch: CrashSafeBatch,
        commit_faults: Arc<AtomicUsize>,
        cap: usize,
    ) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let mailbox = Arc::new(Mailbox::new(cap));
        let mb = Arc::clone(&mailbox);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let join = thread::Builder::new()
            .name("plsp-log-writer".into())
            .spawn(move || match open_and_init(&path) {
                Ok(conn) => {
                    let _ = ready_tx.send(Ok(()));
                    run_loop(conn, mb, clock, batch, commit_faults);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        ready_rx.recv().map_err(|e| e.to_string())?.map_err(|e| e)?;
        Ok(Self {
            mailbox,
            join: Some(join),
        })
    }

    pub fn enqueue_record(&self, record: LogRecord) {
        self.mailbox.try_push(ActorMsg::Record(record));
    }

    pub fn flush(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.mailbox.try_push(ActorMsg::Flush(tx));
        let _ = rx.recv();
    }

    pub fn shutdown(&mut self) {
        self.mailbox.try_push(ActorMsg::Shutdown);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WriterActor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn open_and_init(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty()
            && !path
                .to_str()
                .is_some_and(|s| s.starts_with("file:") || s == ":memory:")
        {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    apply_pragmas(&conn)?;
    conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    Ok(conn)
}

fn apply_pragmas(conn: &Connection) -> Result<(), String> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(Duration::from_millis(5000))
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "wal_autocheckpoint", 1000)
        .map_err(|e| e.to_string())?;
    Ok(())
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_unix_ms INTEGER NOT NULL,
  level TEXT NOT NULL,
  component TEXT,
  source_file TEXT,
  source_line INTEGER,
  source_repo TEXT NOT NULL,
  source_crate TEXT,
  content_path TEXT,
  content_file TEXT,
  content_line INTEGER,
  operation TEXT,
  message TEXT NOT NULL,
  extras TEXT
);
CREATE INDEX IF NOT EXISTS idx_log_ts ON log(ts_unix_ms);
CREATE INDEX IF NOT EXISTS idx_log_level ON log(level);
CREATE INDEX IF NOT EXISTS idx_log_component ON log(component);
CREATE INDEX IF NOT EXISTS idx_log_content_path ON log(content_path);
CREATE INDEX IF NOT EXISTS idx_log_source_repo ON log(source_repo);
";

fn run_loop(
    mut conn: Connection,
    mailbox: Arc<Mailbox>,
    clock: Arc<dyn ClockPort>,
    mut batch: CrashSafeBatch,
    commit_faults: Arc<AtomicUsize>,
) {
    loop {
        let timeout = if !batch.is_empty() && batch.batch_ms() > 0 && batch.batch_max() > 1 {
            Some(Duration::from_millis(batch.batch_ms()))
        } else {
            None
        };
        let msg = match timeout {
            Some(d) => match mailbox.recv_timeout(d) {
                Some(m) => m,
                None => {
                    commit(&mut conn, &mut batch, &clock, &commit_faults, &mailbox);
                    continue;
                }
            },
            None => mailbox.recv(),
        };
        match msg {
            ActorMsg::Record(r) => {
                let now = clock.unix_ms();
                if batch.push(r, now) {
                    commit(&mut conn, &mut batch, &clock, &commit_faults, &mailbox);
                }
            }
            ActorMsg::Flush(ack) => {
                commit(&mut conn, &mut batch, &clock, &commit_faults, &mailbox);
                let _ = ack.send(());
            }
            ActorMsg::Shutdown => {
                commit(&mut conn, &mut batch, &clock, &commit_faults, &mailbox);
                break;
            }
        }
    }
}

fn commit(
    conn: &mut Connection,
    batch: &mut CrashSafeBatch,
    clock: &Arc<dyn ClockPort>,
    commit_faults: &AtomicUsize,
    mailbox: &Mailbox,
) {
    batch.note_channel_drops(mailbox.take_dropped());
    if batch.is_empty() {
        if let Some(meta) = batch.take_meta_row(clock.unix_ms()) {
            batch.push(meta, clock.unix_ms());
        }
        if batch.is_empty() {
            return;
        }
    }
    let now = clock.unix_ms();
    let mut rows = batch.take_for_commit();
    if let Some(meta) = batch.take_meta_row(now) {
        rows.push(meta);
    }
    match write_rows(conn, &rows, commit_faults) {
        Ok(()) => batch.on_commit_ok(now),
        Err(_) => batch.on_commit_fail(rows),
    }
}

fn write_rows(
    conn: &mut Connection,
    rows: &[LogRecord],
    commit_faults: &AtomicUsize,
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    for r in rows {
        insert_row(&tx, r).map_err(|e| e.to_string())?;
    }
    if commit_faults
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        tx.rollback().map_err(|e| e.to_string())?;
        return Err("injected COMMIT failure".into());
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_row(tx: &rusqlite::Transaction<'_>, r: &LogRecord) -> rusqlite::Result<()> {
    let extras = r
        .extras
        .as_ref()
        .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".into()));
    tx.execute(
        "INSERT INTO log (
            ts_unix_ms, level, component, source_file, source_line,
            source_repo, source_crate, content_path, content_file, content_line,
            operation, message, extras
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            r.ts_unix_ms as i64,
            r.level.as_str(),
            r.component.as_ref().map(|c| c.as_str().to_string()),
            r.source_file.as_deref(),
            r.source_line.map(|n| n as i64),
            r.source_repo.as_str(),
            r.source_crate.as_deref(),
            r.content_path.as_deref(),
            r.content_file.as_deref(),
            r.content_line.map(|n| n as i64),
            r.operation.as_deref(),
            r.message.as_str(),
            extras,
        ],
    )?;
    Ok(())
}

/// Reader for tests / WAL checks. Opens a second connection (WAL or shared-cache).
pub fn journal_mode(path: impl AsRef<Path>) -> Result<String, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())
}

pub fn compile_options_contain_omit_load_extension() -> Result<bool, String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("PRAGMA compile_options")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for r in rows {
        let opt = r.map_err(|e| e.to_string())?;
        if opt == "OMIT_LOAD_EXTENSION" {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn load_extension_sql_is_omitted() -> Result<bool, String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    match conn.query_row("SELECT load_extension('x')", [], |row| {
        row.get::<_, String>(0)
    }) {
        Err(e) => {
            let s = e.to_string();
            Ok(
                s.contains("no such function")
                    || s.contains("not authorized")
                    || s.contains("load"),
            )
        }
        Ok(_) => Ok(false),
    }
}

pub fn read_messages(path: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT message FROM log ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn read_levels(path: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT level FROM log ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn read_operations(path: impl AsRef<Path>) -> Result<Vec<Option<String>>, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT operation FROM log ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn index_names(path: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

pub fn row_count(path: impl AsRef<Path>) -> Result<i64, String> {
    let conn = Connection::open(path.as_ref()).map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM log", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())
        .map(|n| n.unwrap_or(0))
}

#[cfg(test)]
fn mailbox_drops_oldest_for_test(cap: usize, n: usize) -> u64 {
    let mb = Mailbox::new(cap);
    for i in 0..n {
        mb.try_push(ActorMsg::Record(LogRecord::at_caller(
            progressive_lsp_core::LogLevel::Info,
            format!("m{i}"),
        )));
    }
    mb.take_dropped()
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::LogLevel;

    #[test]
    fn mailbox_drops_oldest_record_when_full_actor() {
        assert_eq!(mailbox_drops_oldest_for_test(2, 2), 0);
        assert_eq!(mailbox_drops_oldest_for_test(2, 3), 1);
        assert_eq!(mailbox_drops_oldest_for_test(1, 4), 3);
        let mb = Mailbox::new(1);
        mb.try_push(ActorMsg::Record(LogRecord::at_caller(LogLevel::Info, "a")));
        let (tx, _rx) = std::sync::mpsc::channel();
        mb.try_push(ActorMsg::Flush(tx));
        mb.try_push(ActorMsg::Record(LogRecord::at_caller(LogLevel::Info, "b")));
        let first = mb.recv();
        assert!(matches!(first, ActorMsg::Flush(_)));
    }

    #[test]
    fn channel_cap_default_is_4096() {
        assert_eq!(CHANNEL_CAP, 4096);
    }

    #[test]
    fn mailbox_recv_timeout_zero_is_none_without_sleep() {
        let mb = Mailbox::new(4);
        assert!(mb.recv_timeout(Duration::ZERO).is_none());
        mb.try_push(ActorMsg::Shutdown);
        assert!(matches!(
            mb.recv_timeout(Duration::from_millis(0)),
            Some(ActorMsg::Shutdown)
        ));
    }

    #[test]
    fn open_and_init_skips_mkdir_for_memory_uri() {
        let conn = open_and_init(Path::new(":memory:")).unwrap();
        drop(conn);
        let conn =
            open_and_init(Path::new("file:plsp-actor-uri?mode=memory&cache=shared")).unwrap();
        drop(conn);
    }

    #[test]
    fn journal_mode_and_readers_on_missing_path_err() {
        assert!(journal_mode(Path::new("/no-such-plsp-log-wal")).is_err());
        assert!(read_messages(Path::new("/no-such-plsp-log-wal")).is_err());
        assert!(read_levels(Path::new("/no-such-plsp-log-wal")).is_err());
        assert!(read_operations(Path::new("/no-such-plsp-log-wal")).is_err());
        assert!(index_names(Path::new("/no-such-plsp-log-wal")).is_err());
        assert!(row_count(Path::new("/no-such-plsp-log-wal")).is_err());
    }
}
