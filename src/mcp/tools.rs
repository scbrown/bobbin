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

/// Request for finding symbol references
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindRefsRequest {
    /// Symbol name to find references for
    #[schemars(description = "Exact symbol name to find references for (e.g., 'parse_config', 'Config', 'handle_request')")]
    pub symbol: String,

    /// Filter by symbol type (function, struct, trait, etc.)
    #[schemars(description = "Filter by symbol type: function, method, class, struct, enum, interface, module, impl, trait")]
    pub r#type: Option<String>,

    /// Maximum number of usage results (default: 20)
    #[schemars(description = "Maximum number of usage results to return (default: 20)")]
    pub limit: Option<usize>,

    /// Filter to a specific repository
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for finding symbol references
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FindRefsResponse {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<SymbolDefinitionOutput>,
    pub usage_count: usize,
    pub usages: Vec<SymbolUsageOutput>,
}

/// A symbol definition in the response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SymbolDefinitionOutput {
    pub name: String,
    pub chunk_type: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
}

/// A symbol usage in the response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SymbolUsageOutput {
    pub file_path: String,
    pub line: u32,
    pub context: String,
}

/// Request for listing symbols in a file
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListSymbolsRequest {
    /// File path (relative to repo root) to list symbols for
    #[schemars(description = "File path (relative to repo root) to list symbols for (e.g., 'src/main.rs', 'lib/config.ts')")]
    pub file: String,

    /// Filter to a specific repository
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for listing symbols in a file
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListSymbolsResponse {
    pub file: String,
    pub count: usize,
    pub symbols: Vec<SymbolItemOutput>,
}

/// A symbol in the list symbols response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SymbolItemOutput {
    pub name: String,
    pub chunk_type: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
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

/// Request for identifying code hotspots
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HotspotsRequest {
    /// Time window for churn analysis (e.g. "6 months ago", "1 year ago")
    #[schemars(description = "Time window for churn analysis (default: '1 year ago'). Examples: '6 months ago', '3 months ago', '2 years ago'")]
    pub since: Option<String>,

    /// Maximum number of hotspots to return (default: 20)
    #[schemars(description = "Maximum number of hotspots to return (default: 20)")]
    pub limit: Option<usize>,

    /// Minimum hotspot score threshold (0.0-1.0, default: 0.0)
    #[schemars(description = "Minimum hotspot score threshold (0.0-1.0, default: 0.0). Higher values filter to only the most critical hotspots.")]
    pub threshold: Option<f32>,
}

/// Response for code hotspots
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct HotspotsResponse {
    pub count: usize,
    pub since: String,
    pub hotspots: Vec<HotspotItem>,
}

/// A single hotspot entry
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct HotspotItem {
    pub file: String,
    pub score: f32,
    pub churn: u32,
    pub complexity: f32,
    pub language: String,
}

/// Request for project primer/overview
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrimeRequest {
    /// Show only a specific section (e.g. "architecture", "commands", "mcp tools")
    #[schemars(description = "Optional section name to show. Available: 'what bobbin does', 'architecture', 'supported languages', 'key commands', 'mcp tools', 'quick start', 'configuration'. Omit to show the full primer.")]
    pub section: Option<String>,

    /// Show a brief (compact) overview only
    #[schemars(description = "If true, show only the title and first section for a compact overview")]
    pub brief: Option<bool>,
}

/// Response for project primer/overview
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PrimeResponse {
    pub primer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<PrimeStats>,
}

/// Live index statistics included in prime response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PrimeStats {
    pub total_files: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub languages: Vec<PrimeLanguageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_indexed: Option<String>,
}

/// Per-language stats in prime response
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PrimeLanguageStats {
    pub language: String,
    pub file_count: u64,
    pub chunk_count: u64,
}

/// Request for diff-aware review context
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReviewRequest {
    /// Diff specification: "unstaged", "staged", a branch name (prefixed with "branch:"), or a commit range like "HEAD~3..HEAD"
    #[schemars(description = "What to diff. Use 'unstaged' for working tree changes, 'staged' for staged changes, 'branch:<name>' to compare a branch against main, or a commit range like 'HEAD~3..HEAD'.")]
    pub diff: Option<String>,

    /// Maximum lines of context to include (default: 500)
    #[schemars(description = "Maximum lines of code content to include in the review context (default: 500)")]
    pub budget: Option<usize>,

    /// Coupling expansion depth (default: 1, 0 = no coupling)
    #[schemars(description = "Depth of temporal coupling expansion. 0 disables coupling, 1 expands one level (default: 1)")]
    pub depth: Option<u32>,

    /// Filter coupled files to a specific repository
    #[schemars(description = "Filter results to a specific repository name. Omit to search across all indexed repos.")]
    pub repo: Option<String>,
}

/// Response for review context
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReviewResponse {
    pub diff_description: String,
    pub changed_files: Vec<ReviewChangedFile>,
    pub budget: ContextBudgetInfo,
    pub files: Vec<ContextFileOutput>,
    pub summary: ContextSummaryOutput,
}

/// A changed file in the review diff
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReviewChangedFile {
    pub path: String,
    pub status: String,
    pub added_lines: usize,
    pub removed_lines: usize,
}
