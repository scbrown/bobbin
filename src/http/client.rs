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

/// Response from the /grep endpoint
#[derive(Deserialize)]
pub struct GrepResponse {
    pub pattern: String,
    pub count: usize,
    pub results: Vec<GrepResultItem>,
}

/// A single grep result item
#[derive(Deserialize)]
pub struct GrepResultItem {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub language: String,
    pub content_preview: String,
    pub matching_lines: Vec<GrepMatchingLine>,
}

/// A matching line from grep
#[derive(Deserialize)]
pub struct GrepMatchingLine {
    pub line_number: u32,
    pub content: String,
}

/// Response from the /context endpoint
#[derive(Deserialize)]
pub struct ContextResponse {
    pub query: String,
    pub budget: ContextBudgetInfo,
    pub files: Vec<ContextFileOutput>,
    pub summary: ContextSummaryOutput,
}

#[derive(Deserialize)]
pub struct ContextBudgetInfo {
    pub max_lines: usize,
    pub used_lines: usize,
}

#[derive(Deserialize)]
pub struct ContextFileOutput {
    pub path: String,
    pub language: String,
    pub relevance: String,
    pub score: f32,
    #[serde(default)]
    pub coupled_to: Vec<String>,
    pub chunks: Vec<ContextChunkOutput>,
}

#[derive(Deserialize)]
pub struct ContextChunkOutput {
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub match_type: Option<String>,
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub struct ContextSummaryOutput {
    pub total_files: usize,
    pub total_chunks: usize,
    pub direct_hits: usize,
    pub coupled_additions: usize,
    pub bridged_additions: usize,
    pub source_files: usize,
    pub doc_files: usize,
}

/// Response from the /read endpoint
#[derive(Deserialize)]
pub struct ReadChunkResponse {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub actual_start_line: u32,
    pub actual_end_line: u32,
    pub content: String,
    pub language: String,
}

/// Response from the /related endpoint
#[derive(Deserialize)]
pub struct RelatedResponse {
    pub file: String,
    pub related: Vec<RelatedFile>,
}

#[derive(Deserialize)]
pub struct RelatedFile {
    pub path: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Response from the /refs endpoint
#[derive(Deserialize)]
pub struct FindRefsResponse {
    pub symbol: String,
    pub definition: Option<SymbolDefinitionOutput>,
    pub usage_count: usize,
    pub usages: Vec<SymbolUsageOutput>,
}

#[derive(Deserialize)]
pub struct SymbolDefinitionOutput {
    pub name: String,
    pub chunk_type: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
}

#[derive(Deserialize)]
pub struct SymbolUsageOutput {
    pub file_path: String,
    pub line: u32,
    pub context: String,
}

/// Response from the /symbols endpoint
#[derive(Deserialize)]
pub struct ListSymbolsResponse {
    pub file: String,
    pub count: usize,
    pub symbols: Vec<SymbolItemOutput>,
}

#[derive(Deserialize)]
pub struct SymbolItemOutput {
    pub name: String,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
}

/// Response from the /hotspots endpoint
#[derive(Deserialize)]
pub struct HotspotsResponse {
    pub count: usize,
    pub since: String,
    pub hotspots: Vec<HotspotItem>,
}

#[derive(Deserialize)]
pub struct HotspotItem {
    pub file: String,
    pub score: f32,
    pub churn: u32,
    pub complexity: f32,
    pub language: String,
}

/// Response from the /impact endpoint
#[derive(Deserialize)]
pub struct ImpactResponse {
    pub target: String,
    pub mode: String,
    pub depth: u32,
    pub count: usize,
    pub results: Vec<ImpactResultItem>,
}

#[derive(Deserialize)]
pub struct ImpactResultItem {
    pub file: String,
    pub signal: String,
    pub score: f32,
    pub reason: String,
}

/// Response from the /review endpoint
#[derive(Deserialize)]
pub struct ReviewResponse {
    pub diff_description: String,
    pub changed_files: Vec<ReviewChangedFile>,
    pub budget: ContextBudgetInfo,
    pub files: Vec<ContextFileOutput>,
    pub summary: ContextSummaryOutput,
}

#[derive(Deserialize)]
pub struct ReviewChangedFile {
    pub path: String,
    pub status: String,
    pub added_lines: usize,
    pub removed_lines: usize,
}

/// Response from the /similar endpoint
#[derive(Deserialize)]
pub struct SimilarResponse {
    pub mode: String,
    pub threshold: f32,
    pub target: Option<String>,
    pub count: usize,
    pub results: Vec<SimilarResultItem>,
    pub clusters: Vec<SimilarClusterItem>,
}

#[derive(Deserialize)]
pub struct SimilarResultItem {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub similarity: f32,
    pub language: String,
    pub explanation: String,
}

#[derive(Deserialize)]
pub struct SimilarClusterItem {
    pub representative: SimilarChunkRef,
    pub avg_similarity: f32,
    pub member_count: usize,
    pub members: Vec<SimilarResultItem>,
}

#[derive(Deserialize)]
pub struct SimilarChunkRef {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub language: String,
}

/// Response from the /prime endpoint
#[derive(Deserialize)]
pub struct PrimeResponse {
    pub primer: String,
    pub section: Option<String>,
    pub initialized: bool,
    pub stats: Option<PrimeStats>,
}

#[derive(Deserialize)]
pub struct PrimeStats {
    pub total_files: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub languages: Vec<PrimeLanguageStats>,
    pub last_indexed: Option<String>,
}

#[derive(Deserialize)]
pub struct PrimeLanguageStats {
    pub language: String,
    pub file_count: u64,
    pub chunk_count: u64,
}

/// Response from the /beads endpoint
#[derive(Deserialize)]
pub struct SearchBeadsResponse {
    pub query: String,
    pub count: usize,
    pub results: Vec<BeadResultItem>,
}

#[derive(Deserialize)]
pub struct BeadResultItem {
    pub bead_id: String,
    pub title: String,
    pub priority: String,
    pub status: String,
    pub assignee: String,
    pub relevance_score: f32,
    pub snippet: String,
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
        self.get_json(&url, &params).await
    }

    /// Get index status from the remote server.
    pub async fn status(&self) -> Result<StatusResponse> {
        let url = format!("{}/status", self.base_url);
        self.get_json::<StatusResponse>(&url, &[]).await
    }

    /// Get a specific chunk by ID from the remote server.
    pub async fn get_chunk(&self, id: &str) -> Result<ChunkResponse> {
        let url = format!("{}/chunk/{}", self.base_url, id);
        self.get_json::<ChunkResponse>(&url, &[]).await
    }

    /// Grep via the remote server.
    pub async fn grep(
        &self,
        pattern: &str,
        ignore_case: bool,
        regex: bool,
        chunk_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<GrepResponse> {
        let url = format!("{}/grep", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("pattern", pattern.to_string()),
            ("limit", limit.to_string()),
        ];
        if ignore_case {
            params.push(("ignore_case", "true".to_string()));
        }
        if regex {
            params.push(("regex", "true".to_string()));
        }
        if let Some(t) = chunk_type {
            params.push(("type", t.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Assemble context via the remote server.
    pub async fn context(
        &self,
        query: &str,
        budget: Option<usize>,
        depth: Option<u32>,
        max_coupled: Option<usize>,
        limit: Option<usize>,
        coupling_threshold: Option<f32>,
        repo: Option<&str>,
    ) -> Result<ContextResponse> {
        let url = format!("{}/context", self.base_url);
        let mut params: Vec<(&str, String)> = vec![("q", query.to_string())];
        if let Some(b) = budget {
            params.push(("budget", b.to_string()));
        }
        if let Some(d) = depth {
            params.push(("depth", d.to_string()));
        }
        if let Some(mc) = max_coupled {
            params.push(("max_coupled", mc.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }
        if let Some(ct) = coupling_threshold {
            params.push(("coupling_threshold", ct.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Read a file chunk via the remote server.
    pub async fn read_chunk(
        &self,
        file: &str,
        start_line: u32,
        end_line: u32,
        context: Option<u32>,
    ) -> Result<ReadChunkResponse> {
        let url = format!("{}/read", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("file", file.to_string()),
            ("start_line", start_line.to_string()),
            ("end_line", end_line.to_string()),
        ];
        if let Some(c) = context {
            params.push(("context", c.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Find related files via the remote server.
    pub async fn related(
        &self,
        file: &str,
        limit: usize,
        threshold: Option<f32>,
    ) -> Result<RelatedResponse> {
        let url = format!("{}/related", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("file", file.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(t) = threshold {
            params.push(("threshold", t.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Find symbol references via the remote server.
    pub async fn find_refs(
        &self,
        symbol: &str,
        symbol_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<FindRefsResponse> {
        let url = format!("{}/refs", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("symbol", symbol.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(t) = symbol_type {
            params.push(("type", t.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// List symbols in a file via the remote server.
    pub async fn list_symbols(
        &self,
        file: &str,
        repo: Option<&str>,
    ) -> Result<ListSymbolsResponse> {
        let url = format!("{}/symbols", self.base_url);
        let mut params: Vec<(&str, String)> = vec![("file", file.to_string())];
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Get hotspots via the remote server.
    pub async fn hotspots(
        &self,
        since: Option<&str>,
        limit: usize,
        threshold: Option<f32>,
    ) -> Result<HotspotsResponse> {
        let url = format!("{}/hotspots", self.base_url);
        let mut params: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(s) = since {
            params.push(("since", s.to_string()));
        }
        if let Some(t) = threshold {
            params.push(("threshold", t.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Run impact analysis via the remote server.
    pub async fn impact(
        &self,
        target: &str,
        depth: Option<u32>,
        mode: Option<&str>,
        limit: usize,
        threshold: Option<f32>,
        repo: Option<&str>,
    ) -> Result<ImpactResponse> {
        let url = format!("{}/impact", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("target", target.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(d) = depth {
            params.push(("depth", d.to_string()));
        }
        if let Some(m) = mode {
            params.push(("mode", m.to_string()));
        }
        if let Some(t) = threshold {
            params.push(("threshold", t.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Get review context via the remote server.
    pub async fn review(
        &self,
        diff: Option<&str>,
        budget: Option<usize>,
        depth: Option<u32>,
        repo: Option<&str>,
    ) -> Result<ReviewResponse> {
        let url = format!("{}/review", self.base_url);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(d) = diff {
            params.push(("diff", d.to_string()));
        }
        if let Some(b) = budget {
            params.push(("budget", b.to_string()));
        }
        if let Some(d) = depth {
            params.push(("depth", d.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Find similar code via the remote server.
    pub async fn similar(
        &self,
        target: Option<&str>,
        scan: bool,
        threshold: Option<f32>,
        limit: usize,
        repo: Option<&str>,
        cross_repo: bool,
    ) -> Result<SimilarResponse> {
        let url = format!("{}/similar", self.base_url);
        let mut params: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(t) = target {
            params.push(("target", t.to_string()));
        }
        if scan {
            params.push(("scan", "true".to_string()));
        }
        if let Some(t) = threshold {
            params.push(("threshold", t.to_string()));
        }
        if let Some(r) = repo {
            params.push(("repo", r.to_string()));
        }
        if cross_repo {
            params.push(("cross_repo", "true".to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Get project primer via the remote server.
    pub async fn prime(
        &self,
        section: Option<&str>,
        brief: bool,
    ) -> Result<PrimeResponse> {
        let url = format!("{}/prime", self.base_url);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(s) = section {
            params.push(("section", s.to_string()));
        }
        if brief {
            params.push(("brief", "true".to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Search beads via the remote server.
    pub async fn search_beads(
        &self,
        query: &str,
        priority: Option<i32>,
        status: Option<&str>,
        assignee: Option<&str>,
        rig: Option<&str>,
        limit: usize,
        enrich: Option<bool>,
    ) -> Result<SearchBeadsResponse> {
        let url = format!("{}/beads", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(p) = priority {
            params.push(("priority", p.to_string()));
        }
        if let Some(s) = status {
            params.push(("status", s.to_string()));
        }
        if let Some(a) = assignee {
            params.push(("assignee", a.to_string()));
        }
        if let Some(r) = rig {
            params.push(("rig", r.to_string()));
        }
        if let Some(e) = enrich {
            params.push(("enrich", e.to_string()));
        }
        self.get_json(&url, &params).await
    }

    /// Return the base URL (for display/logging).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Internal helper: GET with query params, parse JSON response.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .query(params)
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

        resp.json().await.context("Failed to parse server response")
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
