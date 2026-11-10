//! Types that only exist on the wire between the server and its clients.

use crate::{
    chat::{ChatAnswer, ChatMemory, RagStage},
    document::{IngestOutcome, Stage},
    search::RetrievalOptions,
};
use serde::{Deserialize, Serialize};

/// The server's defaults and capabilities, so a client can start with the same settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerConfig {
    pub version: String,
    /// Default retrieval settings (mode, top-k, reranking, ...)
    pub retrieval: RetrievalOptions,
    /// Rewrite follow-up questions into standalone searches
    pub rewrite: bool,
    /// Stream answers token by token
    pub stream: bool,
    pub default_collection: String,
    pub embed_model: String,
    pub chat_model: String,
    pub reranker_configured: bool,
    pub max_upload_mb: usize,
    /// `GET /events` is available
    pub events_enabled: bool,
    /// Requests need `Authorization: Bearer <token>`
    pub auth_required: bool,
}

/// One decoded event of `POST /chat/stream`.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    Stage(RagStage),
    Token(String),
    Done(ChatAnswer),
    Error(String),
}

/// One decoded event of `POST /ingest/stream`.
#[derive(Debug, Clone)]
pub enum IngestEvent {
    Stage(Stage),
    Done(IngestOutcome),
    Error(String),
}

/// Body of `POST /memory/compact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CompactRequest {
    pub memory: ChatMemory,
}

/// Result of a compaction: replace the first `dropped` turns with `summary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CompactResponse {
    /// `None` when the memory was still short enough
    pub summary: Option<String>,
    pub dropped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceKind {
    Log,
    /// A span finished; carries how long it took.
    SpanClosed { elapsed_ms: u64 },
}

/// One tracing event from the server (`GET /events`, event name `trace`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEventWire {
    /// Unix time in milliseconds (UTC); clients format it in their own time zone.
    pub ts_millis: i64,
    pub level: TraceLevel,
    /// Innermost enclosing span, e.g. `ingest_file{file=notes.pdf}`
    pub span: Option<String>,
    pub message: String,
    pub fields: Vec<(String, String)>,
    #[serde(flatten)]
    pub kind: TraceKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyRowWire {
    pub name: String,
    pub last: u64,
    pub avg: u64,
    pub p95: u64,
    pub history: Vec<u64>,
}

/// An operation running on the server right now.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ActiveOpWire {
    pub name: String,
    pub detail: String,
    pub elapsed_ms: u64,
}

/// Counters, latencies and in-flight operations (`GET /metrics`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MetricsSnapshot {
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub chunks_indexed: u64,
    pub embeddings: u64,
    pub searches: u64,
    pub chats: u64,
    pub errors: u64,
    pub llm_tokens: u64,
    pub llm_rate: f64,
    pub latency: Vec<LatencyRowWire>,
    pub active: Vec<ActiveOpWire>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_payloads_match_the_sse_wire_format() {
        assert_eq!(serde_json::to_string(&RagStage::Reranking { candidates: 12 }).unwrap(), r#"{"stage":"reranking","candidates":12}"#);
        assert_eq!(serde_json::to_string(&RagStage::Searching).unwrap(), r#"{"stage":"searching"}"#);
        assert_eq!(serde_json::to_string(&Stage::Embedding { done: 3, total: 9 }).unwrap(), r#"{"stage":"embedding","done":3,"total":9}"#);
        let back: Stage = serde_json::from_str(r#"{"stage":"indexing"}"#).unwrap();
        assert!(matches!(back, Stage::Indexing));
    }

    #[test]
    fn trace_events_roundtrip_with_a_flattened_kind() {
        let e = TraceEventWire {
            ts_millis: 1,
            level: TraceLevel::Info,
            span: Some("search".into()),
            message: "done".into(),
            fields: vec![("hits".into(), "4".into())],
            kind: TraceKind::SpanClosed { elapsed_ms: 12 },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""kind":"span_closed""#) && json.contains(r#""elapsed_ms":12"#), "{json}");
        let back: TraceEventWire = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.kind, TraceKind::SpanClosed { elapsed_ms: 12 }));
        assert_eq!(back.level, TraceLevel::Info);
    }
}
