//! Optional bearer-token protection: with `API_TOKEN` set, every route except the liveness check
//! and the API docs needs `Authorization: Bearer <token>`.

use crate::api::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Comparison whose time does not depend on where the first mismatch is.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn is_open(path: &str) -> bool {
    matches!(path, "/" | "/health" | "/openapi.json" | "/docs") || path.starts_with("/docs/")
}

pub async fn require_token(State(st): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let Some(token) = st.ctx.cfg.api_token.as_deref() else { return next.run(req).await };
    if is_open(req.uri().path()) {
        return next.run(req).await;
    }
    let supplied = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if supplied.map(|t| ct_eq(t.as_bytes(), token.as_bytes())).unwrap_or(false) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], Json(json!({ "error": "missing or invalid API token" }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(ct_eq(b"secret", b"secret"));
        assert!(!ct_eq(b"secret", b"secreT"));
        assert!(!ct_eq(b"secret", b"secret2"));
        assert!(!ct_eq(b"", b"x"));
    }

    #[test]
    fn docs_and_liveness_stay_open() {
        for p in ["/", "/health", "/openapi.json", "/docs", "/docs/index.html"] {
            assert!(is_open(p), "{p}");
        }
        for p in ["/chat", "/events", "/documents", "/docsx", "/docs-private"] {
            assert!(!is_open(p), "{p} must need the token");
        }
    }
}
