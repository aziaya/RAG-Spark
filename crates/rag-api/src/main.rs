mod api;
mod cli;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use rag_core::{config::Config, context::AppContext, observability};

#[tokio::main]
async fn main() -> Result<()> {
    // A missing .env is fine; a malformed one must not fail silently (it would drop every
    // variable below the bad line).
    if let Err(e) = dotenvy::dotenv() {
        if !e.not_found() {
            eprintln!("warning: .env was not fully loaded: {e}");
        }
    }
    let cli = Cli::parse();
    let hub = observability::init(cli.log_mode());
    let ctx = AppContext::build(Config::from_env()?)?;

    if let Err(e) = ctx.store.ensure_schema().await {
        if cli.lenient_startup() {
            tracing::warn!("Weaviate not ready at startup ({e:#}); will retry when needed");
        } else {
            return Err(e);
        }
    }
    cli::run(cli, ctx, hub).await
}
