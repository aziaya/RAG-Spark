//! The RAG engine: configuration, services (extraction, embeddings, Weaviate, retrieval,
//! reranking, chat), and observability. Used by `rag-api`; clients talk to the API instead.

pub mod config;
pub mod context;
pub mod observability;
pub mod services;
pub mod util;
