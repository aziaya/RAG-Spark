use crate::api::{error::ApiError, state::AppState};
use axum::{extract::State, http::StatusCode, Json};
use rag_types::wire::{CompactRequest, CompactResponse};

/// Fold the older turns of a conversation into a summary. Stateless: the client sends its memory
/// and applies the result (drop the first `dropped` turns, keep `summary`). `summary` is null when
/// the memory is still short enough.
#[utoipa::path(
    post, path = "/memory/compact",
    request_body = CompactRequest,
    responses((status = 200, body = CompactResponse), (status = 502, description = "Chat model unreachable or returned nothing"))
)]
pub async fn compact(State(st): State<AppState>, Json(req): Json<CompactRequest>) -> Result<Json<CompactResponse>, ApiError> {
    match st.ctx.rag.compact_memory(&req.memory).await {
        Ok(Some(c)) => Ok(Json(CompactResponse { summary: Some(c.summary), dropped: c.dropped })),
        Ok(None) => Ok(Json(CompactResponse { summary: None, dropped: 0 })),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}"))),
    }
}
