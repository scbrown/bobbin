//! MCP tool definitions for Bobbin.
//!
//! This module defines the request/response types for each MCP tool.

use serde::{Deserialize, Serialize};

/// Request for semantic search
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRequest {
    /// The search query (natural language description of what you're looking for)
    #[schemars(description = "Natural language search query describing the code you're looking for")]
    pub query: String,

    /// Filter by chunk type (function, method, class, struct, enum, interface, module, impl, trait)
    #[schemars(description = "Filter by code element type: function, method, class, struct, enum, interface, module, impl, trait")]
    pub r#type: Option<String>,

    /// Maximum number of results (default: 10)
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,

    /// Search mode: hybrid (default), semantic, or keyword
    #[schemars(description = "Search mode: 'hybrid' (combines semantic+keyword, default), 'semantic' (vector similarity), or 'keyword' (full-text)")]
    pub mode: Option<String>,

    /// Filter to a specific repository (searches all repos if omitted)
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for semantic search
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SearchResponse {
    pub query: String,
    pub mode: String,
    pub count: usize,
    pub results: Vec<SearchResultItem>,
}

/// A single search result item
#[derive(Debug, Serialize, schemars::JsonSchema)]
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

/// Request for keyword/regex search (grep)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GrepRequest {
    /// Pattern to search for (FTS query or regex with regex=true)
    #[schemars(description = "Pattern to search for. Supports full-text search queries, or regex if regex=true")]
    pub pattern: String,

    /// Case insensitive search
    #[schemars(description = "Enable case-insensitive search")]
    pub ignore_case: Option<bool>,

    /// Use extended regex matching
    #[schemars(description = "Enable extended regex matching (post-filters FTS results)")]
    pub regex: Option<bool>,

    /// Filter by chunk type
    #[schemars(description = "Filter by code element type: function, method, class, struct, enum, interface, module, impl, trait")]
    pub r#type: Option<String>,

    /// Maximum number of results (default: 10)
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,

    /// Filter to a specific repository (searches all repos if omitted)
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for grep search
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GrepResponse {
    pub pattern: String,
    pub count: usize,
    pub results: Vec<GrepResultItem>,
}

/// A single grep result item
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GrepResultItem {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub language: String,
    pub content_preview: String,
    pub matching_lines: Vec<MatchingLine>,
}

/// A line that matches the search pattern
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MatchingLine {
    pub line_number: u32,
    pub content: String,
}

/// Request for context assembly
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContextRequest {
    /// Natural language description of the task
    #[schemars(description = "Natural language task description to assemble context for")]
    pub query: String,

    /// Maximum lines of content to include (default: 500)
    #[schemars(description = "Maximum lines of code content to include in the context bundle (default: 500)")]
    pub budget: Option<usize>,

    /// Coupling expansion depth (default: 1, 0 = no coupling)
    #[schemars(description = "Depth of temporal coupling expansion. 0 disables coupling, 1 expands one level (default: 1)")]
    pub depth: Option<u32>,

    /// Max coupled files per seed file (default: 3)
    #[schemars(description = "Maximum number of coupled files to include per seed file (default: 3)")]
    pub max_coupled: Option<usize>,

    /// Max initial search results (default: 20)
    #[schemars(description = "Maximum number of initial search results to use as seeds (default: 20)")]
    pub limit: Option<usize>,

    /// Min coupling score threshold (default: 0.1)
    #[schemars(description = "Minimum coupling score threshold for including related files (default: 0.1)")]
    pub coupling_threshold: Option<f32>,

    /// Filter to specific repository
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for context assembly
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextResponse {
    pub query: String,
    pub budget: ContextBudgetInfo,
    pub files: Vec<ContextFileOutput>,
    pub summary: ContextSummaryOutput,
}

/// Budget information in context response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextBudgetInfo {
    pub max_lines: usize,
    pub used_lines: usize,
}

/// A file in the context response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextFileOutput {
    pub path: String,
    pub language: String,
    pub relevance: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub coupled_to: Vec<String>,
    pub chunks: Vec<ContextChunkOutput>,
}

/// A chunk in the context response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextChunkOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Summary statistics in context response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextSummaryOutput {
    pub total_files: usize,
    pub total_chunks: usize,
    pub direct_hits: usize,
    pub coupled_additions: usize,
}

/// Request for finding related files
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RelatedRequest {
    /// File path to find related files for
    #[schemars(description = "File path (relative to repo root) to find related files for")]
    pub file: String,

    /// Maximum number of results (default: 10)
    #[schemars(description = "Maximum number of related files to return (default: 10)")]
    pub limit: Option<usize>,

    /// Minimum score threshold (default: 0.0)
    #[schemars(description = "Minimum coupling score threshold (0.0-1.0, default: 0.0)")]
    pub threshold: Option<f32>,
}

/// Response for related files
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RelatedResponse {
    pub file: String,
    pub related: Vec<RelatedFile>,
}

/// A file related to the query file
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RelatedFile {
    pub path: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Request for reading a specific code chunk
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadChunkRequest {
    /// File path containing the chunk
    #[schemars(description = "File path (relative to repo root) containing the code chunk")]
    pub file: String,

    /// Start line of the chunk
    #[schemars(description = "Starting line number of the code chunk")]
    pub start_line: u32,

    /// End line of the chunk
    #[schemars(description = "Ending line number of the code chunk")]
    pub end_line: u32,

    /// Number of context lines to include before and after
    #[schemars(description = "Number of context lines to include before and after the chunk (default: 0)")]
    pub context: Option<u32>,
}

/// Response for reading a code chunk
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReadChunkResponse {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub actual_start_line: u32,
    pub actual_end_line: u32,
    pub content: String,
    pub language: String,
}
