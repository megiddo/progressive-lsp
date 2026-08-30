//! Per-run sqlite debug log. [`RunLog`] is a Repository (same family as
//! `FilesSinceJournal`): production talks to rusqlite; tests inject `:memory:`
//! or a tempfile path. Write failures are a domain Result and never panic the
//! editor. Payloads are structured JSON — never file bodies, clipboard text,
//! or secrets.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::conflict::ConflictChoice;
use crate::error::IdeError;
use crate::ports::ClockPort;

pub const EVENT_RUN_START: &str = "run_start";
pub const EVENT_OPEN_FOLDER: &str = "open_folder";
pub const EVENT_OPEN_FILE: &str = "open_file";
pub const EVENT_TREE_LOAD: &str = "tree_load";
pub const EVENT_TREE_EXPAND: &str = "tree_expand";
pub const EVENT_TAB_OPEN: &str = "tab_open";
pub const EVENT_TAB_CLOSE: &str = "tab_close";
pub const EVENT_SAVE: &str = "save";
pub const EVENT_CONTROL_CONNECT_ERROR: &str = "control_connect_error";
pub const EVENT_CONFLICT_ENQUEUE: &str = "conflict_enqueue";
pub const EVENT_CONFLICT_RESOLVE: &str = "conflict_resolve";

const FORBIDDEN_PAYLOAD_KEYS: &[&str] = &[
    "text",
    "content",
    "body",
    "clipboard",
    "password",
    "secret",
    "token",
];

/// Structured event class stored on each row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogCategory {
    Run,
    Ui,
    Tree,
    Tab,
    Buffer,
    Lsp,
    Control,
    Conflict,
}

impl LogCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Ui => "ui",
            Self::Tree => "tree",
            Self::Tab => "tab",
            Self::Buffer => "buffer",
            Self::Lsp => "lsp",
            Self::Control => "control",
            Self::Conflict => "conflict",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "run" => Some(Self::Run),
            "ui" => Some(Self::Ui),
            "tree" => Some(Self::Tree),
            "tab" => Some(Self::Tab),
            "buffer" => Some(Self::Buffer),
            "lsp" => Some(Self::Lsp),
            "control" => Some(Self::Control),
            "conflict" => Some(Self::Conflict),
            _ => None,
        }
    }
}

/// One append-only debug row. DTO.
#[derive(Clone, Debug, PartialEq)]
pub struct LogRow {
    timestamp_ms: u64,
    category: LogCategory,
    event: String,
    payload: Option<Value>,
}

impl LogRow {
    pub fn new(
        timestamp_ms: u64,
        category: LogCategory,
        event: impl Into<String>,
        payload: Option<Value>,
    ) -> Self {
        Self {
            timestamp_ms,
            category,
            event: event.into(),
            payload: sanitize_payload(payload),
        }
    }

    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    pub fn category(&self) -> LogCategory {
        self.category
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn payload(&self) -> Option<&Value> {
        self.payload.as_ref()
    }
}

/// Drop file bodies, clipboard text, and secret-shaped keys. Only objects remain.
pub fn sanitize_payload(payload: Option<Value>) -> Option<Value> {
    let Value::Object(mut map) = payload? else {
        return None;
    };
    for key in FORBIDDEN_PAYLOAD_KEYS {
        map.remove(*key);
    }
    Some(Value::Object(map))
}

/// Deterministic per-run sqlite path: `{dir}/poc-ide-{timestamp_ms}-{pid}.sqlite`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLogPath {
    path: PathBuf,
}

impl RunLogPath {
    pub fn new(dir: impl AsRef<Path>, timestamp_ms: u64, process_id: u32) -> Self {
        let name = format!("poc-ide-{timestamp_ms}-{process_id}.sqlite");
        Self {
            path: dir.as_ref().join(name),
        }
    }

    pub fn from_clock(dir: impl AsRef<Path>, clock: &impl ClockPort) -> Self {
        Self::new(dir, clock.unix_ms(), std::process::id())
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }
}

/// Directory for per-run sqlite files. Tests pass explicit values so they never
/// write to `~`.
pub fn run_log_dir(log_dir_override: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(dir) = log_dir_override {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    match home {
        Some(h) if !h.is_empty() => PathBuf::from(h)
            .join(".progressivelsp")
            .join("poc-ide-runs"),
        _ => std::env::temp_dir().join("poc-ide-runs"),
    }
}

/// Production directory: `POC_IDE_LOG_DIR`, else `$HOME/.progressivelsp/poc-ide-runs`.
pub fn default_run_log_dir() -> PathBuf {
    run_log_dir(
        std::env::var("POC_IDE_LOG_DIR").ok().as_deref(),
        std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
            .as_deref(),
    )
}

enum Sink {
    Sqlite(Connection),
    Unavailable,
}

/// Repository: one sqlite file (or `:memory:`) for a single application run.
pub struct RunLog {
    sink: Sink,
    path: Option<PathBuf>,
    clock: Box<dyn ClockPort>,
}

impl std::fmt::Debug for RunLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunLog")
            .field("path", &self.path)
            .field("memory", &self.is_memory())
            .field("unavailable", &self.is_unavailable())
            .finish()
    }
}

impl RunLog {
    pub fn memory(clock: impl ClockPort + 'static) -> Result<Self, IdeError> {
        let conn = Connection::open_in_memory().map_err(log_sql)?;
        init_schema(&conn)?;
        let mut log = Self {
            sink: Sink::Sqlite(conn),
            path: None,
            clock: Box::new(clock),
        };
        log.record_run_start();
        Ok(log)
    }

    /// Open an injected path (tempfile in tests). Does not emit `run_start`.
    pub fn open(path: impl AsRef<Path>, clock: impl ClockPort + 'static) -> Result<Self, IdeError> {
        let path = path.as_ref();
        let conn = Connection::open(path).map_err(log_sql)?;
        init_schema(&conn)?;
        Ok(Self {
            sink: Sink::Sqlite(conn),
            path: Some(path.to_path_buf()),
            clock: Box::new(clock),
        })
    }

    /// New file for this process under `dir`, named from the clock timestamp.
    pub fn create_in(
        dir: impl AsRef<Path>,
        clock: impl ClockPort + 'static,
    ) -> Result<Self, IdeError> {
        let named = RunLogPath::from_clock(dir.as_ref(), &clock);
        std::fs::create_dir_all(dir.as_ref())?;
        let mut log = Self::open(named.as_path(), clock)?;
        log.record_run_start();
        Ok(log)
    }

    /// Composition-root helper: `default_run_log_dir()` + [`RunLog::create_in`].
    pub fn open_default(clock: impl ClockPort + 'static) -> Result<Self, IdeError> {
        Self::create_in(default_run_log_dir(), clock)
    }

    /// Append always fails. [`RunLog::record`] still does not panic.
    pub fn unavailable(clock: impl ClockPort + 'static) -> Self {
        Self {
            sink: Sink::Unavailable,
            path: None,
            clock: Box::new(clock),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_memory(&self) -> bool {
        self.path.is_none() && matches!(self.sink, Sink::Sqlite(_))
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self.sink, Sink::Unavailable)
    }

    pub fn append(
        &mut self,
        category: LogCategory,
        event: impl AsRef<str>,
        payload: Option<Value>,
    ) -> Result<(), IdeError> {
        let event = event.as_ref();
        if event.is_empty() {
            return Err(IdeError::log("empty event"));
        }
        let conn = self.conn()?;
        let timestamp_ms = self.clock.unix_ms();
        let payload = sanitize_payload(payload);
        let payload_text = match payload {
            None => None,
            Some(v) => Some(serde_json::to_string(&v).map_err(|e| IdeError::log(e.to_string()))?),
        };
        let ts = i64::try_from(timestamp_ms).unwrap_or(i64::MAX);
        conn.execute(
            "INSERT INTO events (timestamp_ms, category, event, payload) VALUES (?1, ?2, ?3, ?4)",
            params![ts, category.as_str(), event, payload_text],
        )
        .map_err(log_sql)?;
        Ok(())
    }

    /// Ignore-with-classifier: a failed write never panics the editor.
    pub fn record(
        &mut self,
        category: LogCategory,
        event: impl AsRef<str>,
        payload: Option<Value>,
    ) {
        if let Err(err) = self.append(category, event, payload) {
            debug_assert!(err.is_log() || err.is_io(), "log write: {err}");
            let _ = err;
        }
    }

    pub fn rows(&self) -> Result<Vec<LogRow>, IdeError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT timestamp_ms, category, event, payload FROM events ORDER BY id ASC")
            .map_err(log_sql)?;
        let mapped = stmt
            .query_map([], |row| {
                let ts: i64 = row.get(0)?;
                let category: String = row.get(1)?;
                let event: String = row.get(2)?;
                let payload: Option<String> = row.get(3)?;
                Ok((ts, category, event, payload))
            })
            .map_err(log_sql)?;
        let mut out = Vec::new();
        for item in mapped {
            let (ts, category, event, payload) = item.map_err(log_sql)?;
            let category = LogCategory::parse(&category)
                .ok_or_else(|| IdeError::log(format!("unknown category: {category}")))?;
            let payload = payload.and_then(|s| serde_json::from_str(&s).ok());
            out.push(LogRow {
                timestamp_ms: u64::try_from(ts).unwrap_or(0),
                category,
                event,
                payload,
            });
        }
        Ok(out)
    }

    pub fn len(&self) -> Result<usize, IdeError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .map_err(log_sql)?;
        usize::try_from(n).map_err(|_| IdeError::log("count overflow"))
    }

    pub fn is_empty(&self) -> Result<bool, IdeError> {
        Ok(self.len()? == 0)
    }

    pub fn log_open_folder(&mut self, path: &Path) {
        self.record(LogCategory::Ui, EVENT_OPEN_FOLDER, json_path(path));
    }

    pub fn log_open_file(&mut self, path: &Path) {
        self.record(LogCategory::Ui, EVENT_OPEN_FILE, json_path(path));
    }

    pub fn log_tree_load(&mut self, root: &Path, child_count: usize, error: Option<&str>) {
        self.record(
            LogCategory::Tree,
            EVENT_TREE_LOAD,
            Some(json_obj([
                ("path", json!(root.display().to_string())),
                ("child_count", json!(child_count)),
                ("error", opt_str(error)),
            ])),
        );
    }

    pub fn log_tree_expand(&mut self, path: &Path, child_count: usize, error: Option<&str>) {
        self.record(
            LogCategory::Tree,
            EVENT_TREE_EXPAND,
            Some(json_obj([
                ("path", json!(path.display().to_string())),
                ("child_count", json!(child_count)),
                ("error", opt_str(error)),
            ])),
        );
    }

    pub fn log_tab_open(&mut self, path: &Path) {
        self.record(LogCategory::Tab, EVENT_TAB_OPEN, json_path(path));
    }

    pub fn log_tab_close(&mut self, path: &Path) {
        self.record(LogCategory::Tab, EVENT_TAB_CLOSE, json_path(path));
    }

    pub fn log_save(&mut self, path: &Path, error: Option<&str>) {
        self.record(
            LogCategory::Buffer,
            EVENT_SAVE,
            Some(json_obj([
                ("path", json!(path.display().to_string())),
                ("error", opt_str(error)),
            ])),
        );
    }

    /// LSP method + error only. Never file bodies.
    pub fn log_lsp(&mut self, method: &str, error: Option<&str>) {
        self.record(
            LogCategory::Lsp,
            method,
            Some(json_obj([
                ("method", json!(method)),
                ("error", opt_str(error)),
            ])),
        );
    }

    pub fn log_control_connect_error(&mut self, error: &str) {
        self.record(
            LogCategory::Control,
            EVENT_CONTROL_CONNECT_ERROR,
            Some(json!({ "error": error })),
        );
    }

    pub fn log_conflict_enqueue(&mut self, path: &Path, mtime: u64) {
        self.record(
            LogCategory::Conflict,
            EVENT_CONFLICT_ENQUEUE,
            Some(json!({
                "path": path.display().to_string(),
                "mtime": mtime,
            })),
        );
    }

    pub fn log_conflict_resolve(&mut self, path: &Path, choice: ConflictChoice) {
        let choice = match choice {
            ConflictChoice::LoadDisk => "load_disk",
            ConflictChoice::KeepMemory => "keep_memory",
        };
        self.record(
            LogCategory::Conflict,
            EVENT_CONFLICT_RESOLVE,
            Some(json!({
                "path": path.display().to_string(),
                "choice": choice,
            })),
        );
    }

    fn record_run_start(&mut self) {
        let payload = match &self.path {
            Some(path) => json!({ "path": path.display().to_string() }),
            None => json!({ "memory": true }),
        };
        self.record(LogCategory::Run, EVENT_RUN_START, Some(payload));
    }

    fn conn(&self) -> Result<&Connection, IdeError> {
        match &self.sink {
            Sink::Sqlite(conn) => Ok(conn),
            Sink::Unavailable => Err(IdeError::log("sink unavailable")),
        }
    }

    #[cfg(test)]
    fn make_query_only(&self) -> Result<(), IdeError> {
        self.conn()?
            .pragma_update(None, "query_only", true)
            .map_err(log_sql)
    }

    #[cfg(test)]
    fn insert_raw_category(&self, category: &str) -> Result<(), IdeError> {
        self.conn()?
            .execute(
                "INSERT INTO events (timestamp_ms, category, event, payload) VALUES (1, ?1, 'x', NULL)",
                params![category],
            )
            .map_err(log_sql)?;
        Ok(())
    }
}

fn init_schema(conn: &Connection) -> Result<(), IdeError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp_ms INTEGER NOT NULL,
            category TEXT NOT NULL,
            event TEXT NOT NULL,
            payload TEXT
        );",
    )
    .map_err(log_sql)
}

fn log_sql(err: rusqlite::Error) -> IdeError {
    IdeError::log(err.to_string())
}

fn json_path(path: &Path) -> Option<Value> {
    Some(json!({ "path": path.display().to_string() }))
}

fn opt_str(error: Option<&str>) -> Value {
    match error {
        Some(e) => Value::String(e.to_string()),
        None => Value::Null,
    }
}

fn json_obj(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        if !v.is_null() {
            map.insert(k.to_string(), v);
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FakeClock;
    use std::path::Path;

    fn memory_at(ms: u64) -> RunLog {
        RunLog::memory(FakeClock::at_unix_ms(ms)).unwrap()
    }

    #[test]
    fn log_category_value_object_round_trip() {
        let all = [
            LogCategory::Run,
            LogCategory::Ui,
            LogCategory::Tree,
            LogCategory::Tab,
            LogCategory::Buffer,
            LogCategory::Lsp,
            LogCategory::Control,
            LogCategory::Conflict,
        ];
        for cat in all {
            assert_eq!(
                LogCategory::parse(cat.as_str()),
                Some(cat),
                "{}",
                cat.as_str()
            );
        }
        assert_eq!(LogCategory::parse("RUN"), None);
        assert_eq!(LogCategory::parse(""), None);
        assert_eq!(LogCategory::parse("manager"), None);
        assert_eq!(LogCategory::Run.as_str(), "run");
        assert_eq!(LogCategory::Lsp.as_str(), "lsp");
    }

    #[test]
    fn log_row_dto_sanitizes_structured_payload() {
        let kept = LogRow::new(
            7,
            LogCategory::Ui,
            EVENT_OPEN_FOLDER,
            Some(json!({"path": "/ws", "text": "SECRET", "clipboard": "nope"})),
        );
        assert_eq!(kept.timestamp_ms(), 7);
        assert_eq!(kept.category(), LogCategory::Ui);
        assert_eq!(kept.event(), EVENT_OPEN_FOLDER);
        let payload = kept.payload().unwrap();
        assert_eq!(payload["path"], "/ws");
        assert!(payload.get("text").is_none());
        assert!(payload.get("clipboard").is_none());
        assert!(sanitize_payload(Some(Value::String("fn main() {}".into()))).is_none());
        assert!(sanitize_payload(Some(json!(["a"]))).is_none());
        assert!(sanitize_payload(None).is_none());
        let stripped = sanitize_payload(Some(json!({
            "content": "body",
            "body": "x",
            "password": "p",
            "secret": "s",
            "token": "t",
            "method": "textDocument/didOpen"
        })))
        .unwrap();
        assert_eq!(stripped["method"], "textDocument/didOpen");
        assert!(stripped.get("content").is_none());
        assert!(stripped.get("body").is_none());
        assert!(stripped.get("password").is_none());
        assert!(stripped.get("secret").is_none());
        assert!(stripped.get("token").is_none());
    }

    #[test]
    fn run_log_path_value_object_uses_clock_and_pid() {
        let a = RunLogPath::new("/logs", 1_700_000_000_000, 42);
        let b = RunLogPath::new("/logs", 1_700_000_000_001, 42);
        let c = RunLogPath::new("/logs", 1_700_000_000_000, 43);
        assert_eq!(
            a.as_path(),
            Path::new("/logs/poc-ide-1700000000000-42.sqlite")
        );
        assert_ne!(a, b);
        assert_ne!(a, c);
        let clock = FakeClock::at_unix_ms(99);
        let from_clock = RunLogPath::from_clock("/logs", &clock);
        assert_eq!(
            from_clock.as_path(),
            Path::new(&format!("/logs/poc-ide-99-{}.sqlite", std::process::id()))
        );
    }

    #[test]
    fn run_log_dir_never_requires_host_home() {
        assert_eq!(
            run_log_dir(Some("/injected"), Some("/home/me")),
            PathBuf::from("/injected")
        );
        assert_eq!(
            run_log_dir(Some(""), Some("/home/me")),
            PathBuf::from("/home/me/.progressivelsp/poc-ide-runs")
        );
        assert_eq!(
            run_log_dir(None, Some("/home/me")),
            PathBuf::from("/home/me/.progressivelsp/poc-ide-runs")
        );
        assert_eq!(
            run_log_dir(None, None),
            std::env::temp_dir().join("poc-ide-runs")
        );
        assert_eq!(
            run_log_dir(None, Some("")),
            std::env::temp_dir().join("poc-ide-runs")
        );
        let computed = default_run_log_dir();
        assert!(!computed.as_os_str().is_empty());
    }

    #[test]
    fn run_log_repository_memory_appends_and_queries_structured_rows() {
        let mut log = memory_at(1_000);
        assert!(log.is_memory());
        assert!(log.path().is_none());
        assert!(!log.is_unavailable());
        assert_eq!(log.len().unwrap(), 1);
        assert!(!log.is_empty().unwrap());
        let start = &log.rows().unwrap()[0];
        assert_eq!(start.category(), LogCategory::Run);
        assert_eq!(start.event(), EVENT_RUN_START);
        assert_eq!(start.timestamp_ms(), 1_000);
        assert_eq!(start.payload().unwrap()["memory"], true);

        log.append(
            LogCategory::Ui,
            EVENT_OPEN_FOLDER,
            Some(json!({"path": "/ws", "text": "do-not-store"})),
        )
        .unwrap();
        let rows = log.rows().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].event(), EVENT_OPEN_FOLDER);
        assert_eq!(rows[1].timestamp_ms(), 1_000);
        assert_eq!(rows[1].payload().unwrap()["path"], "/ws");
        assert!(rows[1].payload().unwrap().get("text").is_none());
    }

    #[test]
    fn run_log_repository_fake_clock_stamps_rows() {
        let clock = std::sync::Arc::new(FakeClock::at_unix_ms(42));
        let mut log = RunLog::memory(std::sync::Arc::clone(&clock)).unwrap();
        log.log_open_file(Path::new("/ws/a.rs"));
        clock.advance_ms(8);
        log.log_save(Path::new("/ws/a.rs"), None);
        let rows = log.rows().unwrap();
        let open = rows.iter().find(|r| r.event() == EVENT_OPEN_FILE).unwrap();
        let save = rows.iter().find(|r| r.event() == EVENT_SAVE).unwrap();
        assert_eq!(open.timestamp_ms(), 42);
        assert_eq!(save.timestamp_ms(), 50);
        assert_eq!(save.category(), LogCategory::Buffer);
        assert_eq!(save.payload().unwrap()["path"], "/ws/a.rs");
        assert!(save.payload().unwrap().get("error").is_none());
    }

    #[test]
    fn run_log_repository_one_run_file_is_not_another() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = RunLog::create_in(dir.path(), FakeClock::at_unix_ms(11)).unwrap();
        let mut second = RunLog::create_in(dir.path(), FakeClock::at_unix_ms(22)).unwrap();
        let a = first.path().unwrap().to_path_buf();
        let b = second.path().unwrap().to_path_buf();
        assert_ne!(a, b);
        assert!(a.starts_with(dir.path()));
        assert!(b.starts_with(dir.path()));
        first.log_open_folder(Path::new("/ws-a"));
        second.log_open_folder(Path::new("/ws-b"));
        let folders = |rows: &[LogRow]| -> Vec<String> {
            rows.iter()
                .filter(|r| r.event() == EVENT_OPEN_FOLDER)
                .filter_map(|r| r.payload()?.get("path")?.as_str().map(ToString::to_string))
                .collect()
        };
        assert_eq!(folders(&first.rows().unwrap()), vec!["/ws-a".to_string()]);
        assert_eq!(folders(&second.rows().unwrap()), vec!["/ws-b".to_string()]);
        drop(first);
        drop(second);
        let reopened = RunLog::open(&a, FakeClock::at_unix_ms(30)).unwrap();
        assert_eq!(
            folders(&reopened.rows().unwrap()),
            vec!["/ws-a".to_string()]
        );
    }

    #[test]
    fn run_log_repository_typed_events_cover_debug_surface() {
        let mut log = memory_at(3);
        log.log_open_folder(Path::new("/ws"));
        log.log_open_file(Path::new("/ws/a.rs"));
        log.log_tree_load(Path::new("/ws"), 4, None);
        log.log_tree_load(Path::new("/ws"), 0, Some("not a directory"));
        log.log_tree_expand(Path::new("/ws/src"), 2, None);
        log.log_tree_expand(Path::new("/ws/src"), 0, Some("not found"));
        log.log_tab_open(Path::new("/ws/a.rs"));
        log.log_tab_close(Path::new("/ws/a.rs"));
        log.log_save(Path::new("/ws/a.rs"), Some("denied"));
        log.log_lsp("initialize", None);
        log.log_lsp("textDocument/didOpen", None);
        log.log_lsp("textDocument/didChange", None);
        log.log_lsp("textDocument/didSave", None);
        log.log_lsp("textDocument/definition", Some("eof"));
        log.log_lsp("textDocument/implementation", None);
        log.log_lsp("textDocument/references", None);
        log.log_control_connect_error("control socket missing");
        log.log_conflict_enqueue(Path::new("/ws/a.rs"), 9);
        log.log_conflict_resolve(Path::new("/ws/a.rs"), ConflictChoice::LoadDisk);
        log.log_conflict_resolve(Path::new("/ws/a.rs"), ConflictChoice::KeepMemory);

        let rows = log.rows().unwrap();
        let events: Vec<&str> = rows.iter().map(|r| r.event()).collect();
        assert!(events.contains(&EVENT_OPEN_FOLDER));
        assert!(events.contains(&EVENT_OPEN_FILE));
        assert!(events.contains(&EVENT_TREE_LOAD));
        assert!(events.contains(&EVENT_TREE_EXPAND));
        assert!(events.contains(&EVENT_TAB_OPEN));
        assert!(events.contains(&EVENT_TAB_CLOSE));
        assert!(events.contains(&EVENT_SAVE));
        assert!(events.contains(&"initialize"));
        assert!(events.contains(&"textDocument/didOpen"));
        assert!(events.contains(&"textDocument/didChange"));
        assert!(events.contains(&"textDocument/didSave"));
        assert!(events.contains(&"textDocument/definition"));
        assert!(events.contains(&"textDocument/implementation"));
        assert!(events.contains(&"textDocument/references"));
        assert!(events.contains(&EVENT_CONTROL_CONNECT_ERROR));
        assert!(events.contains(&EVENT_CONFLICT_ENQUEUE));
        assert!(events.contains(&EVENT_CONFLICT_RESOLVE));
        let def = rows
            .iter()
            .find(|r| r.event() == "textDocument/definition")
            .unwrap();
        assert_eq!(def.category(), LogCategory::Lsp);
        assert_eq!(def.payload().unwrap()["method"], "textDocument/definition");
        assert_eq!(def.payload().unwrap()["error"], "eof");
        assert!(def.payload().unwrap().get("text").is_none());
        let tree_err = rows
            .iter()
            .filter(|r| r.event() == EVENT_TREE_LOAD)
            .find(|r| r.payload().unwrap().get("error").is_some())
            .unwrap();
        assert_eq!(tree_err.payload().unwrap()["child_count"], 0);
        let expand_ok = rows
            .iter()
            .filter(|r| r.event() == EVENT_TREE_EXPAND)
            .find(|r| r.payload().unwrap().get("error").is_none())
            .unwrap();
        assert_eq!(expand_ok.payload().unwrap()["path"], "/ws/src");
        assert_eq!(expand_ok.payload().unwrap()["child_count"], 2);
        let expand_err = rows
            .iter()
            .filter(|r| r.event() == EVENT_TREE_EXPAND)
            .find(|r| r.payload().unwrap().get("error").is_some())
            .unwrap();
        assert_eq!(expand_err.payload().unwrap()["error"], "not found");
        let keep = rows
            .iter()
            .filter(|r| r.event() == EVENT_CONFLICT_RESOLVE)
            .find(|r| r.payload().unwrap()["choice"] == "keep_memory")
            .unwrap();
        assert_eq!(keep.payload().unwrap()["choice"], "keep_memory");
        let load = rows
            .iter()
            .find(|r| {
                r.event() == EVENT_CONFLICT_RESOLVE && r.payload().unwrap()["choice"] == "load_disk"
            })
            .unwrap();
        assert_eq!(load.category(), LogCategory::Conflict);
    }

    #[test]
    fn run_log_repository_write_failure_does_not_panic() {
        let mut down = RunLog::unavailable(FakeClock::at_unix_ms(1));
        assert!(down.is_unavailable());
        assert!(!down.is_memory());
        let err = down
            .append(LogCategory::Ui, EVENT_OPEN_FOLDER, None)
            .unwrap_err();
        assert!(err.is_log());
        assert!(err.to_string().contains("unavailable"));
        down.record(
            LogCategory::Ui,
            EVENT_OPEN_FOLDER,
            Some(json!({"path": "/ws"})),
        );
        down.log_open_folder(Path::new("/ws"));
        down.log_lsp("initialize", Some("gone"));
        assert!(down.rows().unwrap_err().is_log());
        assert!(down.len().unwrap_err().is_log());
        assert!(down.is_empty().unwrap_err().is_log());

        let mut log = memory_at(2);
        log.make_query_only().unwrap();
        let write_err = log
            .append(
                LogCategory::Tab,
                EVENT_TAB_OPEN,
                json_path(Path::new("/ws/a.rs")),
            )
            .unwrap_err();
        assert!(write_err.is_log());
        log.record(
            LogCategory::Tab,
            EVENT_TAB_CLOSE,
            json_path(Path::new("/ws/a.rs")),
        );
        log.log_save(Path::new("/ws/a.rs"), None);
    }

    #[test]
    fn run_log_repository_rejects_empty_event_and_unknown_category() {
        let mut log = memory_at(4);
        let empty = log.append(LogCategory::Ui, "", None).unwrap_err();
        assert!(empty.is_log());
        assert!(empty.to_string().contains("empty event"));
        log.insert_raw_category("nope").unwrap();
        let bad = log.rows().unwrap_err();
        assert!(bad.is_log());
        assert!(bad.to_string().contains("unknown category"));
    }

    #[test]
    fn run_log_repository_open_default_uses_injected_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var("POC_IDE_LOG_DIR").ok();
        std::env::set_var("POC_IDE_LOG_DIR", tmp.path());
        let result = RunLog::open_default(FakeClock::at_unix_ms(5));
        match prev {
            Some(v) => std::env::set_var("POC_IDE_LOG_DIR", v),
            None => std::env::remove_var("POC_IDE_LOG_DIR"),
        }
        let log = result.unwrap();
        let path = log.path().unwrap();
        assert!(path.starts_with(tmp.path()));
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("poc-ide-5-"));
        assert!(log
            .rows()
            .unwrap()
            .iter()
            .any(|r| r.event() == EVENT_RUN_START));
    }

    #[test]
    fn run_log_repository_create_in_fails_when_dir_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let err = RunLog::create_in(&file, FakeClock::at_unix_ms(1)).unwrap_err();
        assert!(err.is_io() || err.is_log());
    }

    #[test]
    fn run_log_repository_open_missing_parent_is_domain_result() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope").join("run.sqlite");
        let err = RunLog::open(&missing, FakeClock::at_unix_ms(1)).unwrap_err();
        assert!(err.is_log());
    }
}
