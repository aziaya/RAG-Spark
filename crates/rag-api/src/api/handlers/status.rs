use crate::api::state::AppState;
use axum::{extract::State, Json};
use rag_core::services::health::run_checks;
use rag_types::health::Check;

/// Readiness of every dependency: Weaviate, the embedding, chat and rerank servers, the tokenizer
/// file and ONNX Runtime. (`GET /health` is only a liveness probe.)
#[utoipa::path(get, path = "/status", responses((status = 200, body = Vec<Check>)))]
pub async fn status(State(st): State<AppState>) -> Json<Vec<Check>> {
    Json(run_checks(&st.ctx).await)
}
