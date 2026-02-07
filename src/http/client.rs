//! HTTP client for thin-client mode.
//!
//! When `--server <url>` is passed to the CLI, commands proxy through
//! a remote Bobbin HTTP server instead of opening local stores.

use anyhow::{Context, Result};
use serde::Deserialize;

/// HTTP client that proxies CLI commands to a remote Bobbin server.
pub struct Client {
    base_url: String,
    http: reqwest::Client,
}

/// Response from the /search endpoint
#[derive(Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub mode: String,
    pub count: usize,
    pub results: Vec<SearchResultItem>,
}

/// A single search result item from the server
#[derive(Deserialize)]
pub struct SearchResultItem {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub match_type: Option<String>,
    pub language: String,
    pub content_preview: String,
}

/// Response from the /status endpoint
#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    pub index: IndexStats,
}

/// Index stats from the server
#[derive(Debug, Deserialize)]
pub struct IndexStats {
    pub total_files: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub languages: Vec<LanguageStats>,
    pub last_indexed: Option<i64>,
    pub index_size_bytes: u64,
}

/// Per-language statistics
#[derive(Debug, Deserialize)]
pub struct LanguageStats {
    pub language: String,
    pub file_count: u64,
    pub chunk_count: u64,
}

/// Response from the /chunk/{id} endpoint
#[derive(Deserialize)]
pub struct ChunkResponse {
    pub id: String,
    pub file_path: String,
    pub chunk_type: String,
    pub name: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub language: String,
    pub content: String,
}

/// Error response from the server
#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

impl Client {
    /// Create a new client pointing at the given server URL.
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    /// Search via the remote server.
    pub async fn search(
        &self,
        query: &str,
        mode: &str,
        chunk_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SearchResponse> {
        let url = format!("{}/search", self.base_url);

        let mut params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("mode", mode.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(t) = chunk_type {
            params.push(("type", t.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }

        let resp = self
            .http
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to connect to bobbin server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: ErrorBody = resp.json().await.unwrap_or(ErrorBody {
                error: format!("HTTP {}", status),
            });
            anyhow::bail!("Server error ({}): {}", status, body.error);
        }

        resp.json()
            .await
            .context("Failed to parse search response")
    }

    /// Get index status from the remote server.
    pub async fn status(&self) -> Result<StatusResponse> {
        let url = format!("{}/status", self.base_url);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to connect to bobbin server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: ErrorBody = resp.json().await.unwrap_or(ErrorBody {
                error: format!("HTTP {}", status),
            });
            anyhow::bail!("Server error ({}): {}", status, body.error);
        }

        resp.json()
            .await
            .context("Failed to parse status response")
    }

    /// Get a specific chunk by ID from the remote server.
    pub async fn get_chunk(&self, id: &str) -> Result<ChunkResponse> {
        let url = format!("{}/chunk/{}", self.base_url, id);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to connect to bobbin server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body: ErrorBody = resp.json().await.unwrap_or(ErrorBody {
                error: format!("HTTP {}", status),
            });
            anyhow::bail!("Server error ({}): {}", status, body.error);
        }

        resp.json()
            .await
            .context("Failed to parse chunk response")
    }

    /// Return the base URL (for display/logging).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = Client::new("http://localhost:3030/");
        assert_eq!(client.base_url(), "http://localhost:3030");
    }

    #[test]
    fn test_client_preserves_url_without_trailing_slash() {
        let client = Client::new("http://localhost:3030");
        assert_eq!(client.base_url(), "http://localhost:3030");
    }

    #[test]
    fn test_deserialize_search_response() {
        let json = r#"{
            "query": "test",
            "mode": "hybrid",
            "count": 1,
            "results": [{
                "file_path": "src/main.rs",
                "name": "main",
                "chunk_type": "function",
                "start_line": 1,
                "end_line": 10,
                "score": 0.95,
                "match_type": "semantic",
                "language": "rust",
                "content_preview": "fn main() {}"
            }]
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.query, "test");
        assert_eq!(resp.mode, "hybrid");
        assert_eq!(resp.count, 1);
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].file_path, "src/main.rs");
        assert_eq!(resp.results[0].score, 0.95);
    }

    #[test]
    fn test_deserialize_status_response() {
        let json = r#"{
            "status": "ok",
            "index": {
                "total_files": 42,
                "total_chunks": 100,
                "total_embeddings": 100,
                "languages": [{"language": "rust", "file_count": 42, "chunk_count": 100}],
                "last_indexed": 1700000000,
                "index_size_bytes": 1024
            }
        }"#;
        let resp: StatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.index.total_files, 42);
        assert_eq!(resp.index.total_chunks, 100);
        assert_eq!(resp.index.languages.len(), 1);
        assert_eq!(resp.index.languages[0].language, "rust");
    }

    #[test]
    fn test_deserialize_search_response_null_optional_fields() {
        let json = r#"{
            "query": "test",
            "mode": "keyword",
            "count": 1,
            "results": [{
                "file_path": "src/lib.rs",
                "name": null,
                "chunk_type": "function",
                "start_line": 5,
                "end_line": 15,
                "score": 0.5,
                "match_type": null,
                "language": "rust",
                "content_preview": "pub fn foo()"
            }]
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert!(resp.results[0].name.is_none());
        assert!(resp.results[0].match_type.is_none());
    }

    #[tokio::test]
    async fn test_client_connection_refused() {
        let client = Client::new("http://127.0.0.1:19999");
        let result = client.status().await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to connect") || err.contains("error"),
            "Unexpected error: {}",
            err
        );
    }
}
