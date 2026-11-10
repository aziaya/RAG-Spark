//! rag-tui: a terminal client for the RAG server. It holds no models, no vector database and no
//! document-parsing code; everything goes through the server's HTTP API.

mod app;
mod client;
mod input;
mod markdown;
mod obs;
mod run;
mod screens;
mod sessions;
mod theme;
mod ui;
mod widgets;

use anyhow::Result;
use clap::Parser;
use client::Client;
use sessions::SessionStore;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rag-tui", version, about = "Terminal client for the RAG server: chat, search, ingest and live observability")]
struct Args {
    /// Base URL of the rag-server API
    #[arg(long, env = "RAG_SERVER_URL", default_value = "http://127.0.0.1:3000")]
    server: String,
    /// API token, if the server was started with API_TOKEN
    #[arg(long, env = "RAG_API_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// Where saved conversations live (default: ~/.rag-tui)
    #[arg(long, env = "RAG_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".rag-tui"))
        .unwrap_or_else(|| PathBuf::from("rag-tui-data"))
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        if !e.not_found() {
            eprintln!("warning: .env was not fully loaded: {e}");
        }
    }
    let args = Args::parse();
    let client = Client::new(&args.server, args.token.filter(|t| !t.is_empty()))?;
    let cfg = match client.config().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Cannot use the server at {}:\n  {e:#}\n\nStart it with `rag-server`, or point this client at it with --server URL (RAG_SERVER_URL),\nand add --token (RAG_API_TOKEN) if it needs one.",
                client.base_url()
            );
            std::process::exit(1);
        }
    };
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    run::run(client, cfg, SessionStore::new(&data_dir)).await
}
