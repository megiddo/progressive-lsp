//! `LogPort`, records, scope, and test-double / never-fail logs.
//! Core stays sqlite-free. Process-wide `OnceLock<LogPort>` is forbidden.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::panic::Location;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Maximum `LogRecord.message` size (64 KiB). Truncate on a char boundary.
pub const MESSAGE_MAX_BYTES: usize = 64 * 1024;

/// `MemoryLog` ring capacity (bootstrap + sqlite-open failure).
pub const MEMORY_LOG_CAP: usize = 4096;

const FORBIDDEN_EXTRA_KEYS: &[&str] = &[
    "text",
    "content",
    "body",
    "clipboard",
    "password",
    "secret",
    "token",
];

/// Dependency injection / Port. `emit` returns `()`; no `Result`.
/// Libs take `Arc<dyn LogPort>`. Do not install a process-wide `OnceLock`.
pub trait LogPort: Send + Sync {
    fn emit(&self, record: LogRecord);

    #[track_caller]
    fn error(&self, message: &str) {
        self.emit(LogRecord::at_caller(LogLevel::Error, message));
    }

    #[track_caller]
    fn warn(&self, message: &str) {
        self.emit(LogRecord::at_caller(LogLevel::Warn, message));
    }

    #[track_caller]
    fn info(&self, message: &str) {
        self.emit(LogRecord::at_caller(LogLevel::Info, message));
    }

    #[track_caller]
    fn debug(&self, message: &str) {
        self.emit(LogRecord::at_caller(LogLevel::Debug, message));
    }

    #[track_caller]
    fn trace(&self, message: &str) {
        self.emit(LogRecord::at_caller(LogLevel::Trace, message));
    }
}

/// Port for durable append. Prod sqlite lives in `progressive-lsp-log` (LOG-2).
pub trait LogSink: Send + Sync {
    fn append(&self, record: LogRecord) -> Result<(), String>;
}

/// Value object. Unknown parse → `info` (never fail).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// Never-fail parse. Unknown → [`LogLevel::Info`].
    pub fn parse(s: &str) -> Self {
        Self::parse_known(s).unwrap_or(Self::Info)
    }

    /// Strict parse for config: unknown is `None` so the overlay can warn.
    pub fn parse_known(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// Severity rank. Lower is more severe (`error` = 0 … `trace` = 4).
    pub fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
            Self::Trace => 4,
        }
    }

    /// Keep this record when filtering to `min` (Facade, LOG-3).
    pub fn at_least(self, min: Self) -> bool {
        self.rank() <= min.rank()
    }
}

/// Value object. `source_repo` is `progressive-lsp` or `third-party`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogOrigin {
    #[default]
    FirstParty,
    ThirdParty,
}

impl LogOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstParty => "progressive-lsp",
            Self::ThirdParty => "third-party",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "third-party" => Self::ThirdParty,
            _ => Self::FirstParty,
        }
    }
}

/// Value object. Stable strings: `core`, `protocol`, `control`, `engine`,
/// `index`, `watch`, `install`, `script`, `lang-<id>`, pack name, `xtask`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LogComponent(String);

impl LogComponent {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn core() -> Self {
        Self::new("core")
    }

    pub fn protocol() -> Self {
        Self::new("protocol")
    }

    pub fn control() -> Self {
        Self::new("control")
    }

    pub fn engine() -> Self {
        Self::new("engine")
    }

    pub fn index() -> Self {
        Self::new("index")
    }

    pub fn watch() -> Self {
        Self::new("watch")
    }

    pub fn install() -> Self {
        Self::new("install")
    }

    pub fn script() -> Self {
        Self::new("script")
    }

    pub fn xtask() -> Self {
        Self::new("xtask")
    }

    pub fn lang(id: &str) -> Self {
        Self::new(format!("lang-{id}"))
    }
}

/// DTO. Construction never fails. Omit unknown fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub ts_unix_ms: u64,
    pub level: LogLevel,
    pub component: Option<LogComponent>,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
    pub source_repo: LogOrigin,
    pub source_crate: Option<String>,
    pub content_path: Option<String>,
    pub content_file: Option<String>,
    pub content_line: Option<u32>,
    pub operation: Option<String>,
    pub message: String,
    pub extras: Option<BTreeMap<String, String>>,
}

impl LogRecord {
    /// Never-fail constructor. `ts_unix_ms` starts at 0 (Facade stamps `ClockPort`).
    #[track_caller]
    pub fn at_caller(level: LogLevel, message: impl Into<String>) -> Self {
        let loc = Location::caller();
        Self {
            ts_unix_ms: 0,
            level,
            component: None,
            source_file: Some(loc.file().to_string()),
            source_line: Some(loc.line()),
            source_repo: LogOrigin::FirstParty,
            source_crate: None,
            content_path: None,
            content_file: None,
            content_line: None,
            operation: None,
            message: truncate_message(message.into()),
            extras: None,
        }
    }

    /// Apply [`LogScope`] defaults, basename, extras sanitizer, and message cap.
    pub fn prepared(mut self) -> Self {
        if let Some(scope) = LogScope::current() {
            if self.component.is_none() {
                self.component = scope.component.clone();
            }
            if self.content_path.is_none() {
                self.content_path = scope.content_path.clone();
            }
            if self.content_line.is_none() {
                self.content_line = scope.content_line;
            }
            if self.operation.is_none() {
                self.operation = scope.operation.clone();
            }
        }
        if self.content_file.is_none() {
            if let Some(ref path) = self.content_path {
                self.content_file = Some(content_basename(path));
            }
        }
        if let Some(extras) = self.extras.take() {
            self.extras = sanitize_extras(extras);
        }
        self.message = truncate_message(std::mem::take(&mut self.message));
        self
    }
}

/// Drop secret-shaped keys. Empty map → `None` (omit).
pub fn sanitize_extras(mut extras: BTreeMap<String, String>) -> Option<BTreeMap<String, String>> {
    for key in FORBIDDEN_EXTRA_KEYS {
        extras.remove(*key);
    }
    if extras.is_empty() {
        None
    } else {
        Some(extras)
    }
}

/// Lossy UTF-8 then 64 KiB truncate. Never fails.
pub fn message_from_bytes(bytes: &[u8]) -> String {
    truncate_message(String::from_utf8_lossy(bytes).into_owned())
}

fn truncate_message(s: String) -> String {
    if s.len() <= MESSAGE_MAX_BYTES {
        return s;
    }
    let mut end = MESSAGE_MAX_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn content_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.to_string())
}

thread_local! {
    static SCOPE_STACK: RefCell<Vec<LogScope>> = const { RefCell::new(Vec::new()) };
}

/// Context Object. Task-local / thread-local path, line, operation, component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogScope {
    pub content_path: Option<String>,
    pub content_line: Option<u32>,
    pub operation: Option<String>,
    pub component: Option<LogComponent>,
}

impl LogScope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.content_path = Some(path.into());
        self
    }

    pub fn line(mut self, line: u32) -> Self {
        self.content_line = Some(line);
        self
    }

    pub fn operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    pub fn component(mut self, component: LogComponent) -> Self {
        self.component = Some(component);
        self
    }

    pub fn current() -> Option<Self> {
        SCOPE_STACK.with(|stack| stack.borrow().last().cloned())
    }

    /// Push this scope. Drop of the guard restores the previous scope (stack).
    pub fn enter(self) -> LogScopeGuard {
        SCOPE_STACK.with(|stack| stack.borrow_mut().push(self));
        LogScopeGuard { _private: () }
    }
}

/// RAII guard for [`LogScope`] (same Context Object). Drop pops the stack.
#[must_use]
pub struct LogScopeGuard {
    _private: (),
}

impl Drop for LogScopeGuard {
    fn drop(&mut self) {
        SCOPE_STACK.with(|stack| {
            let _ = stack.borrow_mut().pop();
        });
    }
}

/// Test double. Records into a mutex `Vec`. Same `LogPort` / `LogSink`.
#[derive(Clone, Default)]
pub struct FakeLog {
    records: Arc<Mutex<Vec<LogRecord>>>,
}

impl FakeLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> Vec<LogRecord> {
        self.records.lock().expect("FakeLog").clone()
    }
}

impl LogPort for FakeLog {
    fn emit(&self, record: LogRecord) {
        self.records
            .lock()
            .expect("FakeLog")
            .push(record.prepared());
    }
}

impl LogSink for FakeLog {
    fn append(&self, record: LogRecord) -> Result<(), String> {
        self.emit(record);
        Ok(())
    }
}

/// Test double / bootstrap ring. Cap [`MEMORY_LOG_CAP`]. Drops oldest.
#[derive(Clone, Default)]
pub struct MemoryLog {
    records: Arc<Mutex<VecDeque<LogRecord>>>,
}

impl MemoryLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<LogRecord> {
        self.records
            .lock()
            .expect("MemoryLog")
            .iter()
            .cloned()
            .collect()
    }

    pub fn drain(&self) -> Vec<LogRecord> {
        self.records.lock().expect("MemoryLog").drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.records.lock().expect("MemoryLog").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl LogPort for MemoryLog {
    fn emit(&self, record: LogRecord) {
        let mut q = self.records.lock().expect("MemoryLog");
        if q.len() == MEMORY_LOG_CAP {
            q.pop_front();
        }
        q.push_back(record.prepared());
    }
}

impl LogSink for MemoryLog {
    fn append(&self, record: LogRecord) -> Result<(), String> {
        self.emit(record);
        Ok(())
    }
}

/// Test double. Never-fail no-op. `emit` does not panic.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullLog;

impl LogPort for NullLog {
    fn emit(&self, record: LogRecord) {
        let _ = record.prepared();
    }
}

impl LogSink for NullLog {
    fn append(&self, record: LogRecord) -> Result<(), String> {
        self.emit(record);
        Ok(())
    }
}

/// Decorator. Wraps a [`LogSink`] that may return `Result`. Swallows errors.
pub struct NeverFailLog<S> {
    inner: S,
}

impl<S> NeverFailLog<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }
}

impl<S: LogSink> LogPort for NeverFailLog<S> {
    fn emit(&self, record: LogRecord) {
        let _ = self.inner.append(record.prepared());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingSink;

    impl LogSink for FailingSink {
        fn append(&self, _: LogRecord) -> Result<(), String> {
            Err("disk full".into())
        }
    }

    #[test]
    fn log_port_emit_returns_unit_not_result() {
        let log = FakeLog::new();
        let _: () = log.emit(LogRecord::at_caller(LogLevel::Info, "p"));
        let port: Arc<dyn LogPort> = Arc::new(log.clone());
        port.info("via dyn");
        assert_eq!(log.records().len(), 2);
        let other = FakeLog::new();
        other.warn("independent");
        assert_eq!(other.records().len(), 1);
        assert_eq!(log.records().len(), 2);
    }

    #[test]
    fn log_record_construction_never_fails_dto() {
        let rec = LogRecord::at_caller(LogLevel::Error, "");
        assert_eq!(rec.message, "");
        assert_eq!(rec.ts_unix_ms, 0);
        assert_eq!(rec.source_repo, LogOrigin::FirstParty);
        assert!(rec.source_file.is_some());
        assert!(rec.source_line.is_some());
        let huge = "x".repeat(MESSAGE_MAX_BYTES + 16);
        let rec = LogRecord::at_caller(LogLevel::Warn, huge);
        assert_eq!(rec.message.len(), MESSAGE_MAX_BYTES);
        let rec = LogRecord {
            extras: Some(BTreeMap::from([("ok".into(), "1".into())])),
            ..LogRecord::at_caller(LogLevel::Info, "m")
        }
        .prepared();
        assert_eq!(
            rec.extras
                .as_ref()
                .and_then(|e| e.get("ok"))
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn log_level_unknown_parse_is_info_value_object() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
        assert_eq!(LogLevel::parse("error"), LogLevel::Error);
        assert_eq!(LogLevel::parse("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::parse("info"), LogLevel::Info);
        assert_eq!(LogLevel::parse("debug"), LogLevel::Debug);
        assert_eq!(LogLevel::parse("trace"), LogLevel::Trace);
        assert_eq!(LogLevel::parse("verbose"), LogLevel::Info);
        assert_eq!(LogLevel::parse("INFO"), LogLevel::Info);
        assert_eq!(LogLevel::parse(""), LogLevel::Info);
        assert_eq!(LogLevel::parse_known("info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse_known("verbose"), None);
        assert_eq!(LogLevel::Error.as_str(), "error");
        assert_eq!(LogLevel::Warn.as_str(), "warn");
        assert_eq!(LogLevel::Info.as_str(), "info");
        assert_eq!(LogLevel::Debug.as_str(), "debug");
        assert_eq!(LogLevel::Trace.as_str(), "trace");
        assert_eq!(LogLevel::Error.rank(), 0);
        assert_eq!(LogLevel::Warn.rank(), 1);
        assert_eq!(LogLevel::Info.rank(), 2);
        assert_eq!(LogLevel::Debug.rank(), 3);
        assert_eq!(LogLevel::Trace.rank(), 4);
        assert!(LogLevel::Error.at_least(LogLevel::Info));
        assert!(LogLevel::Info.at_least(LogLevel::Info));
        assert!(!LogLevel::Debug.at_least(LogLevel::Info));
        assert!(!LogLevel::Trace.at_least(LogLevel::Error));
        assert!(LogLevel::Warn.at_least(LogLevel::Warn));
        assert!(!LogLevel::Info.at_least(LogLevel::Warn));
    }

    #[test]
    fn log_origin_source_repo_strings_are_value_object() {
        assert_eq!(LogOrigin::FirstParty.as_str(), "progressive-lsp");
        assert_eq!(LogOrigin::ThirdParty.as_str(), "third-party");
        assert_eq!(LogOrigin::parse("progressive-lsp"), LogOrigin::FirstParty);
        assert_eq!(LogOrigin::parse("third-party"), LogOrigin::ThirdParty);
        assert_eq!(LogOrigin::parse("other"), LogOrigin::FirstParty);
        assert_eq!(LogOrigin::default(), LogOrigin::FirstParty);
    }

    #[test]
    fn log_component_stable_strings_are_value_object() {
        assert_eq!(LogComponent::core().as_str(), "core");
        assert_eq!(LogComponent::protocol().as_str(), "protocol");
        assert_eq!(LogComponent::control().as_str(), "control");
        assert_eq!(LogComponent::engine().as_str(), "engine");
        assert_eq!(LogComponent::index().as_str(), "index");
        assert_eq!(LogComponent::watch().as_str(), "watch");
        assert_eq!(LogComponent::install().as_str(), "install");
        assert_eq!(LogComponent::script().as_str(), "script");
        assert_eq!(LogComponent::xtask().as_str(), "xtask");
        assert_eq!(LogComponent::lang("java").as_str(), "lang-java");
        assert_eq!(LogComponent::new("clangd").as_str(), "clangd");
    }

    #[test]
    fn log_scope_nest_and_restore_is_context_object() {
        assert!(LogScope::current().is_none());
        let outer = LogScope::new()
            .path("/ws/A.java")
            .line(3)
            .operation("index")
            .component(LogComponent::index())
            .enter();
        let cur = LogScope::current().unwrap();
        assert_eq!(cur.content_path.as_deref(), Some("/ws/A.java"));
        assert_eq!(cur.content_line, Some(3));
        assert_eq!(cur.operation.as_deref(), Some("index"));
        assert_eq!(
            cur.component.as_ref().map(LogComponent::as_str),
            Some("index")
        );
        {
            let _inner = LogScope::new().operation("textDocument/definition").enter();
            let inner = LogScope::current().unwrap();
            assert_eq!(inner.operation.as_deref(), Some("textDocument/definition"));
            assert!(inner.content_path.is_none());
        }
        let restored = LogScope::current().unwrap();
        assert_eq!(restored.operation.as_deref(), Some("index"));
        assert_eq!(restored.content_path.as_deref(), Some("/ws/A.java"));
        drop(outer);
        assert!(LogScope::current().is_none());
    }

    #[test]
    fn emit_copies_unset_scope_fields_and_keeps_caller_set() {
        let log = FakeLog::new();
        let _g = LogScope::new()
            .path("/ws/B.rs")
            .line(9)
            .operation("index")
            .component(LogComponent::index())
            .enter();
        log.info("from scope");
        let rec = &log.records()[0];
        assert_eq!(rec.content_path.as_deref(), Some("/ws/B.rs"));
        assert_eq!(rec.content_file.as_deref(), Some("B.rs"));
        assert_eq!(rec.content_line, Some(9));
        assert_eq!(rec.operation.as_deref(), Some("index"));
        assert_eq!(
            rec.component.as_ref().map(LogComponent::as_str),
            Some("index")
        );
        let mut override_rec = LogRecord::at_caller(LogLevel::Warn, "set");
        override_rec.operation = Some("serve".into());
        override_rec.component = Some(LogComponent::core());
        log.emit(override_rec);
        let rec = &log.records()[1];
        assert_eq!(rec.operation.as_deref(), Some("serve"));
        assert_eq!(
            rec.component.as_ref().map(LogComponent::as_str),
            Some("core")
        );
        assert_eq!(rec.content_path.as_deref(), Some("/ws/B.rs"));
    }

    #[test]
    fn never_fail_log_swallows_sink_errors_decorator() {
        let log = NeverFailLog::new(FailingSink);
        let _: () = log.emit(LogRecord::at_caller(LogLevel::Error, "full"));
        log.error("still unit");
        let _ = log.inner();
        let wrapped = NeverFailLog::new(NullLog);
        wrapped.info("null sink");
        let fake = FakeLog::new();
        let wrapped = NeverFailLog::new(fake.clone());
        wrapped.warn("through decorator");
        assert_eq!(fake.records().len(), 1);
        assert_eq!(fake.records()[0].level, LogLevel::Warn);
    }

    #[test]
    fn fake_log_records_into_vec_test_double() {
        let log = FakeLog::new();
        log.error("e");
        log.warn("w");
        log.info("i");
        log.debug("d");
        log.trace("t");
        let recs = log.records();
        assert_eq!(recs.len(), 5);
        assert_eq!(recs[0].level, LogLevel::Error);
        assert_eq!(recs[1].level, LogLevel::Warn);
        assert_eq!(recs[2].level, LogLevel::Info);
        assert_eq!(recs[3].level, LogLevel::Debug);
        assert_eq!(recs[4].level, LogLevel::Trace);
        assert_eq!(recs[2].message, "i");
        assert_eq!(recs[2].source_repo.as_str(), "progressive-lsp");
        let _ = FakeLog::append(&log, LogRecord::at_caller(LogLevel::Info, "sink"));
        assert_eq!(log.records().len(), 6);
    }

    #[test]
    fn memory_log_caps_at_4096_test_double() {
        let log = MemoryLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        log.info("keep");
        assert!(!log.is_empty());
        assert_eq!(log.snapshot().len(), 1);
        assert_eq!(log.snapshot()[0].message, "keep");
        let drained = log.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].message, "keep");
        assert!(log.is_empty());
        for i in 0..MEMORY_LOG_CAP {
            log.info(&format!("n{i}"));
        }
        assert_eq!(log.len(), MEMORY_LOG_CAP);
        assert_eq!(log.snapshot()[0].message, "n0");
        log.info("overflow");
        assert_eq!(log.len(), MEMORY_LOG_CAP);
        assert_eq!(log.snapshot()[0].message, "n1");
        assert_eq!(log.snapshot().last().unwrap().message, "overflow");
        let drained = log.drain();
        assert_eq!(drained.len(), MEMORY_LOG_CAP);
        assert!(log.is_empty());
        let _ = MemoryLog::append(&log, LogRecord::at_caller(LogLevel::Info, "again"));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn null_log_emit_is_noop_never_fail_test_double() {
        let log = NullLog;
        log.error("e");
        log.warn("w");
        log.info("i");
        log.debug("d");
        log.trace("t");
        let _: () = log.emit(LogRecord::at_caller(LogLevel::Info, "x"));
        assert!(NullLog::append(&log, LogRecord::at_caller(LogLevel::Warn, "s")).is_ok());
        let _ = NullLog;
    }

    #[test]
    fn log_sink_append_is_port() {
        let fake = FakeLog::new();
        assert!(LogSink::append(&fake, LogRecord::at_caller(LogLevel::Info, "a")).is_ok());
        assert_eq!(fake.records().len(), 1);
    }

    #[test]
    fn sanitize_extras_drops_secret_keys_dto() {
        let mut extras = BTreeMap::new();
        extras.insert("text".into(), "fn main".into());
        extras.insert("content".into(), "body".into());
        extras.insert("body".into(), "x".into());
        extras.insert("clipboard".into(), "y".into());
        extras.insert("password".into(), "p".into());
        extras.insert("secret".into(), "s".into());
        extras.insert("token".into(), "t".into());
        extras.insert("path".into(), "/ws/A.java".into());
        let clean = sanitize_extras(extras).unwrap();
        assert_eq!(clean.get("path").map(String::as_str), Some("/ws/A.java"));
        assert!(!clean.contains_key("text"));
        assert!(!clean.contains_key("token"));
        assert!(sanitize_extras(BTreeMap::from([("secret".into(), "x".into())])).is_none());
        assert!(sanitize_extras(BTreeMap::new()).is_none());
        let rec = LogRecord {
            extras: Some(BTreeMap::from([
                ("token".into(), "leak".into()),
                ("op".into(), "index".into()),
            ])),
            ..LogRecord::at_caller(LogLevel::Info, "e")
        }
        .prepared();
        assert_eq!(
            rec.extras
                .as_ref()
                .and_then(|e| e.get("op"))
                .map(String::as_str),
            Some("index")
        );
        assert!(rec.extras.as_ref().unwrap().get("token").is_none());
    }

    #[test]
    fn message_from_bytes_is_lossy_and_truncated() {
        assert_eq!(message_from_bytes(b"ok"), "ok");
        assert_eq!(message_from_bytes(&[0xff, 0xfe]), "\u{FFFD}\u{FFFD}");
        let mut bytes = vec![b'a'; MESSAGE_MAX_BYTES + 8];
        bytes.extend_from_slice(&[0xff, 0x80]);
        let msg = message_from_bytes(&bytes);
        assert!(msg.len() <= MESSAGE_MAX_BYTES);
        let emoji = "é".repeat((MESSAGE_MAX_BYTES / 2) + 8);
        let rec = LogRecord::at_caller(LogLevel::Info, emoji);
        assert!(rec.message.len() <= MESSAGE_MAX_BYTES);
        assert!(rec.message.is_char_boundary(rec.message.len()));
        // 65535 ASCII bytes + 2-byte UTF-8: cap sits mid-character.
        let mut split = "a".repeat(MESSAGE_MAX_BYTES - 1);
        split.push('é');
        assert_eq!(split.len(), MESSAGE_MAX_BYTES + 1);
        let rec = LogRecord::at_caller(LogLevel::Info, split);
        assert_eq!(rec.message.len(), MESSAGE_MAX_BYTES - 1);
        assert!(rec.message.is_char_boundary(rec.message.len()));
        assert!(rec.message.chars().all(|c| c == 'a'));
    }

    #[test]
    fn content_basename_from_path() {
        assert_eq!(content_basename("/ws/pkg/A.java"), "A.java");
        assert_eq!(content_basename("A.java"), "A.java");
        assert_eq!(content_basename("/"), "/");
    }

    #[test]
    fn convenience_methods_fill_track_caller_location() {
        let log = FakeLog::new();
        log.info("here");
        let rec = &log.records()[0];
        assert!(
            rec.source_file
                .as_deref()
                .is_some_and(|f| f.ends_with("log.rs")),
            "{:?}",
            rec.source_file
        );
        assert!(rec.source_line.unwrap() > 0);
    }
}
