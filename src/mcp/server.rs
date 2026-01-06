//! MCP Server implementation for Bobbin.
//!
//! This module provides the main MCP server that exposes Bobbin's code search
//! and analysis capabilities to AI agents via the Model Context Protocol.

use std::path::PathBuf;

use anyhow::{Context, Result};
use regex::Regex;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Annotated, CallToolResult, Content, GetPromptRequestParam, GetPromptResult,
    Implementation, ListPromptsResult, ListResourcesResult, PaginatedRequestParam, Prompt,
    PromptMessage, PromptMessageRole, ProtocolVersion, RawResource, ReadResourceRequestParam,
    ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler};

use super::tools::*;
use crate::config::Config;
use crate::index::Embedder;
use crate::search::{HybridSearch, SemanticSearch};
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{ChunkType, MatchType, SearchResult};

/// MCP Server for Bobbin code search
#[derive(Clone)]
pub struct BobbinMcpServer {
    /// Path to the repository root
    repo_root: PathBuf,
    /// Tool router for MCP protocol
    tool_router: ToolRouter<Self>,
}

impl BobbinMcpServer {
    /// Create a new MCP server for the given repository
    pub fn new(repo_root: PathBuf) -> Result<Self> {
        // Check if bobbin is initialized
        let config_path = Config::config_path(&repo_root);
        if !config_path.exists() {
            anyhow::bail!(
                "Bobbin not initialized in {}. Run `bobbin init` first.",
                repo_root.display()
            );
        }

        Ok(Self {
            repo_root,
            tool_router: Self::tool_router(),
        })
    }

    /// Open the metadata store (not held across awaits to avoid Send issues)
    fn open_metadata_store(&self) -> Result<MetadataStore> {
        let db_path = Config::db_path(&self.repo_root);
        MetadataStore::open(&db_path).context("Failed to open metadata store")
    }

    /// Get index statistics as a JSON string
    fn get_stats_json(&self) -> Result<String> {
        let store = self.open_metadata_store()?;
        let stats = store.get_stats()?;
        Ok(serde_json::to_string_pretty(&stats)?)
    }

    /// Parse chunk type from string
    fn parse_chunk_type(s: &str) -> Result<ChunkType> {
        match s.to_lowercase().as_str() {
            "function" | "func" | "fn" => Ok(ChunkType::Function),
            "method" => Ok(ChunkType::Method),
            "class" => Ok(ChunkType::Class),
            "struct" => Ok(ChunkType::Struct),
            "enum" => Ok(ChunkType::Enum),
            "interface" => Ok(ChunkType::Interface),
            "module" | "mod" => Ok(ChunkType::Module),
            "impl" => Ok(ChunkType::Impl),
            "trait" => Ok(ChunkType::Trait),
            "doc" | "documentation" => Ok(ChunkType::Doc),
            "other" => Ok(ChunkType::Other),
            _ => anyhow::bail!("Unknown chunk type '{}'", s),
        }
    }

    /// Truncate content to a maximum length
    fn truncate_content(content: &str, max_len: usize) -> String {
        if content.len() <= max_len {
            content.to_string()
        } else {
            let truncated: String = content.chars().take(max_len).collect();
            format!("{}...", truncated.trim_end())
        }
    }

    /// Convert SearchResult to SearchResultItem
    fn to_search_result_item(result: &SearchResult) -> SearchResultItem {
        SearchResultItem {
            file_path: result.chunk.file_path.clone(),
            name: result.chunk.name.clone(),
            chunk_type: result.chunk.chunk_type.to_string(),
            start_line: result.chunk.start_line,
            end_line: result.chunk.end_line,
            score: result.score,
            match_type: result.match_type.map(|mt| match mt {
                MatchType::Semantic => "semantic".to_string(),
                MatchType::Keyword => "keyword".to_string(),
                MatchType::Hybrid => "hybrid".to_string(),
            }),
            language: result.chunk.language.clone(),
            content_preview: Self::truncate_content(&result.chunk.content, 300),
        }
    }

    /// Find matching lines in content
    fn find_matching_lines(
        content: &str,
        pattern: &str,
        regex: Option<&Regex>,
        ignore_case: bool,
        start_line: u32,
    ) -> Vec<MatchingLine> {
        let lines: Vec<&str> = content.lines().collect();
        let mut results = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            let matches = if let Some(re) = regex {
                re.is_match(line)
            } else if ignore_case {
                line.to_lowercase().contains(&pattern.to_lowercase())
            } else {
                line.contains(pattern)
            };

            if matches {
                results.push(MatchingLine {
                    line_number: start_line + idx as u32,
                    content: line.to_string(),
                });
            }

            // Limit to 10 matching lines per chunk
            if results.len() >= 10 {
                break;
            }
        }

        results
    }

    /// Read file content with line range
    fn read_file_lines(
        &self,
        file: &str,
        start: u32,
        end: u32,
        context: u32,
    ) -> Result<(String, u32, u32)> {
        let file_path = self.repo_root.join(file);

        if !file_path.exists() {
            anyhow::bail!("File not found: {}", file);
        }

        let content = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read file: {}", file))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len() as u32;

        // Calculate actual range with context
        let actual_start = start.saturating_sub(context).max(1);
        let actual_end = (end + context).min(total_lines);

        // Extract the lines (convert to 0-indexed)
        let start_idx = (actual_start - 1) as usize;
        let end_idx = actual_end as usize;

        let selected_lines = if end_idx <= lines.len() {
            lines[start_idx..end_idx].join("\n")
        } else {
            lines[start_idx..].join("\n")
        };

        Ok((selected_lines, actual_start, actual_end))
    }

    /// Detect language from file extension
    fn detect_language(file: &str) -> String {
        let ext = file.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "cpp" | "cc" | "cxx" | "hpp" | "h" => "cpp",
            "c" => "c",
            "md" => "markdown",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            _ => "unknown",
        }
        .to_string()
    }

    /// Build prompt content for codebase exploration
    fn build_explore_prompt(&self, focus: &str) -> Result<String> {
        let stats_json = self.get_stats_json()?;

        let prompt_text = match focus {
            "architecture" => format!(
                "# Codebase Exploration: Architecture\n\n\
                ## Index Statistics\n\
                ```json\n{}\n```\n\n\
                ## Exploration Steps\n\n\
                1. **Understand the structure**: Use `search` with queries like \"main entry point\", \"application setup\", or \"configuration\" to find key files.\n\n\
                2. **Identify core modules**: Search for \"module\", \"service\", or \"handler\" to find the main components.\n\n\
                3. **Find interfaces**: Use `grep` to search for trait/interface definitions that define contracts between components.\n\n\
                4. **Trace dependencies**: Use `related` on key files to understand how components connect.\n\n\
                ## Suggested Queries\n\n\
                - `search(\"main function\")` - Find entry points\n\
                - `search(\"configuration handling\")` - Find config code\n\
                - `grep(\"pub struct\", type=\"struct\")` - List public data structures\n\
                - `grep(\"pub trait\", type=\"trait\")` - List public traits\n",
                stats_json
            ),

            "entry_points" => format!(
                "# Codebase Exploration: Entry Points\n\n\
                ## Index Statistics\n\
                ```json\n{}\n```\n\n\
                ## Exploration Steps\n\n\
                1. **Find main functions**: Search for \"main\", \"run\", or \"start\" functions.\n\n\
                2. **Identify CLI commands**: Look for argument parsing, subcommands, or command handlers.\n\n\
                3. **Find API endpoints**: Search for route handlers, HTTP methods, or endpoint definitions.\n\n\
                4. **Trace initialization**: Follow the startup sequence from main to understand bootstrapping.\n\n\
                ## Suggested Queries\n\n\
                - `search(\"main entry point application\")` - Find main functions\n\
                - `grep(\"fn main\", type=\"function\")` - Find main() directly\n\
                - `search(\"command line arguments parsing\")` - Find CLI handling\n\
                - `search(\"http endpoint handler\")` - Find API routes\n",
                stats_json
            ),

            "dependencies" => format!(
                "# Codebase Exploration: Dependencies\n\n\
                ## Index Statistics\n\
                ```json\n{}\n```\n\n\
                ## Exploration Steps\n\n\
                1. **External dependencies**: Check Cargo.toml, package.json, or requirements.txt for external libs.\n\n\
                2. **Internal coupling**: Use `related` on core files to see which files change together.\n\n\
                3. **Import patterns**: Use `grep` to find import/use statements and understand dependencies.\n\n\
                4. **Shared utilities**: Search for helper functions, utilities, or common modules.\n\n\
                ## Suggested Queries\n\n\
                - `related(\"src/main.rs\")` - Find files coupled to main\n\
                - `grep(\"use crate::\")` - Find internal imports (Rust)\n\
                - `grep(\"import\")` - Find imports (Python/JS/TS)\n\
                - `search(\"shared utility helper\")` - Find utility code\n",
                stats_json
            ),

            "tests" => format!(
                "# Codebase Exploration: Tests\n\n\
                ## Index Statistics\n\
                ```json\n{}\n```\n\n\
                ## Exploration Steps\n\n\
                1. **Find test files**: Look for files with test in the name or test directories.\n\n\
                2. **Test patterns**: Identify testing frameworks and patterns used.\n\n\
                3. **Coverage areas**: See which modules have corresponding tests.\n\n\
                4. **Test utilities**: Find test helpers, fixtures, and mocks.\n\n\
                ## Suggested Queries\n\n\
                - `search(\"test\")` - Find test-related code\n\
                - `grep(\"#[test]\")` - Find Rust tests\n\
                - `grep(\"def test\")` - Find Python tests\n\
                - `grep(\"describe(\")` - Find JS/TS tests\n\
                - `search(\"mock fixture test helper\")` - Find test utilities\n",
                stats_json
            ),

            _ => format!(
                "# Codebase Exploration: {}\n\n\
                ## Index Statistics\n\
                ```json\n{}\n```\n\n\
                ## Getting Started\n\n\
                Use these tools to explore:\n\n\
                1. **`search`**: Natural language queries for semantic code search\n\
                2. **`grep`**: Exact pattern matching for specific terms\n\
                3. **`related`**: Find files that change together\n\
                4. **`read_chunk`**: View specific code sections\n\n\
                ## Suggested First Steps\n\n\
                1. Check the index stats above to understand the codebase size\n\
                2. Use `search(\"{}\")` to find relevant code\n\
                3. Use `related` on interesting files to understand connections\n\
                4. Use `read_chunk` to examine specific code sections\n",
                focus, stats_json, focus
            ),
        };

        Ok(prompt_text)
    }
}

#[tool_router]
impl BobbinMcpServer {
    /// Semantic search for code
    #[tool(description = "Search for code using natural language. Finds functions, classes, and other code elements that match the semantic meaning of your query. Best for: 'functions that handle authentication', 'error handling code', 'database connection logic'.")]
    async fn search(
        &self,
        Parameters(req): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);
        let mode = req.mode.as_deref().unwrap_or("hybrid");

        // Parse type filter
        let type_filter = if let Some(ref t) = req.r#type {
            Some(Self::parse_chunk_type(t).map_err(|e| McpError::internal_error(e.to_string(), None))?)
        } else {
            None
        };

        let config_path = Config::config_path(&self.repo_root);
        let config = Config::load(&config_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Open store fresh for this request (avoids Send issues)
        let store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Check if index exists
        let stats = store
            .get_stats()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        // Request more results if filtering
        let search_limit = if type_filter.is_some() {
            limit * 3
        } else {
            limit
        };

        let results: Vec<SearchResult> = match mode {
            "keyword" => store
                .search_fts(&req.query, search_limit)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,

            "semantic" | "hybrid" => {
                let lance_path = Config::lance_path(&self.repo_root);
                let model_dir = Config::model_cache_dir()
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                let embedder = Embedder::load(&model_dir, &config.embedding.model)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                let vector_store = VectorStore::open(&lance_path)
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                if mode == "semantic" {
                    let mut search = SemanticSearch::new(embedder, vector_store);
                    search
                        .search(&req.query, search_limit)
                        .await
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?
                } else {
                    // For hybrid, we need to do semantic search first, then combine
                    // We can't hold the store reference across await, so we do keyword search first
                    let keyword_results = store
                        .search_fts(&req.query, search_limit)
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                    // Now do semantic search
                    let mut semantic_search = SemanticSearch::new(embedder, vector_store);
                    let semantic_results = semantic_search
                        .search(&req.query, search_limit)
                        .await
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                    // Combine using RRF
                    HybridSearch::combine(
                        semantic_results,
                        keyword_results,
                        config.search.semantic_weight,
                        search_limit,
                    )
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?
                }
            }

            _ => {
                return Err(McpError::invalid_params(
                    format!("Invalid search mode: {}. Use 'hybrid', 'semantic', or 'keyword'", mode),
                    None,
                ));
            }
        };

        // Filter by chunk type
        let filtered: Vec<SearchResult> = if let Some(ref chunk_type) = type_filter {
            results
                .into_iter()
                .filter(|r| &r.chunk.chunk_type == chunk_type)
                .take(limit)
                .collect()
        } else {
            results.into_iter().take(limit).collect()
        };

        let response = SearchResponse {
            query: req.query,
            mode: mode.to_string(),
            count: filtered.len(),
            results: filtered.iter().map(Self::to_search_result_item).collect(),
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Keyword/regex search
    #[tool(description = "Search for code using exact keywords or regex patterns. Best for: finding specific function names, variable references, or pattern matching. Use ignore_case=true for case-insensitive search, regex=true for regex patterns.")]
    async fn grep(&self, Parameters(req): Parameters<GrepRequest>) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);
        let ignore_case = req.ignore_case.unwrap_or(false);
        let use_regex = req.regex.unwrap_or(false);

        // Parse type filter
        let type_filter = if let Some(ref t) = req.r#type {
            Some(Self::parse_chunk_type(t).map_err(|e| McpError::internal_error(e.to_string(), None))?)
        } else {
            None
        };

        // Build regex if needed
        let regex_pattern = if use_regex {
            let pattern = if ignore_case {
                format!("(?i){}", req.pattern)
            } else {
                req.pattern.clone()
            };
            Some(
                Regex::new(&pattern)
                    .map_err(|e| McpError::invalid_params(format!("Invalid regex: {}", e), None))?,
            )
        } else {
            None
        };

        let store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Check if index exists
        let stats = store
            .get_stats()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        // FTS query
        let fts_query = if use_regex {
            // Extract terms from regex for FTS
            let cleaned: String = req
                .pattern
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '_' || c == ' ' { c } else { ' ' })
                .collect();
            let words: Vec<&str> = cleaned
                .split_whitespace()
                .filter(|w| w.len() >= 2)
                .collect();
            if words.is_empty() {
                req.pattern.clone()
            } else {
                words.join(" OR ")
            }
        } else {
            req.pattern.clone()
        };

        let search_limit = if type_filter.is_some() || use_regex {
            limit * 5
        } else {
            limit
        };

        let results = store
            .search_fts(&fts_query, search_limit)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Apply filters
        let filtered: Vec<SearchResult> = results
            .into_iter()
            .filter(|r| {
                if let Some(ref chunk_type) = type_filter {
                    &r.chunk.chunk_type == chunk_type
                } else {
                    true
                }
            })
            .filter(|r| {
                if let Some(ref re) = regex_pattern {
                    re.is_match(&r.chunk.content)
                        || r.chunk.name.as_ref().is_some_and(|n| re.is_match(n))
                } else {
                    true
                }
            })
            .filter(|r| {
                if !ignore_case && regex_pattern.is_none() {
                    r.chunk.content.contains(&req.pattern)
                        || r.chunk.name.as_ref().is_some_and(|n| n.contains(&req.pattern))
                } else {
                    true
                }
            })
            .take(limit)
            .collect();

        let response = GrepResponse {
            pattern: req.pattern.clone(),
            count: filtered.len(),
            results: filtered
                .iter()
                .map(|r| GrepResultItem {
                    file_path: r.chunk.file_path.clone(),
                    name: r.chunk.name.clone(),
                    chunk_type: r.chunk.chunk_type.to_string(),
                    start_line: r.chunk.start_line,
                    end_line: r.chunk.end_line,
                    score: r.score,
                    language: r.chunk.language.clone(),
                    content_preview: Self::truncate_content(&r.chunk.content, 200),
                    matching_lines: Self::find_matching_lines(
                        &r.chunk.content,
                        &req.pattern,
                        regex_pattern.as_ref(),
                        ignore_case,
                        r.chunk.start_line,
                    ),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Find related files
    #[tool(description = "Find files that are related to a given file based on git commit history. Files that frequently change together have higher coupling scores. Useful for understanding dependencies and impact analysis.")]
    async fn related(
        &self,
        Parameters(req): Parameters<RelatedRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(10);
        let threshold = req.threshold.unwrap_or(0.0);

        let store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Verify file exists in index
        if store
            .get_file(&req.file)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .is_none()
        {
            return Err(McpError::invalid_params(
                format!("File not found in index: {}", req.file),
                None,
            ));
        }

        let couplings = store
            .get_coupling(&req.file, limit)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let related: Vec<RelatedFile> = couplings
            .into_iter()
            .filter(|c| c.score >= threshold)
            .map(|c| {
                let other_path = if c.file_a == req.file {
                    c.file_b
                } else {
                    c.file_a
                };
                RelatedFile {
                    path: other_path,
                    score: c.score,
                    co_changes: c.co_changes,
                }
            })
            .collect();

        let response = RelatedResponse {
            file: req.file,
            related,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Read a specific code chunk
    #[tool(description = "Read a specific section of code from a file. Specify the file path and line range. Optionally include context lines before and after.")]
    async fn read_chunk(
        &self,
        Parameters(req): Parameters<ReadChunkRequest>,
    ) -> Result<CallToolResult, McpError> {
        let context = req.context.unwrap_or(0);

        let (content, actual_start, actual_end) = self
            .read_file_lines(&req.file, req.start_line, req.end_line, context)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = ReadChunkResponse {
            file: req.file.clone(),
            start_line: req.start_line,
            end_line: req.end_line,
            actual_start_line: actual_start,
            actual_end_line: actual_end,
            content,
            language: Self::detect_language(&req.file),
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for BobbinMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
            server_info: Implementation {
                name: "bobbin".to_string(),
                title: Some("Bobbin Code Search".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Bobbin is a semantic code search engine. Use the search tool for natural language queries, \
                grep for exact pattern matching, related for finding coupled files, and read_chunk to \
                view specific code sections. Start with `bobbin://index/stats` to see the index status."
                    .to_string(),
            ),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            meta: None,
            resources: vec![Annotated::new(
                RawResource::new("bobbin://index/stats", "Index Statistics"),
                None,
            )],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri == "bobbin://index/stats" {
            let stats_json = self
                .get_stats_json()
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;

            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(stats_json, &request.uri)],
            })
        } else {
            Err(McpError::resource_not_found(
                format!("Unknown resource: {}", request.uri),
                None,
            ))
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            meta: None,
            prompts: vec![Prompt::new(
                "explore_codebase",
                Some("Guided exploration of the codebase with focused prompts for understanding architecture, entry points, dependencies, and tests"),
                Some(vec![rmcp::model::PromptArgument {
                    name: "focus".to_string(),
                    title: Some("Focus Area".to_string()),
                    description: Some(
                        "Optional focus area: 'architecture', 'entry_points', 'dependencies', 'tests', or a custom query"
                            .to_string(),
                    ),
                    required: Some(false),
                }]),
            )],
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        if request.name != "explore_codebase" {
            return Err(McpError::invalid_params(
                format!("Unknown prompt: {}", request.name),
                None,
            ));
        }

        let focus = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("focus"))
            .and_then(|v| v.as_str())
            .unwrap_or("architecture");

        let prompt_text = self
            .build_explore_prompt(focus)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(GetPromptResult {
            description: Some(format!("Explore codebase with focus on: {}", focus)),
            messages: vec![PromptMessage::new_text(PromptMessageRole::User, prompt_text)],
        })
    }
}

/// Run the MCP server on stdio transport
pub async fn run_server(repo_root: PathBuf) -> Result<()> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    let server = BobbinMcpServer::new(repo_root)?;

    // Serve on stdio transport
    let service = server.serve(stdio()).await?;

    // Wait for the service to complete
    service.waiting().await?;

    Ok(())
}
