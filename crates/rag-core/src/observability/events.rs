use chrono::{DateTime, Local};
use rag_types::wire::{TraceEventWire, TraceKind, TraceLevel};
use tracing::Level;

#[derive(Debug, Clone)]
pub enum EventKind {
    /// An ordinary log event.
    Log,
    /// A span finished; carries how long it took.
    SpanClosed { elapsed_ms: u64 },
}

/// One tracing event, as captured by the layer and streamed to clients.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub ts: DateTime<Local>,
    pub level: Level,
    /// Innermost enclosing span, e.g. `ingest_file{file=notes.pdf}`.
    pub span: Option<String>,
    pub message: String,
    pub fields: Vec<(String, String)>,
    pub kind: EventKind,
}

impl TraceEvent {
    pub fn to_wire(&self) -> TraceEventWire {
        let level = match self.level {
            Level::ERROR => TraceLevel::Error,
            Level::WARN => TraceLevel::Warn,
            Level::INFO => TraceLevel::Info,
            Level::DEBUG => TraceLevel::Debug,
            Level::TRACE => TraceLevel::Trace,
        };
        TraceEventWire {
            ts_millis: self.ts.timestamp_millis(),
            level,
            span: self.span.clone(),
            message: self.message.clone(),
            fields: self.fields.clone(),
            kind: match self.kind {
                EventKind::Log => TraceKind::Log,
                EventKind::SpanClosed { elapsed_ms } => TraceKind::SpanClosed { elapsed_ms },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_the_wire_format() {
        let e = TraceEvent {
            ts: Local::now(),
            level: Level::WARN,
            span: Some("search".into()),
            message: "slow".into(),
            fields: vec![("ms".into(), "900".into())],
            kind: EventKind::SpanClosed { elapsed_ms: 900 },
        };
        let w = e.to_wire();
        assert_eq!(w.level, TraceLevel::Warn);
        assert!(matches!(w.kind, TraceKind::SpanClosed { elapsed_ms: 900 }));
        assert_eq!(w.fields[0].0, "ms");
    }
}
