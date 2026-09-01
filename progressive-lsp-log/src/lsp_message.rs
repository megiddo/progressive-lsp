//! `LspLogMessageAdapter` — Adapter. Secondary; never a crash substitute.

use std::sync::Arc;

use progressive_lsp_core::{LogComponent, LogLevel, LogOrigin, LogPort, LogRecord};
use serde_json::Value;

/// Adapter. `window/logMessage` / `window/showMessage` / `$/logTrace`.
pub struct LspLogMessageAdapter {
    port: Arc<dyn LogPort>,
    pack: String,
}

impl LspLogMessageAdapter {
    pub fn new(port: Arc<dyn LogPort>, pack: impl Into<String>) -> Self {
        Self {
            port,
            pack: pack.into(),
        }
    }

    /// `Some` only for proxied `window/logMessage` / `window/showMessage` / `$/logTrace`.
    pub fn attach_if_proxied(method: &str, port: Arc<dyn LogPort>, pack: &str) -> Option<Self> {
        match method {
            "window/logMessage" | "window/showMessage" | "$/logTrace" => {
                Some(Self::new(port, pack))
            }
            _ => None,
        }
    }

    pub fn ingest_log_message(&self, lsp_type: u64, message: &str) {
        self.ingest(
            "window/logMessage",
            &serde_json::json!({"type": lsp_type, "message": message}),
        );
    }

    pub fn ingest_log_trace(&self, message: &str) {
        self.ingest("$/logTrace", &serde_json::json!({"message": message}));
    }

    /// Ingest a proxied engine notification. Unknown methods are ignored.
    pub fn ingest(&self, method: &str, params: &Value) {
        let (level, message) = match method {
            "window/logMessage" | "window/showMessage" => {
                let ty = params.get("type").and_then(Value::as_u64).unwrap_or(3);
                let msg = params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                (lsp_message_type(ty), msg)
            }
            "$/logTrace" => {
                let msg = params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                (LogLevel::Debug, msg)
            }
            _ => return,
        };
        let mut rec = LogRecord::at_caller(level, message);
        rec.source_repo = LogOrigin::ThirdParty;
        rec.component = Some(LogComponent::new(self.pack.clone()));
        rec.operation = Some("spawn".into());
        rec.source_crate = Some(self.pack.clone());
        self.port.emit(rec);
    }
}

fn lsp_message_type(ty: u64) -> LogLevel {
    match ty {
        1 => LogLevel::Error,
        2 => LogLevel::Warn,
        4 => LogLevel::Debug,
        _ => LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::FakeLog;
    use serde_json::json;

    #[test]
    fn lsp_log_message_adapter_is_secondary_not_crash_substitute() {
        let log = FakeLog::new();
        let adapter = LspLogMessageAdapter::new(Arc::new(log.clone()), "ty");
        adapter.ingest(
            "window/logMessage",
            &json!({"type": 1, "message": "engine err"}),
        );
        adapter.ingest(
            "window/showMessage",
            &json!({"type": 2, "message": "shown"}),
        );
        adapter.ingest("$/logTrace", &json!({"message": "trace-me"}));
        adapter.ingest("window/logMessage", &json!({"type": 3, "message": "i"}));
        adapter.ingest("window/logMessage", &json!({"type": 4, "message": "log"}));
        adapter.ingest("textDocument/definition", &json!({}));
        adapter.ingest("window/logMessage", &json!({}));
        adapter.ingest_log_message(1, "helper-err");
        adapter.ingest_log_trace("helper-trace");
        assert!(LspLogMessageAdapter::attach_if_proxied(
            "textDocument/definition",
            Arc::new(log.clone()),
            "ty"
        )
        .is_none());
        assert!(LspLogMessageAdapter::attach_if_proxied(
            "window/logMessage",
            Arc::new(log.clone()),
            "ty"
        )
        .is_some());
        assert!(
            LspLogMessageAdapter::attach_if_proxied("$/logTrace", Arc::new(log.clone()), "ty")
                .is_some()
        );
        assert!(LspLogMessageAdapter::attach_if_proxied(
            "window/showMessage",
            Arc::new(log.clone()),
            "ty"
        )
        .is_some());
        let recs = log.records();
        assert_eq!(recs.len(), 8);
        assert_eq!(recs[0].level, LogLevel::Error);
        assert_eq!(recs[0].message, "engine err");
        assert_eq!(recs[0].source_repo, LogOrigin::ThirdParty);
        assert_eq!(recs[0].operation.as_deref(), Some("spawn"));
        assert_eq!(recs[1].level, LogLevel::Warn);
        assert_eq!(recs[2].level, LogLevel::Debug);
        assert_eq!(recs[2].message, "trace-me");
        assert_eq!(recs[3].level, LogLevel::Info);
        assert_eq!(recs[4].level, LogLevel::Debug);
        assert_eq!(recs[6].message, "helper-err");
        assert_eq!(recs[7].message, "helper-trace");
        assert!(
            recs.iter().all(|r| r.operation.as_deref() != Some("crash")),
            "logMessage is not a crash substitute"
        );
    }
}
