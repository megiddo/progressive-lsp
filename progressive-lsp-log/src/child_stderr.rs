//! `ChildStderrAdapter` — Observer + Adapter. Never attached to child stdout.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{
    message_from_bytes, LogComponent, LogLevel, LogOrigin, LogPort, LogRecord,
};

/// Bounded unread stderr lines so a noisy pack cannot stall LSP stdout.
pub const STDERR_DRAIN_CAP: usize = 1024;

/// Observer + Adapter. Line-delimited pack stderr → [`LogPort`].
pub struct ChildStderrAdapter {
    port: Arc<dyn LogPort>,
    pack: String,
}

impl ChildStderrAdapter {
    pub fn new(port: Arc<dyn LogPort>, pack: impl Into<String>) -> Self {
        Self {
            port,
            pack: pack.into(),
        }
    }

    /// `Some` only when a stderr pipe exists. stdout is never this Adapter.
    pub fn attach_if_stderr_pipe(
        has_stderr_pipe: bool,
        stdout_is_lsp: bool,
        port: Arc<dyn LogPort>,
        pack: &str,
    ) -> Option<Self> {
        if !stdout_is_lsp || !has_stderr_pipe {
            return None;
        }
        Some(Self::new(port, pack))
    }

    pub fn ingest_bytes(&self, bytes: &[u8]) {
        self.ingest_line(&message_from_bytes(bytes));
    }

    pub fn ingest_line(&self, line: &str) {
        let (level, source_crate, message) = parse_stderr_line(line);
        let mut rec = LogRecord::at_caller(level, message);
        rec.source_repo = LogOrigin::ThirdParty;
        rec.source_crate = source_crate;
        rec.component = Some(LogComponent::new(self.pack.clone()));
        rec.operation = Some("spawn".into());
        self.port.emit(rec);
    }

    pub fn drain_fake(&self, fake: &FakeChildStderr) {
        for line in fake.drain() {
            self.ingest_line(&line);
        }
    }
}

/// Test double. Bounded line source for [`ChildStderrAdapter`].
#[derive(Default)]
pub struct FakeChildStderr {
    lines: Mutex<VecDeque<String>>,
}

impl FakeChildStderr {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_line(&self, line: impl Into<String>) {
        let mut q = self.lines.lock().expect("FakeChildStderr");
        if q.len() == STDERR_DRAIN_CAP {
            q.pop_front();
        }
        q.push_back(line.into());
    }

    pub fn push_bytes(&self, bytes: &[u8]) {
        self.push_line(message_from_bytes(bytes));
    }

    pub fn drain(&self) -> Vec<String> {
        self.lines
            .lock()
            .expect("FakeChildStderr")
            .drain(..)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.lines.lock().expect("FakeChildStderr").len()
    }
}

/// Best-effort `LEVEL module: message`. Never panics. No regex.
fn parse_stderr_line(line: &str) -> (LogLevel, Option<String>, String) {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return (LogLevel::Info, None, String::new());
    }
    let (head, rest) = match trimmed.split_once(char::is_whitespace) {
        Some(pair) => pair,
        None => {
            if let Some(level) = level_token(trimmed) {
                return (level, None, String::new());
            }
            return (LogLevel::Info, None, trimmed.to_string());
        }
    };
    let Some(level) = level_token(head) else {
        return (LogLevel::Info, None, trimmed.to_string());
    };
    if let Some((module, message)) = rest.split_once(": ") {
        let module = module.trim();
        if !module.is_empty() && !module.contains(' ') {
            return (level, Some(module.to_string()), message.trim().to_string());
        }
    }
    if let Some((module, message)) = rest.rsplit_once(':') {
        let module = module.trim();
        if !module.is_empty() && !module.contains(' ') {
            return (level, Some(module.to_string()), message.trim().to_string());
        }
    }
    (level, None, rest.trim().to_string())
}

fn level_token(token: &str) -> Option<LogLevel> {
    let t = token
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    match t.as_str() {
        "error" | "err" => Some(LogLevel::Error),
        "warn" | "warning" => Some(LogLevel::Warn),
        "info" => Some(LogLevel::Info),
        "debug" | "dbg" => Some(LogLevel::Debug),
        "trace" => Some(LogLevel::Trace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;

    #[test]
    fn child_stderr_adapter_never_attaches_to_stdout_observer() {
        let log = FakeLog::new();
        assert!(ChildStderrAdapter::attach_if_stderr_pipe(
            true,
            false,
            Arc::new(log.clone()),
            "python"
        )
        .is_none());
        assert!(ChildStderrAdapter::attach_if_stderr_pipe(
            false,
            true,
            Arc::new(log.clone()),
            "python"
        )
        .is_none());
        let attached =
            ChildStderrAdapter::attach_if_stderr_pipe(true, true, Arc::new(log.clone()), "python");
        assert!(attached.is_some());
    }

    #[test]
    fn child_stderr_line_parse_is_best_effort_adapter() {
        let log = FakeLog::new();
        let adapter = ChildStderrAdapter::new(Arc::new(log.clone()), "rust-analyzer");
        adapter.ingest_line("ERROR rust_analyzer::main: boom");
        adapter.ingest_line("not a level line");
        adapter.ingest_bytes(b"WARN foo:hi");
        adapter.ingest_line("INFO");
        adapter.ingest_line("DEBUG x: d");
        adapter.ingest_line("TRACE y: t");
        adapter.ingest_line("");
        adapter.ingest_bytes(&[0xff, 0xfe, b'z']);
        let recs = log.records();
        assert_eq!(recs[0].level, LogLevel::Error);
        assert_eq!(recs[0].source_crate.as_deref(), Some("rust_analyzer::main"));
        assert_eq!(recs[0].message, "boom");
        assert_eq!(recs[0].source_repo, LogOrigin::ThirdParty);
        assert_eq!(recs[1].level, LogLevel::Info);
        assert_eq!(recs[1].message, "not a level line");
        assert_eq!(recs[2].level, LogLevel::Warn);
        assert_eq!(recs[2].message, "hi");
        assert_eq!(recs[3].level, LogLevel::Info);
        assert_eq!(recs[4].level, LogLevel::Debug);
        assert_eq!(recs[5].level, LogLevel::Trace);
        assert!(recs.iter().any(|r| r.message.contains('z')));
    }

    #[test]
    fn fake_child_stderr_bounded_drain_drops_oldest() {
        let fake = FakeChildStderr::new();
        for i in 0..STDERR_DRAIN_CAP + 2 {
            fake.push_line(format!("l{i}"));
        }
        assert_eq!(fake.len(), STDERR_DRAIN_CAP);
        let log = FakeLog::new();
        let adapter = ChildStderrAdapter::new(Arc::new(log.clone()), "ty");
        adapter.drain_fake(&fake);
        let recs = log.records();
        assert_eq!(recs.len(), STDERR_DRAIN_CAP);
        assert_eq!(recs[0].message, "l2");
        assert!(fake.drain().is_empty());
        fake.push_bytes(b"one");
        assert_eq!(fake.len(), 1);
    }
}
