use crate::api::{error::ApiError, state::AppState};
use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;

/// Live tracing events as Server-Sent Events (event name `trace`, data = a `TraceEventWire`).
///
/// Events can contain question text, so protect the server with `API_TOKEN` when it is reachable by
/// anyone else, or switch this off with `API_EVENTS=false`. A slow reader simply misses events.
#[utoipa::path(
    get, path = "/events",
    responses(
        (status = 200, description = "text/event-stream of trace events", content_type = "text/event-stream", body = String),
        (status = 404, description = "Event streaming is disabled")
    )
)]
pub async fn events(State(st): State<AppState>) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let hub = st
        .hub
        .as_ref()
        .filter(|_| st.ctx.cfg.events_enabled)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "event streaming is disabled (API_EVENTS=false)".into()))?;
    let rx = hub.subscribe();
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let event = Event::default().event("trace").json_data(ev.to_wire()).unwrap_or_default();
                    return Some((Ok::<Event, Infallible>(event), rx));
                }
                Err(RecvError::Lagged(_)) => continue, // missed some: carry on with the newest
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
