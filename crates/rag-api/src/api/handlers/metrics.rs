use axum::Json;
use rag_core::observability::metrics::metrics;
use rag_types::wire::MetricsSnapshot;

/// Counters, per-operation latencies and the operations running right now.
#[utoipa::path(get, path = "/metrics", responses((status = 200, body = MetricsSnapshot)))]
pub async fn metrics_snapshot() -> Json<MetricsSnapshot> {
    Json(metrics().wire_snapshot())
}
