//! Typed HTTP/SSE client for the rag-server API. The terminal UI talks to the server only through
//! this module.

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use rag_types::{
    chat::{ChatAnswer, ChatMemory, RagStage},
    collection::{ChunkPage, CollectionInfo, DocumentInfo},
    document::{IngestOutcome, Stage},
    health::Check,
    search::{Hit, RetrievalOptions},
    sse::{SseEvent, SseReader},
    wire::{ChatEvent, CompactRequest, CompactResponse, MetricsSnapshot, ServerConfig, TraceEventWire},
};
use reqwest::{multipart, Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

/// Merge the retrieval options into a request body (the server accepts them as flat fields).
fn with_options(mut body: Value, o: &RetrievalOptions) -> Result<Value> {
    if let (Value::Object(m), Value::Object(opts)) = (&mut body, serde_json::to_value(o)?) {
        m.extend(opts);
    }
    Ok(body)
}

/// Pull `{"error": "..."}` out of an error body, falling back to the raw text.
fn error_text(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| body.trim().to_string())
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?;
        Ok(Self { http, base: base.trim_end_matches('/').to_string(), token })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn req(&self, method: Method, path: &str) -> RequestBuilder {
        let r = self.http.request(method, format!("{}{}", self.base, path));
        match &self.token {
            Some(t) => r.bearer_auth(t),
            None => r,
        }
    }

    /// Turn a non-2xx response into an error carrying the server's message.
    async fn ok(resp: Response) -> Result<Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow!("401 unauthorized: the server needs an API token (--token / RAG_API_TOKEN)"));
        }
        Err(anyhow!("{status}: {}", error_text(&body)))
    }

    async fn send(&self, rb: RequestBuilder, what: &str) -> Result<Response> {
        let resp = rb.send().await.with_context(|| format!("{what}: cannot reach {}", self.base))?;
        Self::ok(resp).await
    }

    async fn json<T: DeserializeOwned>(&self, rb: RequestBuilder, what: &str) -> Result<T> {
        self.send(rb, what).await?.json::<T>().await.with_context(|| format!("{what}: unexpected response"))
    }

    pub async fn config(&self) -> Result<ServerConfig> {
        self.json(self.req(Method::GET, "/config").timeout(Duration::from_secs(5)), "GET /config").await
    }

    pub async fn status(&self) -> Result<Vec<Check>> {
        self.json(self.req(Method::GET, "/status").timeout(Duration::from_secs(30)), "GET /status").await
    }

    pub async fn metrics(&self) -> Result<MetricsSnapshot> {
        self.json(self.req(Method::GET, "/metrics").timeout(Duration::from_secs(3)), "GET /metrics").await
    }

    pub async fn collections(&self) -> Result<Vec<CollectionInfo>> {
        self.json(self.req(Method::GET, "/collections").timeout(Duration::from_secs(30)), "GET /collections").await
    }

    pub async fn documents(&self, collection: Option<&str>) -> Result<Vec<DocumentInfo>> {
        let mut rb = self.req(Method::GET, "/documents").timeout(Duration::from_secs(30));
        if let Some(c) = collection {
            rb = rb.query(&[("collection", c)]);
        }
        self.json(rb, "GET /documents").await
    }

    pub async fn chunks(&self, collection: &str, source: &str, limit: usize, offset: usize) -> Result<ChunkPage> {
        let rb = self
            .req(Method::GET, "/documents/chunks")
            .timeout(Duration::from_secs(30))
            .query(&[("collection", collection), ("source", source)])
            .query(&[("limit", limit), ("offset", offset)]);
        self.json(rb, "GET /documents/chunks").await
    }

    pub async fn delete_document(&self, collection: &str, source: &str) -> Result<()> {
        let rb = self.req(Method::DELETE, "/documents").timeout(Duration::from_secs(60)).query(&[("collection", collection), ("source", source)]);
        self.send(rb, "DELETE /documents").await.map(|_| ())
    }

    pub async fn delete_collection(&self, name: &str) -> Result<()> {
        let rb = self.req(Method::DELETE, &format!("/collections/{name}")).timeout(Duration::from_secs(120));
        self.send(rb, "DELETE /collections").await.map(|_| ())
    }

    pub async fn search(&self, query: &str, o: &RetrievalOptions) -> Result<Vec<Hit>> {
        let body = with_options(json!({ "query": query }), o)?;
        self.json(self.req(Method::POST, "/search").timeout(Duration::from_secs(120)).json(&body), "POST /search").await
    }

    /// Fold old turns into a summary (the caller applies the result to its own memory).
    pub async fn compact(&self, memory: &ChatMemory) -> Result<CompactResponse> {
        let rb = self.req(Method::POST, "/memory/compact").timeout(Duration::from_secs(600)).json(&CompactRequest { memory: memory.clone() });
        self.json(rb, "POST /memory/compact").await
    }

    fn chat_body(question: &str, memory: &ChatMemory, o: &RetrievalOptions, rewrite: bool) -> Result<Value> {
        with_options(json!({ "question": question, "history": memory.turns, "summary": memory.summary, "rewrite": rewrite }), o)
    }

    /// Non-streaming answer.
    pub async fn chat(&self, question: &str, memory: &ChatMemory, o: &RetrievalOptions, rewrite: bool) -> Result<ChatAnswer> {
        let body = Self::chat_body(question, memory, o, rewrite)?;
        self.json(self.req(Method::POST, "/chat").json(&body), "POST /chat").await
    }

    /// Streaming answer: `on_event` receives stages and tokens as they arrive; the full answer is
    /// returned at the end. Dropping the returned future closes the connection, which stops
    /// generation on the server.
    pub async fn chat_stream(
        &self,
        question: &str,
        memory: &ChatMemory,
        o: &RetrievalOptions,
        rewrite: bool,
        mut on_event: impl FnMut(ChatEvent),
    ) -> Result<ChatAnswer> {
        let body = Self::chat_body(question, memory, o, rewrite)?;
        let resp = self.send(self.req(Method::POST, "/chat/stream").json(&body), "POST /chat/stream").await?;
        let (mut answer, mut failure) = (None::<ChatAnswer>, None::<String>);
        read_sse(resp, |ev| {
            match ev.event.as_deref() {
                Some("stage") => {
                    if let Ok(s) = serde_json::from_str::<RagStage>(&ev.data) {
                        on_event(ChatEvent::Stage(s));
                    }
                }
                Some("token") => {
                    if let Some(t) = serde_json::from_str::<Value>(&ev.data).ok().and_then(|v| v["text"].as_str().map(String::from)) {
                        on_event(ChatEvent::Token(t));
                    }
                }
                Some("done") => {
                    answer = serde_json::from_str(&ev.data).ok();
                    return false;
                }
                Some("error") => {
                    failure = Some(error_text(&ev.data));
                    return false;
                }
                _ => {}
            }
            true
        })
        .await?;
        match (answer, failure) {
            (Some(a), _) => Ok(a),
            (None, Some(e)) => Err(anyhow!(e)),
            (None, None) => Err(anyhow!("the server closed the stream before the answer was complete")),
        }
    }

    /// Upload one file and follow its progress. `source` is the document id inside `collection`.
    pub async fn ingest_file(&self, file: &Path, source: &str, collection: &str, max_mb: usize, mut on_stage: impl FnMut(Stage)) -> Result<IngestOutcome> {
        let meta = tokio::fs::metadata(file).await.with_context(|| format!("cannot read {}", file.display()))?;
        if meta.len() > (max_mb as u64) * 1024 * 1024 {
            return Err(anyhow!("file is {:.1} MB; the server accepts up to {max_mb} MB", meta.len() as f64 / 1_048_576.0));
        }
        let bytes = tokio::fs::read(file).await.with_context(|| format!("cannot read {}", file.display()))?;
        let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "upload".into());
        let form = multipart::Form::new()
            .text("collection", collection.to_string())
            .text("source", source.to_string())
            .part("file", multipart::Part::bytes(bytes).file_name(name));
        let resp = self.send(self.req(Method::POST, "/ingest/stream").multipart(form), "POST /ingest/stream").await?;
        let (mut outcome, mut failure) = (None::<IngestOutcome>, None::<String>);
        read_sse(resp, |ev| {
            match ev.event.as_deref() {
                Some("stage") => {
                    if let Ok(s) = serde_json::from_str::<Stage>(&ev.data) {
                        on_stage(s);
                    }
                }
                Some("done") => {
                    outcome = serde_json::from_str(&ev.data).ok();
                    return false;
                }
                Some("error") => {
                    failure = Some(error_text(&ev.data));
                    return false;
                }
                _ => {}
            }
            true
        })
        .await?;
        match (outcome, failure) {
            (Some(o), _) => Ok(o),
            (None, Some(e)) => Err(anyhow!(e)),
            (None, None) => Err(anyhow!("the server closed the stream before the file was finished")),
        }
    }

    /// Follow the server's live trace events until the connection ends.
    pub async fn stream_events(&self, mut on_event: impl FnMut(TraceEventWire)) -> Result<()> {
        let resp = self.send(self.req(Method::GET, "/events"), "GET /events").await?;
        read_sse(resp, |ev| {
            if ev.event.as_deref() == Some("trace") {
                if let Ok(e) = serde_json::from_str::<TraceEventWire>(&ev.data) {
                    on_event(e);
                }
            }
            true
        })
        .await
    }
}

/// Feed a streaming response through the SSE reader; `on` returns false to stop reading.
async fn read_sse(resp: Response, mut on: impl FnMut(SseEvent) -> bool) -> Result<()> {
    let mut reader = SseReader::default();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("the connection to the server was interrupted")?;
        for ev in reader.push(&bytes) {
            if !on(ev) {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rag_types::{chat::ChatTurn, search::SearchMode};
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// One-shot HTTP server: records the request and answers with the given head and body slices
    /// (written with pauses, so the client really sees a stream).
    async fn serve(status: &str, content_type: &str, slices: Vec<Vec<u8>>) -> (String, Arc<Mutex<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (seen2, head) = (seen.clone(), format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n"));
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1 << 16];
            let mut got = 0;
            loop {
                let n = sock.read(&mut buf[got..]).await.unwrap();
                got += n;
                let text = String::from_utf8_lossy(&buf[..got]).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)));
                    let chunked = text[..end].to_lowercase().contains("transfer-encoding: chunked");
                    let done = match len {
                        Some(l) => got >= end + 4 + l,
                        None => !chunked || text.ends_with("0\r\n\r\n"),
                    };
                    if done {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            *seen2.lock().unwrap() = buf[..got].to_vec();
            sock.write_all(head.as_bytes()).await.unwrap();
            for s in slices {
                if sock.write_all(&s).await.is_err() {
                    return;
                }
                let _ = sock.flush().await;
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        });
        (url, seen)
    }

    fn opts() -> RetrievalOptions {
        RetrievalOptions { mode: SearchMode::Semantic, alpha: 0.5, top_k: 3, rerank: true, rerank_weight: 0.6, candidates: 12, source: None, collections: vec!["legal".into()] }
    }

    #[test]
    fn options_become_flat_request_fields() {
        let v = with_options(json!({ "query": "q" }), &opts()).unwrap();
        assert_eq!(v["query"], "q");
        assert_eq!((v["mode"].as_str(), v["top_k"].as_u64(), v["rerank"].as_bool()), (Some("semantic"), Some(3), Some(true)));
        assert_eq!(v["collections"], json!(["legal"]));
        assert_eq!(error_text(r#"{"error":"boom"}"#), "boom");
        assert_eq!(error_text("plain text\n"), "plain text");
    }

    #[tokio::test]
    async fn sends_the_bearer_token_and_decodes_json() {
        let cfg = json!({ "version": "9", "retrieval": opts(), "rewrite": true, "stream": true, "default_collection": "default", "embed_model": "e", "chat_model": "c",
            "reranker_configured": false, "max_upload_mb": 50, "events_enabled": true, "auth_required": true });
        let body = cfg.to_string().into_bytes();
        let (url, seen) = serve("200 OK", &format!("application/json\r\nContent-Length: {}", body.len()), vec![body]).await;
        let c = Client::new(&url, Some("s3cret".into())).unwrap();
        let got = c.config().await.unwrap();
        assert_eq!((got.version.as_str(), got.max_upload_mb, got.events_enabled), ("9", 50, true));
        let req = String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        assert!(req.starts_with("GET /config"), "{req}");
        assert!(req.to_lowercase().contains("authorization: bearer s3cret"), "{req}");
    }

    #[tokio::test]
    async fn server_errors_carry_the_servers_message() {
        let body = br#"{"error":"question must be 1-4000 characters"}"#.to_vec();
        let (url, _) = serve("400 Bad Request", &format!("application/json\r\nContent-Length: {}", body.len()), vec![body]).await;
        let err = Client::new(&url, None).unwrap().search("x", &opts()).await.unwrap_err();
        assert!(err.to_string().contains("400") && err.to_string().contains("1-4000"), "{err}");
        let (url, _) = serve("401 Unauthorized", "application/json\r\nContent-Length: 2", vec![b"{}".to_vec()]).await;
        let err = Client::new(&url, None).unwrap().config().await.unwrap_err();
        assert!(err.to_string().contains("API token"), "{err}");
    }

    fn sse(event: &str, data: Value) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    #[tokio::test]
    async fn chat_stream_decodes_stages_tokens_and_the_final_answer() {
        let answer = json!({ "answer": "Kokoro runs on CPU [1].", "sources": [], "searched_for": "kokoro gpu", "warnings": [] });
        let mut body = String::new();
        body += &sse("stage", json!({ "stage": "searching" }));
        body += &sse("stage", json!({ "stage": "generating", "passages": 2 }));
        for t in ["Kokoro ", "runs ", "é "] {
            body += &sse("token", json!({ "text": t }));
        }
        body += &sse("done", answer);
        // awkward slicing: mid-line and inside the multi-byte "é"
        let bytes = body.into_bytes();
        let slices: Vec<Vec<u8>> = bytes.chunks(23).map(|c| c.to_vec()).collect();
        let (url, seen) = serve("200 OK", "text/event-stream", slices).await;
        let mut memory = ChatMemory::default();
        memory.turns.push(ChatTurn { role: "user".into(), content: "what is kokoro".into() });
        let (mut stages, mut tokens) = (vec![], vec![]);
        let got = Client::new(&url, None)
            .unwrap()
            .chat_stream("does it need a GPU?", &memory, &opts(), true, |ev| match ev {
                ChatEvent::Stage(s) => stages.push(format!("{s:?}")),
                ChatEvent::Token(t) => tokens.push(t),
                _ => {}
            })
            .await
            .unwrap();
        assert_eq!(tokens, ["Kokoro ", "runs ", "é "]);
        assert_eq!(stages.len(), 2);
        assert_eq!(got.searched_for.as_deref(), Some("kokoro gpu"));
        let req = String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        assert!(req.starts_with("POST /chat/stream"), "{req}");
        let body: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["question"], "does it need a GPU?");
        assert_eq!(body["history"][0]["content"], "what is kokoro");
        assert_eq!((body["rewrite"].as_bool(), body["mode"].as_str(), body["top_k"].as_u64()), (Some(true), Some("semantic"), Some(3)));
    }

    #[tokio::test]
    async fn a_streamed_error_event_becomes_an_error() {
        let body = sse("stage", json!({ "stage": "searching" })) + &sse("error", json!({ "error": "chat endpoint returned 503" }));
        let (url, _) = serve("200 OK", "text/event-stream", vec![body.into_bytes()]).await;
        let err = Client::new(&url, None).unwrap().chat_stream("q", &ChatMemory::default(), &opts(), false, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("503"), "{err}");
        let (url, _) = serve("200 OK", "text/event-stream", vec![b"event: token\ndata: {\"text\":\"x\"}\n\n".to_vec()]).await;
        let err = Client::new(&url, None).unwrap().chat_stream("q", &ChatMemory::default(), &opts(), false, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("closed the stream"), "{err}");
    }

    #[tokio::test]
    async fn ingest_uploads_the_file_with_its_collection_and_source_and_follows_stages() {
        let dir = std::env::temp_dir().join("rag_tui_ingest_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, "hello upload").unwrap();
        let mut body = String::new();
        body += &sse("stage", json!({ "stage": "extracting" }));
        body += &sse("stage", json!({ "stage": "embedding", "done": 1, "total": 2 }));
        body += &sse("done", json!({ "status": "indexed", "source": "docs/notes.txt", "chunks": 2 }));
        let (url, seen) = serve("200 OK", "text/event-stream", vec![body.into_bytes()]).await;
        let mut stages = vec![];
        let out = Client::new(&url, Some("t".into())).unwrap().ingest_file(&file, "docs/notes.txt", "legal", 50, |s| stages.push(s)).await.unwrap();
        assert!(matches!(out, IngestOutcome::Indexed { chunks: 2, .. }));
        assert_eq!(stages.len(), 2);
        let req = String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        assert!(req.starts_with("POST /ingest/stream"), "{req}");
        for needle in ["name=\"collection\"", "legal", "name=\"source\"", "docs/notes.txt", "filename=\"notes.txt\"", "hello upload"] {
            assert!(req.contains(needle), "multipart body is missing {needle}: {req}");
        }
        // too large: refused locally, nothing is sent
        let err = Client::new(&url, None).unwrap().ingest_file(&file, "x", "c", 0, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("accepts up to 0 MB"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn trace_events_are_decoded_and_others_ignored() {
        let ev = json!({ "ts_millis": 5, "level": "info", "span": null, "message": "m", "fields": [], "kind": "log" });
        let body = ": keep-alive\n\n".to_string() + &sse("trace", ev) + &sse("other", json!({}));
        let (url, _) = serve("200 OK", "text/event-stream", vec![body.into_bytes()]).await;
        let mut got = vec![];
        Client::new(&url, None).unwrap().stream_events(|e| got.push(e)).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].message, "m");
    }
}
