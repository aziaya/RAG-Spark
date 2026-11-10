use super::upload::read_upload;
use crate::api::{dto::UploadForm, error::ApiError, state::AppState};
use axum::{
    extract::{Multipart, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use rag_types::document::IngestOutcome;
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::sync::mpsc::unbounded_channel;

/// Ingest one file and stream its progress as Server-Sent Events.
///
/// Same form fields as `POST /ingest`. Events: `stage` (`{"stage": "checking|extracting|embedding|indexing", ...}`),
/// then exactly one of `done` (the outcome, same body as `POST /ingest`) or `error` (`{"error": "..."}`).
/// If the client disconnects, the file still finishes so the document is never left half-written.
#[utoipa::path(
    post, path = "/ingest/stream",
    request_body(content = UploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "text/event-stream of stage, done and error events", content_type = "text/event-stream", body = String),
        (status = 400, description = "Missing or invalid file")
    )
)]
pub async fn ingest_stream(State(st): State<AppState>, mut mp: Multipart) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let up = read_upload(&mut mp, &st.ctx.cfg).await?;
    let (tx, rx) = unbounded_channel::<Event>();
    let (ctx, permits) = (st.ctx.clone(), st.ingest_permits.clone());

    tokio::spawn(async move {
        let result: Result<IngestOutcome, String> = async {
            ctx.store.ensure_schema().await.map_err(|e| format!("{e:#}"))?;
            let _permit = permits.acquire_owned().await.map_err(|e| e.to_string())?;
            let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
            let path = dir.path().join(&up.name);
            tokio::fs::write(&path, &up.data).await.map_err(|e| e.to_string())?;
            let tx_stage = tx.clone();
            let secs = ctx.cfg.ingest_timeout_secs;
            tokio::time::timeout(
                Duration::from_secs(secs),
                ctx.pipeline.ingest_file_progress(&path, &up.source, &up.collection, move |stage| {
                    let _ = tx_stage.send(Event::default().event("stage").json_data(&stage).unwrap_or_default());
                }),
            )
            .await
            .map_err(|_| format!("ingest exceeded {secs}s"))?
            .map_err(|e| format!("{e:#}"))
        }
        .await;
        let last = match result {
            Ok(outcome) => Event::default().event("done").json_data(&outcome).unwrap_or_default(),
            Err(e) => Event::default().event("error").json_data(json!({ "error": e })).unwrap_or_default(),
        };
        let _ = tx.send(last);
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|e| (Ok::<Event, Infallible>(e), rx)) });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
