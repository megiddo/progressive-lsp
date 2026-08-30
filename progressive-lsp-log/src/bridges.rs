//! `LogCrateBridge` / `TracingBridge` — Adapters. Installed only in the bin.

use std::sync::Arc;

use progressive_lsp_core::{LogComponent, LogLevel, LogOrigin, LogPort, LogRecord};
use tracing::field::{Field, Visit};
use tracing::Event;
use tracing_subscriber::layer::{Context, Layer};

/// Adapter. `log::Log::log` → [`LogPort`]. Origin is third-party unless the
/// target starts with `progressive_lsp`.
pub struct LogCrateBridge {
    port: Arc<dyn LogPort>,
}

impl LogCrateBridge {
    pub fn new(port: Arc<dyn LogPort>) -> Self {
        Self { port }
    }

    /// Composition-root / bin only. Tests call [`log::Log`] methods directly.
    pub fn try_install(port: Arc<dyn LogPort>) -> Result<(), log::SetLoggerError> {
        let leaked: &'static Self = Box::leak(Box::new(Self::new(port)));
        log::set_logger(leaked)?;
        log::set_max_level(log::LevelFilter::Trace);
        Ok(())
    }

    fn origin_for_target(target: &str) -> LogOrigin {
        if target.starts_with("progressive_lsp") {
            LogOrigin::FirstParty
        } else {
            LogOrigin::ThirdParty
        }
    }

    fn level_from_log(level: log::Level) -> LogLevel {
        match level {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Trace,
        }
    }
}

impl log::Log for LogCrateBridge {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target();
        let mut rec = LogRecord::at_caller(
            Self::level_from_log(record.level()),
            record.args().to_string(),
        );
        rec.source_repo = Self::origin_for_target(target);
        rec.source_crate = Some(target.to_string());
        rec.source_file = record.file().map(str::to_string);
        rec.source_line = record.line();
        rec.component = Some(LogComponent::new(target));
        rec.operation = Some("log".into());
        self.port.emit(rec);
    }

    fn flush(&self) {}
}

/// Adapter. `tracing` events → [`LogPort`]. Same origin rule as [`LogCrateBridge`].
pub struct TracingBridge {
    port: Arc<dyn LogPort>,
}

impl TracingBridge {
    pub fn new(port: Arc<dyn LogPort>) -> Self {
        Self { port }
    }

    /// Composition-root / bin only. Tests use `with_default`.
    pub fn try_install(
        port: Arc<dyn LogPort>,
    ) -> Result<(), tracing_subscriber::util::TryInitError> {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        tracing_subscriber::registry()
            .with(Self::new(port))
            .try_init()
    }

    fn origin_for_target(target: &str) -> LogOrigin {
        LogCrateBridge::origin_for_target(target)
    }

    fn level_from_tracing(level: &tracing::Level) -> LogLevel {
        match *level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }
}

struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && self.message.is_empty() {
            self.message = format!("{value:?}");
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_debug(field, &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_debug(field, &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_debug(field, &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_debug(field, &value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_debug(field, &value);
    }
}

impl<S> Layer<S> for TracingBridge
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();
        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);
        if visitor.message.is_empty() {
            visitor.message = target.to_string();
        }
        let mut rec = LogRecord::at_caller(Self::level_from_tracing(meta.level()), visitor.message);
        rec.source_repo = Self::origin_for_target(target);
        rec.source_crate = Some(target.to_string());
        rec.source_file = meta.file().map(str::to_string);
        rec.source_line = meta.line();
        rec.component = Some(LogComponent::new(target));
        rec.operation = Some("log".into());
        self.port.emit(rec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;
    use progressive_lsp_core::FakeLog;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn log_crate_bridge_origin_is_third_party_unless_progressive_lsp_adapter() {
        let log = FakeLog::new();
        let bridge = LogCrateBridge::new(Arc::new(log.clone()));
        bridge.log(
            &log::Record::builder()
                .args(format_args!("from other"))
                .level(log::Level::Warn)
                .target("serde_json")
                .build(),
        );
        bridge.log(
            &log::Record::builder()
                .args(format_args!("from us"))
                .level(log::Level::Error)
                .target("progressive_lsp_core::log")
                .file(Some("log.rs"))
                .line(Some(9))
                .build(),
        );
        bridge.log(
            &log::Record::builder()
                .args(format_args!("dbg"))
                .level(log::Level::Debug)
                .target("x")
                .build(),
        );
        bridge.log(
            &log::Record::builder()
                .args(format_args!("tr"))
                .level(log::Level::Trace)
                .target("x")
                .build(),
        );
        bridge.log(
            &log::Record::builder()
                .args(format_args!("inf"))
                .level(log::Level::Info)
                .target("x")
                .build(),
        );
        bridge.flush();
        assert!(bridge.enabled(
            &log::Metadata::builder()
                .level(log::Level::Info)
                .target("x")
                .build()
        ));
        let recs = log.records();
        assert_eq!(recs[0].source_repo, LogOrigin::ThirdParty);
        assert_eq!(recs[0].level, LogLevel::Warn);
        assert_eq!(recs[0].message, "from other");
        assert_eq!(recs[1].source_repo, LogOrigin::FirstParty);
        assert_eq!(recs[1].level, LogLevel::Error);
        assert_eq!(recs[1].source_file.as_deref(), Some("log.rs"));
        assert_eq!(recs[1].source_line, Some(9));
        assert_eq!(recs[2].level, LogLevel::Debug);
        assert_eq!(recs[3].level, LogLevel::Trace);
        assert_eq!(recs[4].level, LogLevel::Info);
    }

    #[test]
    fn tracing_bridge_origin_rule_matches_log_crate_adapter() {
        let log = FakeLog::new();
        let layer = TracingBridge::new(Arc::new(log.clone()));
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "eframe", "ide event");
            tracing::info!(target: "progressive_lsp_log::bridges", "ours");
            tracing::error!(target: "other", "boom");
            tracing::debug!(target: "other", "d");
            tracing::trace!(target: "other", "t");
        });
        let recs = log.records();
        assert_eq!(recs.len(), 5);
        assert_eq!(recs[0].source_repo, LogOrigin::ThirdParty);
        assert_eq!(recs[0].level, LogLevel::Warn);
        assert!(recs[0].message.contains("ide event"));
        assert_eq!(recs[1].source_repo, LogOrigin::FirstParty);
        assert_eq!(recs[2].level, LogLevel::Error);
        assert_eq!(recs[3].level, LogLevel::Debug);
        assert_eq!(recs[4].level, LogLevel::Trace);
    }
}
