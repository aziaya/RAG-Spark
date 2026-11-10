use crate::api::state::AppState;
use axum::{extract::State, Json};
use rag_types::wire::ServerConfig;

/// The server's defaults and capabilities, so a client can start with the same settings.
#[utoipa::path(get, path = "/config", responses((status = 200, body = ServerConfig)))]
pub async fn config(State(st): State<AppState>) -> Json<ServerConfig> {
    let (cfg, ctx) = (&st.ctx.cfg, &st.ctx);
    Json(ServerConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        retrieval: cfg.retrieval_defaults(),
        rewrite: cfg.chat_rewrite,
        stream: cfg.chat_stream,
        default_collection: cfg.default_collection.clone(),
        embed_model: cfg.embed_model.clone(),
        chat_model: cfg.chat_model.clone(),
        reranker_configured: ctx.reranker.configured(),
        max_upload_mb: cfg.max_upload_mb,
        events_enabled: cfg.events_enabled && st.hub.is_some(),
        auth_required: cfg.api_token.is_some(),
    })
}
