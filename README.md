# RAG Spark

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.92%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Cargo workspace](https://img.shields.io/badge/cargo-workspace-informational?logo=rust)](Cargo.toml)
[![Sponsor](https://img.shields.io/github/sponsors/aziaya?logo=githubsponsors&label=sponsor)](https://github.com/sponsors/aziaya)

Blazing-fast, high-performance RAG pipeline in Rust — **HTTP API + terminal UI + CLI**, split into a Cargo workspace.

Ingestion, hybrid retrieval and chat over your documents:
**docling.rs** (extract + token-aware chunking) → **EmbeddingGemma 2** (OpenAI-compatible endpoint) → **Weaviate** (bring-your-own vectors) → answers from a local chat model (llama-server).

```
file -> sha256 -> unchanged? skip
     -> docling.rs convert -> HybridChunker (tokenizer.json of the embedding model)
     -> embed (heading path + chunk) -> replace the document's chunks -> Weaviate
question -> (rewrite follow-up using the conversation) -> hybrid | semantic | keyword search
         -> (rerank candidates, blend with retrieval score) -> top-k passages -> chat model -> answer + sources
```

<details>
<summary><strong>Table of contents</strong></summary>

- [Workspace layout](#workspace-layout)
- [Interfaces](#interfaces)
- [Setup](#setup)
  - [Model servers](#model-servers-example-llama-server-invocations)
- [Build & run](#build--run)
- [The terminal UI (`rag-tui`)](#the-terminal-ui-rag-tui)
- [Collections](#collections)
- [Chat: follow-ups, memory, retrieval controls](#chat-follow-ups-memory-retrieval-controls)
- [API](#api)
- [Notes](#notes)
- [Sponsor](#sponsor)

</details>

## Workspace layout

| crate | what it is |
|---|---|
| `crates/rag-types` | shared types, SSE reader, text/path helpers, wire types |
| `crates/rag-core` | services (extraction, embeddings, Weaviate, retrieval, reranking, chat), config, tracing hub, metrics |
| `crates/rag-api` | axum HTTP server (binary `rag-server`) + admin CLI (`ingest`, `search`, `chat`, `classic`) |
| `crates/rag-tui` | standalone terminal client (binary `rag-tui`) — talks to `rag-server` over HTTP/SSE only; no models, no vector DB, no parsing code of its own |

`rag-api` and `rag-tui` are separate binaries now: run `rag-server` for the API and point `rag-tui` at it (locally or over the network).

## Interfaces

| Interface | How to use | Best for |
|-----------|------------|----------|
| **HTTP API** | `cargo run -p rag-api` (or `rag-server`), Swagger UI at `/docs` | Integration with other services, frontends, agents |
| **Terminal UI** | `cargo run -p rag-tui` (or `rag-tui`) against a running server | Day-to-day chat / ingest / search / status, live metrics |
| **Admin CLI** | `rag-server ingest|search|chat|classic` | Scripting, one-off ingest/search/chat, automation (talks to the services directly, no running server needed) |

Both the API and the admin CLI run against the same `AppContext` (same Weaviate index, same embedding + chat models); `rag-tui` is purely an HTTP client of `rag-server`.

## Setup

1. Rust **1.92+** (needed for `rag-api` with the `docling` feature; `rag-tui` alone builds on older toolchains).
2. `docker compose up -d` (Weaviate on :8080). Bump `WEAVIATE_VERSION` to a current release.
3. Serve the embedding model and a chat model with llama-server (or any OpenAI-compatible server) and set
   `EMBED_*` / `CHAT_*` in `.env`.
4. Download `tokenizer.json` from the **google/embeddinggemma-2** repo and set `CHUNK_TOKENIZER` to its absolute path.
5. `cp example.env .env`. Values containing spaces or `|` **must be quoted** (an unquoted line stops `.env` loading silently).
6. For PDFs and images export `ORT_DYLIB_PATH=/path/to/libonnxruntime.dylib` (docling uses `ort-load-dynamic`).

### Model servers (example `llama-server` invocations)

Embedding model (`EMBED_BASE_URL=http://127.0.0.1:8081/v1`):

```
llama-server \
  -m embeddinggemma-2-Q8_0.gguf \
  --embeddings \
  -c 8192 \
  -b 2048 \
  --port 8081 \
  -ngl 0
```

Chat model (`CHAT_BASE_URL=http://127.0.0.1:8082/v1`):

```
llama-server \
  -hf unsloth/gemma-4-e2b-it-GGUF:Q4_K_M \
  --device none \
  -c 8192 -b 512 --port 8082
```

Reranker, optional (`RERANK_BASE_URL=http://127.0.0.1:8083/v1`, `RERANK_ENABLED=true`):

```
llama-server \
  -hf "sikreutz/bge-reranker-v2-m3-Q4_K_M-GGUF" \
  --port 8083 \
  --ctx-size 8192 \
  --reranking \
  --n-gpu-layers 0
```

## Build & run

```
cargo build -p rag-api --no-default-features   # server without docling (txt/md/csv only)
cargo build -p rag-api                          # server with docling (needs Rust 1.92+ and ONNX Runtime at run time)
cargo build --release -p rag-tui                # standalone rag-tui executable (target/release/rag-tui)
cargo test  -p rag-types -p rag-core -p rag-api
```

```
cargo run -p rag-api                             # HTTP API, Swagger UI at /docs (default bind 127.0.0.1:3000)
cargo run -p rag-api -- classic                  # single-pane prompt-and-menu admin UI
cargo run -p rag-api -- ingest ./docs            # ingest a file or folder
cargo run -p rag-api -- search "question"
cargo run -p rag-api -- chat "question"          # omit the question for interactive chat
cargo run -p rag-tui                             # terminal client; needs rag-server running
```

Run server + TUI together in one shot:

```
cargo run -p rag-api & cargo run -p rag-tui
```

`rag-tui` takes `--server <url>` / `RAG_SERVER_URL` (default `http://127.0.0.1:3000`), `--token` / `RAG_API_TOKEN` if the
server was started with `API_TOKEN`, and `--data-dir` / `RAG_DATA_DIR` for where it saves conversations (default `~/.rag-tui`).

## The terminal UI (`rag-tui`)

The active screen (Chat, Ingest, Search, Docs, Status) is on the left; observability is on the right
(or underneath on narrow terminals) and updates live, streamed from the server's `/events` and `/metrics`, while a
question or an ingestion runs:

- **In flight**: every running operation (`ingest_file`, `extract`, `embed_batch`, `weaviate_write`, `search`,
  `llm_generate`) with its elapsed time.
- **Metrics**: files indexed / skipped / failed, chunks, embeddings, searches, chats, errors, LLM tokens and tok/s, and
  last / average / p95 latency per operation with a small trend chart.
- **Trace**: a timestamped, color-coded log of tracing events and span timings.

Keys: `Esc` leaves the text field (NAV mode), `i`/`Enter` types again, `1`-`5` / `h` `l` switch screens, `Tab` focuses
the trace pane, `v` verbosity (INFO/DEBUG/TRACE), `p` pause, `X` clear, `L` layout (right / bottom / hidden),
`?` help, `q` or `Ctrl-C` quit. Chat has slash-commands (`/help`). Set `RAG_LOG_FILE` to also write the trace to a file.

### Ingest screen

![Ingest screen](docs/screenshots/tui-ingest.png)

### Search screen

![Search screen](docs/screenshots/tui-search.png)

### Docs screen

![Docs screen](docs/screenshots/tui-docs.png)

### Status screen

![Status screen](docs/screenshots/tui-status.png)

### Chat screen

![Chat screen](docs/screenshots/tui-chat.png)

![Chat with sources expanded](docs/screenshots/tui-chat-source.png)

## Collections

A collection is a named group of documents (e.g. `legal`, `hr`). Every chunk stores its collection, and a document is
identified by *(collection, source)*, so the same file can live in two collections without clashing.

- **Ingest into a collection**: TUI Ingest screen (`Ctrl-K` picks an existing collection or creates one), CLI
  `rag-server ingest ./docs --collection legal`, API `POST /ingest` with a `collection` form field. No name = `DEFAULT_COLLECTION`.
- **Search and chat cover all collections by default.** Narrow them with `/collection` (picker, or `/collection a,b`) in
  chat, `Ctrl-K` on the Search screen, `--collection a,b` on the CLI, or `collection` / `collections` in API requests.
  Names are normalised (`Legal Docs` becomes `legal-docs`).
- **Browse**: the Docs screen lists documents with their collection (`f` filters, `d` deletes a document, `D` a whole
  collection). `Enter` opens the **chunk viewer** (paged, in document order; `Enter` reads a chunk, `n`/`p` page).
  API: `GET /collections`, `GET /documents`, `GET /documents/chunks?collection=&source=&limit=&offset=`.

## Chat: follow-ups, memory, retrieval controls

- **Follow-ups**: with history, the question is first rewritten into a standalone search query ("does it need a GPU?"
  becomes "kokoro GPU requirements"); the answer still sees your original wording. The chat shows what was searched for.
  This costs one short extra model call per follow-up (`/rewrite off` or `CHAT_REWRITE=false` disables it).
- **Memory**: the last turns are kept word for word — the model sees up to the last 8 messages (each cut at 2,000
  characters); the follow-up rewrite step sees the last 4, cut at 300 characters. Conversations are saved automatically
  by `rag-tui` under `DATA_DIR/sessions`. `/new`, `/sessions` (resume), `/forget` (delete), `/memory` (inspect).
- **Retrieval modes**: `hybrid` (BM25 + vector, blended by alpha), `semantic` (vector only, cosine), `keyword` (BM25 only).
- **Reranking**: fetch `candidates` passages, have a cross-encoder score them, blend with the retrieval score
  (`rerank weight` 0 = retrieval order, 1 = reranker only; both scores are normalised first), keep the best `top-k`.
  If the reranker is down the answer still works and a warning is shown.
- **Streaming**: answers appear token by token with a live cursor and tokens/s readout; `Ctrl-X` (or `x` in navigate mode,
  `/cancel`) stops a running answer, which closes the connection so the model stops generating, and keeps the partial
  text out of memory. `/stream on|off`, a row in `/settings`, or `CHAT_STREAM=false` turns it off. `POST /chat/stream`
  streams Server-Sent Events: `stage`, then one `token` event per piece of text, then `done` (same body as `POST /chat`)
  or `error`. Closing the connection stops generation. The trace pane shows time to first token (`first_token`)
  alongside the other timings.
- **Controls**: `/settings` opens an interactive panel (↑↓ row, ←→ change); or `/mode`, `/alpha`, `/topk`, `/candidates`,
  `/rerank on|off`, `/weight`, `/rewrite on|off`. The same options are accepted by `POST /chat` and `POST /search`.

Running a reranker locally: `llama-server -m <reranker>.gguf --reranking --embedding --pooling rank --port 8083`, then set
`RERANK_BASE_URL=http://127.0.0.1:8083/v1` and `RERANK_ENABLED=true`. Reranker quality through llama-server varies by model,
so compare results on your own documents.

## API

Auth: set `API_TOKEN` to require `Authorization: Bearer <token>` on every route except `/health` and the docs.

- `GET /health`, `GET /status`, `GET /config`, `GET /metrics`, `GET /events` (live trace, SSE)
- `POST /ingest` (multipart `file` + `collection`), `POST /ingest/stream` (same, as stage events)
- `POST /search`, `POST /chat`, `POST /chat/stream` (SSE)
- `POST /memory/compact`
- `GET /collections`, `DELETE /collections/{name}`
- `GET /documents`, `DELETE /documents`, `GET /documents/chunks?collection=&source=&limit=&offset=`
- Swagger UI at `/docs`, OpenAPI spec at `/openapi.json`

`/search` and `/chat` accept `mode`, `alpha`, `top_k`, `rerank`, `rerank_weight`, `candidates`, `source`,
`collection`/`collections`; `/chat` also takes `history`, `summary` and `rewrite`, and returns `searched_for` and `warnings`.

![Swagger UI overview](docs/screenshots/swagger-overview.png)

![Chat endpoint in Swagger](docs/screenshots/swagger-chat.png)

## Notes

- Weaviate is queried over REST + GraphQL (there is no official Rust client).
- Documents ingested from the CLI are identified by their full path; uploads through `/ingest` by filename (always
  together with their collection).
- Ingestion embeds before it deletes a document's old chunks, but delete + insert is not atomic.
- Without the `docling` feature only `.txt`, `.md` and `.csv` can be read.

## Sponsor

If RAG Spark is useful to you, consider [sponsoring the project](https://github.com/sponsors/aziaya) — it helps keep it maintained and growing.
