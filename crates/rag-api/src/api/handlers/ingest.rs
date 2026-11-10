use super::upload::read_upload;
use crate::api::{
    dto::UploadForm,
    error::{internal, ApiError},
    state::AppState,
};
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use rag_types::document::IngestOutcome;
use std::time::Duration;

/// Upload and ingest one file (multipart field `file`) into a collection (optional text field
/// `collection`; default: the server's DEFAULT_COLLECTION). An optional text field `source` sets
/// the document id (default: the file name).
#[utoipa::path(
    post, path = "/ingest",
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Indexed or skipped (unchanged)", body = IngestOutcome),
        (status = 400, description = "Missing or invalid file"),
        (status = 504, description = "Ingest timed out"),
        (status = 500, description = "Extraction, embedding or Weaviate error")
    )
)]
pub async fn ingest(State(st): State<AppState>, mut mp: Multipart) -> Result<Json<IngestOutcome>, ApiError> {
    let up = read_upload(&mut mp, &st.ctx.cfg).await?;
    st.ctx.store.ensure_schema().await.map_err(internal)?;
    let _permit = st.ingest_permits.acquire().await.map_err(internal)?;
    let dir = tempfile::tempdir().map_err(internal)?;
    let path = dir.path().join(&up.name); // keep the extension: format detection may rely on it
    tokio::fs::write(&path, &up.data).await.map_err(internal)?;

    let secs = st.ctx.cfg.ingest_timeout_secs;
    let out = tokio::time::timeout(Duration::from_secs(secs), st.ctx.pipeline.ingest_file(&path, &up.source, &up.collection))
        .await
        .map_err(|_| ApiError(StatusCode::GATEWAY_TIMEOUT, format!("ingest exceeded {secs}s")))?
        .map_err(internal)?;
    Ok(Json(out))
}
