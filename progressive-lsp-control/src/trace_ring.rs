//! In-memory trace rows keyed by LSP `traceId` (SEAMS-3). TTL by id count.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use progressive_lsp_core::{LogLevel, LogRecord};

use crate::messages::TraceRow;

const MAX_TRACE_IDS: usize = 10_000;
const DEFAULT_MAX_ROWS: u32 = 500;

#[derive(Debug, Default)]
pub struct TraceRing {
    inner: Mutex<TraceRingInner>,
}

#[derive(Debug, Default)]
struct TraceRingInner {
    rows: HashMap<String, Vec<TraceRow>>,
    order: VecDeque<String>,
}

impl TraceRing {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, trace_id: &str, record: &LogRecord) {
        if trace_id.is_empty() {
            return;
        }
        let row = log_record_to_row(record);
        let mut g = self.inner.lock().expect("trace ring");
        if !g.rows.contains_key(trace_id) {
            g.order.push_back(trace_id.to_string());
            while g.order.len() > MAX_TRACE_IDS {
                if let Some(old) = g.order.pop_front() {
                    g.rows.remove(&old);
                }
            }
        }
        g.rows.entry(trace_id.to_string()).or_default().push(row);
    }

    pub fn fetch(&self, trace_id: &str, max_rows: u32) -> Vec<TraceRow> {
        let cap = if max_rows == 0 {
            DEFAULT_MAX_ROWS
        } else {
            max_rows
        } as usize;
        let g = self.inner.lock().expect("trace ring");
        g.rows
            .get(trace_id)
            .map(|v| {
                let start = v.len().saturating_sub(cap);
                v[start..].to_vec()
            })
            .unwrap_or_default()
    }
}

fn log_record_to_row(record: &LogRecord) -> TraceRow {
    let mut extras = std::collections::BTreeMap::new();
    if let Some(map) = &record.extras {
        for (k, v) in map {
            extras.insert(k.clone(), v.clone());
        }
    }
    TraceRow {
        unix_ms: record.ts_unix_ms,
        level: log_level_str(record.level).into(),
        component: record
            .component
            .as_ref()
            .map(|c| c.as_str().to_string())
            .unwrap_or_default(),
        operation: record.operation.clone().unwrap_or_default(),
        message: record.message.clone(),
        extras,
    }
}

fn log_level_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_core::LogRecord;

    #[test]
    fn fetch_empty_id_is_not_found_rows() {
        let ring = TraceRing::new();
        assert!(ring.fetch("", 10).is_empty());
        assert!(ring.fetch("missing", 10).is_empty());
    }

    #[test]
    fn append_and_fetch_respects_max_rows() {
        let ring = TraceRing::new();
        for i in 0..5 {
            let mut rec = LogRecord::at_caller(LogLevel::Info, "op");
            rec.message = format!("m{i}");
            ring.append("tid", &rec);
        }
        let rows = ring.fetch("tid", 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].message, "m3");
        assert_eq!(rows[1].message, "m4");
    }
}
