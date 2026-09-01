//! `CliUsageAdapter` — Adapter. `--help` / usage also writes stderr (IT-1.7).

use std::sync::Arc;

use progressive_lsp_core::{LogComponent, LogLevel, LogOrigin, LogPort, LogRecord};

/// Adapter. CLI usage/help stays on stderr and is also a `LogPort::warn`.
pub struct CliUsageAdapter {
    port: Arc<dyn LogPort>,
}

impl CliUsageAdapter {
    pub fn new(port: Arc<dyn LogPort>) -> Self {
        Self { port }
    }

    pub fn emit_usage(&self, text: &str) {
        eprint_usage(text);
        let mut rec = LogRecord::at_caller(LogLevel::Warn, text);
        rec.source_repo = LogOrigin::FirstParty;
        rec.component = Some(LogComponent::core());
        rec.operation = Some("cli".into());
        self.port.emit(rec);
    }
}

/// IT-1.7: usage/help is the one product `eprintln!` exception.
fn eprint_usage(text: &str) {
    eprintln!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;

    #[test]
    fn cli_usage_adapter_logs_warn_operation_cli() {
        let log = FakeLog::new();
        CliUsageAdapter::new(Arc::new(log.clone())).emit_usage("usage: progressive-lsp");
        let recs = log.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].level, LogLevel::Warn);
        assert_eq!(recs[0].operation.as_deref(), Some("cli"));
        assert_eq!(recs[0].message, "usage: progressive-lsp");
        assert_eq!(recs[0].source_repo, LogOrigin::FirstParty);
    }
}
