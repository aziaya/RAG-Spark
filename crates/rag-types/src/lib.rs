//! Types and small helpers shared by the RAG server (`rag-core`, `rag-api`) and its clients
//! (`rag-tui`). Deliberately light: no networking, no heavy dependencies.

pub mod chat;
pub mod collection;
pub mod document;
pub mod health;
pub mod search;
pub mod sse;
pub mod text;
pub mod wire;
