//! Observer + Adapter: child stderr lines → [`RunLog`]. Bounded so a noisy
//! serve cannot stall LSP stdout. Does not depend on `progressive-lsp-log`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::Mutex;

use crate::log::RunLog;

/// Same cap as server `ChildStderrAdapter`. Overflow drops oldest.
pub const STDERR_DRAIN_CAP: usize = 1024;

/// Observer + Adapter. Line-delimited child stderr, never child stdout.
#[derive(Debug, Default)]
pub struct ChildStderrDrain {
    lines: Mutex<VecDeque<String>>,
}

impl ChildStderrDrain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_line(&self, line: impl Into<String>) {
        let mut q = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == STDERR_DRAIN_CAP {
            q.pop_front();
        }
        q.push_back(line.into());
    }

    pub fn drain(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Write drained lines into [`RunLog`] (`category=lsp`, `event=child_stderr`).
    pub fn write_to(&self, log: &mut RunLog) {
        for line in self.drain() {
            log.log_child_stderr(&line);
        }
    }

    /// Drain a line source (tests pass a `Cursor`; spawn attaches the pipe).
    pub fn drain_reader(&self, reader: impl Read) {
        let mut buf = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match buf.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    if bytes.ends_with(b"\n") {
                        bytes.pop();
                    }
                    if bytes.ends_with(b"\r") {
                        bytes.pop();
                    }
                    self.push_line(String::from_utf8_lossy(&bytes).into_owned());
                }
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{LogCategory, EVENT_CHILD_STDERR};
    use crate::ports::FakeClock;
    use std::io::Cursor;

    #[test]
    fn child_stderr_drain_observer_adapter_overflow_drops_oldest() {
        let drain = ChildStderrDrain::new();
        assert!(drain.is_empty());
        for i in 0..STDERR_DRAIN_CAP + 2 {
            drain.push_line(format!("l{i}"));
        }
        assert_eq!(drain.len(), STDERR_DRAIN_CAP);
        let mut log = RunLog::memory(FakeClock::at_unix_ms(1)).unwrap();
        drain.write_to(&mut log);
        assert!(drain.is_empty());
        let rows: Vec<_> = log
            .rows()
            .unwrap()
            .into_iter()
            .filter(|r| r.event() == EVENT_CHILD_STDERR)
            .collect();
        assert_eq!(rows.len(), STDERR_DRAIN_CAP);
        assert_eq!(rows[0].category(), LogCategory::Lsp);
        assert_eq!(rows[0].payload().unwrap()["line"], "l2");
        assert_eq!(
            rows.last().unwrap().payload().unwrap()["line"],
            format!("l{}", STDERR_DRAIN_CAP + 1)
        );
        drain.drain_reader(Cursor::new(b"one\r\ntwo\n"));
        assert_eq!(drain.len(), 2);
        drain.write_to(&mut log);
        let extra: Vec<_> = log
            .rows()
            .unwrap()
            .into_iter()
            .filter(|r| r.event() == EVENT_CHILD_STDERR)
            .collect();
        assert_eq!(extra.len(), STDERR_DRAIN_CAP + 2);
        assert_eq!(extra[STDERR_DRAIN_CAP].payload().unwrap()["line"], "one");
        assert_eq!(
            extra[STDERR_DRAIN_CAP + 1].payload().unwrap()["line"],
            "two"
        );
        drain.drain_reader(Cursor::new([0xff, 0xfe, b'z', b'\n']));
        assert_eq!(drain.len(), 1);
        assert!(drain.drain()[0].contains('z'));
        drain.drain_reader(Cursor::new(Vec::<u8>::new()));
        assert!(drain.is_empty());
    }
}
