//! Non-interactive commands.

use crate::{api, cli::classic};
use anyhow::Result;
use rag_core::{context::AppContext, observability::TraceHub};
use rag_types::{collection::sanitize_collection, text::collect_files};
use std::{net::SocketAddr, path::Path};
use tracing::{info, warn};

pub async fn ingest(ctx: &AppContext, path: &Path, collection: Option<String>) -> Result<()> {
    let default = &ctx.cfg.default_collection;
    let collection = collection.map(|c| sanitize_collection(&c, default)).unwrap_or_else(|| default.clone());
    let files = collect_files(path);
    if files.is_empty() {
        anyhow::bail!("no files found at {}", path.display());
    }
    classic::ingest_paths(&ctx.pipeline, files, &collection).await
}

pub async fn search(ctx: &AppContext, query: &str, limit: usize, alpha: f32, collections: Vec<String>) -> Result<()> {
    let mut opts = ctx.cfg.retrieval_defaults();
    opts.top_k = limit;
    opts.alpha = alpha;
    opts.collections = collections.iter().map(|n| sanitize_collection(n, "")).filter(|n| !n.is_empty()).collect();
    let hits = ctx.rag.search(query, &opts.sanitized()).await?;
    println!("{}", serde_json::to_string_pretty(&hits)?);
    Ok(())
}

/// Is this bind address reachable only from this machine?
fn is_loopback(addr: &str) -> bool {
    addr.parse::<SocketAddr>().map(|a| a.ip().is_loopback()).unwrap_or_else(|_| addr.starts_with("localhost:"))
}

pub async fn serve(ctx: AppContext, hub: Option<TraceHub>) -> Result<()> {
    let addr = ctx.cfg.bind_addr.clone();
    if ctx.cfg.api_token.is_none() && !is_loopback(&addr) {
        warn!("API_TOKEN is not set and the server listens on {addr}: anyone who can reach it can read and change your documents");
    }
    if ctx.cfg.api_token.is_some() {
        info!("bearer-token authentication is on");
    }
    let state = api::state::AppState::new(ctx, hub);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("listening on http://{addr}  (Swagger UI: /docs)");
    axum::serve(listener, api::router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("127.0.0.1:3000"));
        assert!(is_loopback("[::1]:3000"));
        assert!(is_loopback("localhost:3000"));
        assert!(!is_loopback("0.0.0.0:3000"));
        assert!(!is_loopback("192.168.1.20:3000"));
    }
}
