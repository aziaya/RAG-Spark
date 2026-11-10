use rag_core::{context::AppContext, observability::TraceHub};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub ctx: AppContext,
    /// Live trace broadcast (None when running a one-shot command)
    pub hub: Option<TraceHub>,
    pub ingest_permits: Arc<Semaphore>,
    pub chat_permits: Arc<Semaphore>,
}

impl AppState {
    pub fn new(ctx: AppContext, hub: Option<TraceHub>) -> Self {
        Self {
            ingest_permits: Arc::new(Semaphore::new(ctx.cfg.max_concurrent_ingests.max(1))),
            chat_permits: Arc::new(Semaphore::new(ctx.cfg.chat_max_concurrent.max(1))),
            hub,
            ctx,
        }
    }
}
