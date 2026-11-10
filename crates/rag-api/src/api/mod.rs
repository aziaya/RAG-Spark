//! HTTP API: router, bearer-token auth, OpenAPI document and Swagger UI.

pub mod auth;
pub mod dto;
pub mod error;
pub mod handlers;
pub mod state;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    response::Redirect,
    routing::{delete, get, post},
    Router,
};
use state::AppState;
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

/// Adds the "Authorize" button for `API_TOKEN` to the docs.
struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme("bearer", SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()));
    }
}

#[derive(OpenApi)]
#[openapi(
    info(title = "rag-pipeline API", description = "Docling -> embeddings -> Weaviate ingestion, hybrid search and chat, with live status, metrics and trace streams"),
    modifiers(&BearerAuth),
    paths(
        handlers::health::health,
        handlers::status::status,
        handlers::config::config,
        handlers::metrics::metrics_snapshot,
        handlers::events::events,
        handlers::ingest::ingest,
        handlers::ingest_stream::ingest_stream,
        handlers::search::search,
        handlers::chat::chat,
        handlers::chat_stream::chat_stream,
        handlers::memory::compact,
        handlers::collections::list_collections,
        handlers::collections::list_documents,
        handlers::collections::document_chunks,
        handlers::collections::delete_document,
        handlers::collections::delete_collection
    ),
    components(schemas(
        dto::SearchReq,
        dto::ChatReq,
        dto::RetrievalParams,
        dto::UploadForm,
        rag_types::search::SearchMode,
        rag_types::search::Hit,
        rag_types::search::RetrievalOptions,
        rag_types::document::IngestOutcome,
        rag_types::chat::ChatAnswer,
        rag_types::chat::SourceRef,
        rag_types::chat::ChatTurn,
        rag_types::chat::ChatMemory,
        rag_types::collection::CollectionInfo,
        rag_types::collection::DocumentInfo,
        rag_types::collection::ChunkInfo,
        rag_types::collection::ChunkPage,
        rag_types::health::Check,
        rag_types::health::Level,
        rag_types::wire::ServerConfig,
        rag_types::wire::CompactRequest,
        rag_types::wire::CompactResponse,
        rag_types::wire::MetricsSnapshot,
        rag_types::wire::LatencyRowWire,
        rag_types::wire::ActiveOpWire
    ))
)]
struct ApiDoc;

pub fn router(state: AppState) -> Router {
    let limit = state.ctx.cfg.max_upload_mb * 1024 * 1024;
    Router::new()
        .route("/", get(|| async { Redirect::to("/docs") }))
        .route("/health", get(handlers::health::health))
        .route("/status", get(handlers::status::status))
        .route("/config", get(handlers::config::config))
        .route("/metrics", get(handlers::metrics::metrics_snapshot))
        .route("/events", get(handlers::events::events))
        .route("/ingest", post(handlers::ingest::ingest))
        .route("/ingest/stream", post(handlers::ingest_stream::ingest_stream))
        .route("/search", post(handlers::search::search))
        .route("/chat", post(handlers::chat::chat))
        .route("/chat/stream", post(handlers::chat_stream::chat_stream))
        .route("/memory/compact", post(handlers::memory::compact))
        .route("/collections", get(handlers::collections::list_collections))
        .route("/collections/{name}", delete(handlers::collections::delete_collection))
        .route("/documents", get(handlers::collections::list_documents).delete(handlers::collections::delete_document))
        .route("/documents/chunks", get(handlers::collections::document_chunks))
        .layer(DefaultBodyLimit::max(limit))
        // outermost: authentication runs before anything else
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_token))
        .with_state(state)
        // Swagger UI at /docs, spec at /openapi.json (not behind the token: it documents, it exposes no data)
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
}
