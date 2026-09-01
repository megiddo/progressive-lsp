//! `LogFileTailAdapter` — Adapter. Engine log files (zls / biome). Never parses LSP.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use progressive_lsp_core::{
    message_from_bytes, LogComponent, LogLevel, LogOrigin, LogPort, LogRecord,
};

/// Adapter. Tail an engine log **file**. Prefer `$PREFIX/log/<pack>/`.
pub struct LogFileTailAdapter {
    port: Arc<dyn LogPort>,
    pack: String,
    path: PathBuf,
    offset: Mutex<u64>,
}

impl LogFileTailAdapter {
    pub fn new(port: Arc<dyn LogPort>, pack: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            port,
            pack: pack.into(),
            path: path.into(),
            offset: Mutex::new(0),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wire only when a tail path exists. Missing file is still attached (poll is silent).
    pub fn attach_if_tail_path(
        tail_path: Option<&Path>,
        port: Arc<dyn LogPort>,
        pack: &str,
    ) -> Option<Self> {
        tail_path.map(|path| Self::new(port, pack, path))
    }

    /// Read newly appended bytes. Missing file / IO errors are silent (never fail).
    /// Lines that look like LSP `Content-Length` are emitted as text, not parsed.
    pub fn poll(&self) {
        let mut file = match File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let mut offset = self.offset.lock().expect("tail");
        if file.seek(SeekFrom::Start(*offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return;
        }
        *offset += buf.len() as u64;
        drop(offset);
        if buf.is_empty() {
            return;
        }
        let text = message_from_bytes(&buf);
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            self.emit_line(line);
        }
    }

    fn emit_line(&self, line: &str) {
        let mut rec = LogRecord::at_caller(LogLevel::Info, line);
        rec.source_repo = LogOrigin::ThirdParty;
        rec.component = Some(LogComponent::new(self.pack.clone()));
        rec.operation = Some("spawn".into());
        rec.source_crate = Some(self.pack.clone());
        self.port.emit(rec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;

    #[test]
    fn log_file_tail_adapter_attaches_only_when_path_exists() {
        let log = FakeLog::new();
        assert!(
            LogFileTailAdapter::attach_if_tail_path(None, Arc::new(log.clone()), "zls").is_none()
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zls.log");
        std::fs::write(&path, "engine line\n").unwrap();
        let tail = LogFileTailAdapter::attach_if_tail_path(
            Some(path.as_path()),
            Arc::new(log.clone()),
            "zls",
        )
        .expect("tail path attaches");
        tail.poll();
        let recs = log.records();
        assert!(
            recs.iter().any(|r| r.level == LogLevel::Info
                && r.source_repo == LogOrigin::ThirdParty
                && r.operation.as_deref() == Some("spawn")
                && r.message == "engine line"),
            "{recs:?}"
        );
    }

    #[test]
    fn log_file_tail_adapter_does_not_parse_lsp_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zls.log");
        let log = FakeLog::new();
        let tail = LogFileTailAdapter::new(Arc::new(log.clone()), "zls", &path);
        assert_eq!(tail.path(), path.as_path());
        tail.poll();
        assert!(log.records().is_empty());
        std::fs::write(&path, "hello\nContent-Length: 2\r\n\r\n{}\n").unwrap();
        tail.poll();
        let recs = log.records();
        assert!(recs.iter().any(|r| r.message == "hello"));
        assert!(
            recs.iter().any(|r| r.message.starts_with("Content-Length")),
            "{recs:?}"
        );
        assert_eq!(recs[0].source_repo, LogOrigin::ThirdParty);
        std::fs::write(&path, "hello\nContent-Length: 2\r\n\r\n{}\nmore\n").unwrap();
        tail.poll();
        assert!(log.records().iter().any(|r| r.message == "more"));
    }

    #[test]
    fn log_file_tail_lossy_utf8_and_missing_file_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("biome.log");
        let log = FakeLog::new();
        let tail = LogFileTailAdapter::new(Arc::new(log.clone()), "biome", &path);
        tail.poll();
        std::fs::write(&path, [0xff, 0x00, b'x', b'\n']).unwrap();
        tail.poll();
        assert!(!log.records().is_empty());
        tail.poll();
        let n = log.records().len();
        tail.poll();
        assert_eq!(log.records().len(), n);
    }
}
