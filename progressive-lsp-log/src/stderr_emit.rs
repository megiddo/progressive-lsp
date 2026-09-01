//! `StderrEmitAdapter` — Adapter. Former diagnostic `eprintln!` sites.

use std::sync::Arc;

use progressive_lsp_core::{LogComponent, LogLevel, LogOrigin, LogPort, LogRecord};

/// Adapter. Product diagnostic fatals go here, never `eprintln!`.
pub struct StderrEmitAdapter {
    port: Arc<dyn LogPort>,
}

impl StderrEmitAdapter {
    pub fn new(port: Arc<dyn LogPort>) -> Self {
        Self { port }
    }

    pub fn emit(&self, message: &str) {
        let mut rec = LogRecord::at_caller(LogLevel::Error, message);
        rec.source_repo = LogOrigin::FirstParty;
        rec.component = Some(LogComponent::core());
        rec.operation = Some("serve".into());
        self.port.emit(rec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;

    #[test]
    fn stderr_emit_adapter_writes_port_not_eprintln() {
        let log = FakeLog::new();
        StderrEmitAdapter::new(Arc::new(log.clone())).emit("fatal");
        let recs = log.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].level, LogLevel::Error);
        assert_eq!(recs[0].message, "fatal");
        assert_eq!(recs[0].source_repo, LogOrigin::FirstParty);
    }
}
