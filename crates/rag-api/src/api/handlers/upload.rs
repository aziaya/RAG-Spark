//! Multipart upload parsing shared by `POST /ingest` and `POST /ingest/stream`.

use crate::api::error::ApiError;
use axum::{body::Bytes, extract::Multipart};
use rag_core::config::Config;
use rag_types::collection::sanitize_collection;
use std::path::Path;

pub struct Upload {
    /// File name only (used for the temporary copy, so the extension survives)
    pub name: String,
    pub data: Bytes,
    pub collection: String,
    /// Stable id of the document inside its collection
    pub source: String,
}

/// A client-supplied document id, made safe: forward slashes only, no empty / `.` parts, and
/// `..` anywhere falls back to the plain file name. Capped at 500 characters.
pub fn clean_source_id(raw: &str, fallback: &str) -> String {
    let parts: Vec<&str> = raw.trim().split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() || parts.contains(&"..") {
        return fallback.to_string();
    }
    parts.join("/").chars().take(500).collect()
}

/// Read the `file` (required), `collection` and `source` fields, in any order.
pub async fn read_upload(mp: &mut Multipart, cfg: &Config) -> Result<Upload, ApiError> {
    let (mut file, mut collection, mut source) = (None::<(String, Bytes)>, None::<String>, None::<String>);
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::bad_request(e.to_string()))? {
        match field.name() {
            Some("collection") => collection = Some(field.text().await.map_err(|e| ApiError::bad_request(e.to_string()))?),
            Some("source") => source = Some(field.text().await.map_err(|e| ApiError::bad_request(e.to_string()))?),
            Some("file") => {
                // Never trust the client's path: keep only the final component.
                let name = field
                    .file_name()
                    .and_then(|n| Path::new(n).file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from)
                    .ok_or_else(|| ApiError::bad_request("file needs a filename"))?;
                let data = field.bytes().await.map_err(|e| ApiError::bad_request(e.to_string()))?;
                file = Some((name, data));
            }
            _ => {}
        }
    }
    let (name, data) = file.ok_or_else(|| ApiError::bad_request("missing file field"))?;
    let default = &cfg.default_collection;
    let collection = collection.map(|c| sanitize_collection(&c, default)).unwrap_or_else(|| default.clone());
    let source = source.map(|s| clean_source_id(&s, &name)).unwrap_or_else(|| name.clone());
    Ok(Upload { name, data, collection, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_are_made_safe() {
        assert_eq!(clean_source_id("docs/sub/a.txt", "a.txt"), "docs/sub/a.txt");
        assert_eq!(clean_source_id("docs\\sub\\a.txt", "a.txt"), "docs/sub/a.txt", "Windows separators become /");
        assert_eq!(clean_source_id("/abs/./a.txt", "a.txt"), "abs/a.txt");
        assert_eq!(clean_source_id("../../etc/passwd", "a.txt"), "a.txt", "traversal falls back to the file name");
        assert_eq!(clean_source_id("   ", "a.txt"), "a.txt");
        assert_eq!(clean_source_id(&"x".repeat(900), "a").chars().count(), 500);
    }
}
