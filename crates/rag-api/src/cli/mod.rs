//! Command-line entry points. With no command the server starts.

pub mod classic;
pub mod commands;
pub mod plain_chat;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rag_core::{
    context::AppContext,
    observability::{LogMode, TraceHub},
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rag-server",
    about = "RAG server: Docling -> embeddings -> Weaviate, hybrid retrieval, reranking and chat over HTTP. Run without a command to start the API; use `rag-tui` for the terminal client."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the HTTP API (the default); Swagger UI at /docs
    Serve,
    /// Ingest a file or a folder into a collection, with progress
    Ingest {
        path: PathBuf,
        /// Collection to save into (default: DEFAULT_COLLECTION)
        #[arg(short, long)]
        collection: Option<String>,
    },
    /// Hybrid search, printed as JSON
    Search {
        query: String,
        #[arg(short, long, default_value_t = 5)]
        limit: usize,
        /// 0 = keyword only, 1 = vector only
        #[arg(short, long, default_value_t = 0.5)]
        alpha: f32,
        /// Search only these collections (comma separated; default: all)
        #[arg(short, long, value_delimiter = ',')]
        collection: Vec<String>,
    },
    /// Chat with your documents: `chat "question"` answers once, plain `chat` is interactive
    Chat {
        /// Ask one question and exit (omit for interactive mode)
        question: Option<String>,
        /// Chunks to retrieve (defaults to CHAT_TOP_K)
        #[arg(short, long)]
        limit: Option<usize>,
        /// 0 = keyword only, 1 = vector only
        #[arg(short, long, default_value_t = 0.5)]
        alpha: f32,
        /// Only search this ingested file
        #[arg(short, long)]
        source: Option<String>,
        /// Search only these collections (comma separated; default: all)
        #[arg(short, long, value_delimiter = ',')]
        collection: Vec<String>,
    },
    /// Single-pane prompt-and-menu admin UI that talks to the services directly
    Classic,
}

impl Cli {
    fn command(&self) -> &Command {
        self.command.as_ref().unwrap_or(&Command::Serve)
    }

    /// How logging should behave for this command.
    pub fn log_mode(&self) -> LogMode {
        match self.command() {
            Command::Serve => LogMode::Server("info"),
            _ => LogMode::Plain("warn"),
        }
    }

    /// Commands that should start even when Weaviate is down (they can show why).
    pub fn lenient_startup(&self) -> bool {
        matches!(self.command(), Command::Serve | Command::Classic)
    }
}

/// Normalise collection names typed on the command line.
fn names(raw: &[String]) -> Vec<String> {
    raw.iter().map(|n| rag_types::collection::sanitize_collection(n, "")).filter(|n| !n.is_empty()).collect()
}

pub async fn run(cli: Cli, ctx: AppContext, hub: Option<TraceHub>) -> Result<()> {
    use std::io::IsTerminal;
    let is_tty = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    match cli.command {
        None | Some(Command::Serve) => commands::serve(ctx, hub).await,
        Some(Command::Classic) => classic::run_menu(&ctx).await,
        Some(Command::Ingest { path, collection }) => commands::ingest(&ctx, &path, collection).await,
        Some(Command::Search { query, limit, alpha, collection }) => commands::search(&ctx, &query, limit, alpha, collection).await,
        Some(Command::Chat { question, limit, alpha, source, collection }) => {
            let limit = limit.unwrap_or(ctx.cfg.chat_top_k).clamp(1, 20);
            let alpha = alpha.clamp(0.0, 1.0);
            if question.is_none() && is_tty {
                classic::chat_session(&ctx, classic::ChatOpts { limit, alpha, source, collections: names(&collection) }).await
            } else {
                // One-shot questions and piped input use the plain, script-friendly mode.
                plain_chat::run(&ctx, question, plain_chat::Opts { limit, alpha, source, collections: names(&collection) }).await
            }
        }
    }
}
