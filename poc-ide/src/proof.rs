//! `ProofStatus` DTO the footer renders. Last discover comes from `RunLog`
//! discover rows / `DiscoverCommand` — not from an LSP IO thread.

use std::path::Path;

use crate::log::{LogRow, CHILD_LOG_LEVEL, SERVE_WAL_NOT_OPEN};

/// DTO / Value object. Binary basename, log level, both sqlite paths, last discover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofStatus {
    binary_basename: String,
    log_level: String,
    run_log_path: String,
    serve_wal_path: String,
    last_discover: String,
}

impl ProofStatus {
    pub fn new(
        binary_basename: impl Into<String>,
        log_level: impl Into<String>,
        run_log_path: impl Into<String>,
        serve_wal_path: impl Into<String>,
        last_discover: impl Into<String>,
    ) -> Self {
        Self {
            binary_basename: binary_basename.into(),
            log_level: log_level.into(),
            run_log_path: run_log_path.into(),
            serve_wal_path: serve_wal_path.into(),
            last_discover: last_discover.into(),
        }
    }

    pub fn from_parts(
        binary: Option<&Path>,
        log_level: &str,
        run_log_path: Option<&Path>,
        serve_wal_path: Option<&Path>,
        last_discover: impl Into<String>,
    ) -> Self {
        Self::new(
            binary
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "missing".into()),
            log_level,
            run_log_path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ":memory:".into()),
            serve_wal_path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| SERVE_WAL_NOT_OPEN.into()),
            last_discover,
        )
    }

    pub fn binary_basename(&self) -> &str {
        &self.binary_basename
    }

    pub fn log_level(&self) -> &str {
        &self.log_level
    }

    pub fn run_log_path(&self) -> &str {
        &self.run_log_path
    }

    pub fn serve_wal_path(&self) -> &str {
        &self.serve_wal_path
    }

    pub fn last_discover(&self) -> &str {
        &self.last_discover
    }

    /// `definition L23:88 → 0 locations`
    pub fn discover_line(method: &str, line: u32, character: u32, location_count: u64) -> String {
        let short = method.rsplit('/').next().unwrap_or(method);
        format!("{short} L{line}:{character} → {location_count} locations")
    }

    pub fn last_discover_from_rows(rows: &[LogRow]) -> String {
        rows.iter()
            .rev()
            .find_map(|row| {
                let payload = row.payload()?;
                let count = payload.get("location_count")?.as_u64()?;
                let line = u32::try_from(payload.get("line")?.as_u64()?).ok()?;
                let character = u32::try_from(payload.get("character")?.as_u64()?).ok()?;
                let method = payload.get("method")?.as_str()?;
                Some(Self::discover_line(method, line, character, count))
            })
            .unwrap_or_default()
    }

    pub fn footer_line(&self) -> String {
        let mut line = format!(
            "{}  {}  run={}  wal={}",
            self.binary_basename, self.log_level, self.run_log_path, self.serve_wal_path
        );
        if !self.last_discover.is_empty() {
            line.push_str("  ");
            line.push_str(&self.last_discover);
        }
        line
    }
}

impl Default for ProofStatus {
    fn default() -> Self {
        Self::from_parts(None, CHILD_LOG_LEVEL, None, None, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{LogCategory, RunLog};
    use crate::ports::FakeClock;
    use std::path::Path;

    #[test]
    fn proof_status_dto_formats_footer_and_last_discover() {
        let status = ProofStatus::from_parts(
            Some(Path::new("/opt/bin/progressive-lsp")),
            CHILD_LOG_LEVEL,
            Some(Path::new("/logs/poc-ide-1-2.sqlite")),
            Some(Path::new("/pfx/log/serve-1-2.sqlite")),
            ProofStatus::discover_line("textDocument/definition", 23, 88, 0),
        );
        assert_eq!(status.binary_basename(), "progressive-lsp");
        assert_eq!(status.log_level(), "debug");
        assert_eq!(status.run_log_path(), "/logs/poc-ide-1-2.sqlite");
        assert_eq!(status.serve_wal_path(), "/pfx/log/serve-1-2.sqlite");
        assert_eq!(status.last_discover(), "definition L23:88 → 0 locations");
        assert_eq!(
            status.footer_line(),
            "progressive-lsp  debug  run=/logs/poc-ide-1-2.sqlite  wal=/pfx/log/serve-1-2.sqlite  definition L23:88 → 0 locations"
        );
        assert_eq!(
            ProofStatus::discover_line("textDocument/references", 0, 1, 3),
            "references L0:1 → 3 locations"
        );

        let pending = ProofStatus::default();
        assert_eq!(pending.binary_basename(), "missing");
        assert_eq!(pending.log_level(), "debug");
        assert_eq!(pending.run_log_path(), ":memory:");
        assert_eq!(pending.serve_wal_path(), SERVE_WAL_NOT_OPEN);
        assert!(pending.last_discover().is_empty());
        assert_eq!(
            pending.footer_line(),
            format!("missing  debug  run=:memory:  wal={SERVE_WAL_NOT_OPEN}")
        );

        let mut log = RunLog::memory(FakeClock::at_unix_ms(1)).unwrap();
        log.log_lsp("initialize", None);
        log.log_discover(
            "textDocument/definition",
            Path::new("/ws/a.rs"),
            "file:///ws/a.rs",
            23,
            88,
            Some(0),
            None,
        );
        log.log_discover(
            "textDocument/implementation",
            Path::new("/ws/a.rs"),
            "file:///ws/a.rs",
            1,
            2,
            Some(4),
            None,
        );
        let from_log = ProofStatus::last_discover_from_rows(&log.rows().unwrap());
        assert_eq!(from_log, "implementation L1:2 → 4 locations");
        assert_eq!(ProofStatus::last_discover_from_rows(&[]), "");
        let only_lsp = LogRow::new(1, LogCategory::Lsp, "initialize", None);
        assert_eq!(ProofStatus::last_discover_from_rows(&[only_lsp]), "");
        let named = ProofStatus::new(
            "bin",
            "debug",
            "run.sqlite",
            "wal.sqlite",
            "definition L0:0 → 1 locations",
        );
        assert!(named
            .footer_line()
            .contains("definition L0:0 → 1 locations"));
    }
}
