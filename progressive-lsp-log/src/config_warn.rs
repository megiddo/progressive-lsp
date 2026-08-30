//! `ConfigWarnAdapter` — Adapter. `ConfigLoad.warnings` → `operation=config`.

use std::sync::Arc;

use progressive_lsp_core::{LogComponent, LogLevel, LogOrigin, LogPort, LogRecord, LogScope};

/// Adapter. Unknown keys and invalid `[log]` values emit `warn`.
pub struct ConfigWarnAdapter {
    port: Arc<dyn LogPort>,
}

impl ConfigWarnAdapter {
    pub fn new(port: Arc<dyn LogPort>) -> Self {
        Self { port }
    }

    pub fn emit_warnings(&self, warnings: &[String]) {
        let _g = LogScope::enter(
            LogScope::new()
                .operation("config")
                .component(LogComponent::core()),
        );
        for warning in warnings {
            let mut rec = LogRecord::at_caller(LogLevel::Warn, warning);
            rec.source_repo = LogOrigin::FirstParty;
            rec.component = Some(LogComponent::core());
            rec.operation = Some("config".into());
            self.port.emit(rec);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;

    #[test]
    fn config_warn_adapter_emits_warn_operation_config() {
        let log = FakeLog::new();
        ConfigWarnAdapter::new(Arc::new(log.clone()))
            .emit_warnings(&["unknown config key ignored: future".into()]);
        ConfigWarnAdapter::new(Arc::new(log.clone())).emit_warnings(&[]);
        let recs = log.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].level, LogLevel::Warn);
        assert_eq!(recs[0].operation.as_deref(), Some("config"));
        assert_eq!(recs[0].source_repo, LogOrigin::FirstParty);
        assert!(recs[0].message.contains("future"));
    }
}
