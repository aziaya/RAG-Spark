//! Logging, tracing and metrics.
//!
//! * `Plain`: human-readable logs on stdout (one-shot CLI commands).
//! * `Server`: stdout logs plus a [`TraceHub`] that the API streams to clients (`GET /events`).

pub mod events;
pub mod layer;
pub mod metrics;

use events::TraceEvent;
use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub enum LogMode {
    /// stdout logs at the given default level ("info", "warn", ...)
    Plain(&'static str),
    /// stdout logs plus a broadcast of every event for live clients
    Server(&'static str),
}

/// Fan-out of tracing events to any number of subscribers (slow ones simply miss events).
#[derive(Clone)]
pub struct TraceHub {
    tx: broadcast::Sender<TraceEvent>,
}

impl TraceHub {
    pub fn subscribe(&self) -> broadcast::Receiver<TraceEvent> {
        self.tx.subscribe()
    }
}

/// Install the global subscriber. Returns the hub in `Server` mode.
pub fn init(mode: LogMode) -> Option<TraceHub> {
    match mode {
        LogMode::Plain(level) => {
            let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into());
            tracing_subscriber::fmt().with_env_filter(filter).init();
            None
        }
        LogMode::Server(level) => {
            let (tx, _) = broadcast::channel(2048);
            let hub = TraceHub { tx: tx.clone() };
            // Our own crates at full detail for clients, dependencies only when something is wrong.
            let trace_filter = EnvFilter::try_from_env("RAG_TRACE").unwrap_or_else(|_| EnvFilter::new("rag_core=trace,rag_api=trace,warn"));
            let stdout_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into());
            let ui = layer::UiLayer::new(move |e| {
                // No subscribers is the normal case: ignore the error.
                let _ = tx.send(e);
            })
            .with_filter(trace_filter);
            tracing_subscriber::registry()
                .with(ui)
                .with(tracing_subscriber::fmt::layer().with_filter(stdout_filter))
                .init();
            Some(hub)
        }
    }
}
