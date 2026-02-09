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
use crate::search::context::{ContentMode, ContextAssembler, ContextConfig, FileRelevance};
use crate::search::{HybridSearch, SemanticSearch};
use crate::storage::{MetadataStore, VectorStore};
use crate::analysis::complexity::ComplexityAnalyzer;
use crate::analysis::refs::RefAnalyzer;
use crate::index::GitAnalyzer;
use crate::types::{ChunkType, MatchType, SearchResult};

/// MCP Server for Bobbin code search
#[derive(Clone)]
pub struct BobbinMcpServer {
    repo_root: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl BobbinMcpServer {
    pub fn new(repo_root: PathBuf) -> Result<Self> {
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

    /// Open the metadata store (for coupling queries)
    fn open_metadata_store(&self) -> Result<MetadataStore> {
        let db_path = Config::db_path(&self.repo_root);
        MetadataStore::open(&db_path).context("Failed to open metadata store")
    }

    /// Open the vector store
    async fn open_vector_store(&self) -> Result<VectorStore> {
        let lance_path = Config::lance_path(&self.repo_root);
        VectorStore::open(&lance_path)
            .await
            .context("Failed to open vector store")
    }

    /// Get index statistics as a JSON string
    async fn get_stats_json(&self) -> Result<String> {
        let store = self.open_vector_store().await?;
        let stats = store.get_stats(None).await?;
        Ok(serde_json::to_string_pretty(&stats)?)
    }

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
            "section" => Ok(ChunkType::Section),
            "table" => Ok(ChunkType::Table),
            "code_block" | "codeblock" => Ok(ChunkType::CodeBlock),
            "commit" => Ok(ChunkType::Commit),
            "other" => Ok(ChunkType::Other),
            _ => anyhow::bail!("Unknown chunk type '{}'", s),
        }
    }

    fn truncate_content(content: &str, max_len: usize) -> String {
        if content.len() <= max_len {
            content.to_string()
        } else {
            let truncated: String = content.chars().take(max_len).collect();
            format!("{}...", truncated.trim_end())
        }
    }

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

            if results.len() >= 10 {
                break;
            }
        }

        results
    }

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

        let actual_start = start.saturating_sub(context).max(1);
        let actual_end = (end + context).min(total_lines);

        let start_idx = (actual_start - 1) as usize;
        let end_idx = actual_end as usize;

        let selected_lines = if end_idx <= lines.len() {
            lines[start_idx..end_idx].join("\n")
        } else {
            lines[start_idx..].join("\n")
        };

        Ok((selected_lines, actual_start, actual_end))
    }

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

    async fn build_explore_prompt(&self, focus: &str) -> Result<String> {
        let stats_json = self.get_stats_json().await?;

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

        let type_filter = if let Some(ref t) = req.r#type {
            Some(Self::parse_chunk_type(t).map_err(|e| McpError::internal_error(e.to_string(), None))?)
        } else {
            None
        };

        let config_path = Config::config_path(&self.repo_root);
        let config = Config::load(&config_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let stats = vector_store.get_stats(None).await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        let search_limit = if type_filter.is_some() {
            limit * 3
        } else {
            limit
        };

        let repo_filter = req.repo.as_deref();

        let results: Vec<SearchResult> = match mode {
            "keyword" => vector_store
                .search_fts(&req.query, search_limit, repo_filter)
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,

            "semantic" | "hybrid" => {
                let model_dir = Config::model_cache_dir()
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                let embedder = Embedder::from_config(&config.embedding, &model_dir)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;

                if mode == "semantic" {
                    let mut search = SemanticSearch::new(embedder, vector_store);
                    search
                        .search(&req.query, search_limit, repo_filter)
                        .await
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?
                } else {
                    let mut search = HybridSearch::new(
                        embedder,
                        vector_store,
                        config.search.semantic_weight,
                    );
                    search
                        .search(&req.query, search_limit, repo_filter)
                        .await
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

        let type_filter = if let Some(ref t) = req.r#type {
            Some(Self::parse_chunk_type(t).map_err(|e| McpError::internal_error(e.to_string(), None))?)
        } else {
            None
        };

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

        let mut vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let stats = vector_store.get_stats(None).await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        // Build FTS query
        let fts_query = if use_regex {
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

        let results = vector_store
            .search_fts(&fts_query, search_limit, req.repo.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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

    /// Assemble task-relevant context
    #[tool(description = "Assemble a comprehensive context bundle for a task. Given a natural language task description, combines semantic search results with temporally coupled files from git history. Returns a deduplicated, budget-aware set of relevant code chunks grouped by file. Ideal for understanding everything relevant to a task before making changes.")]
    async fn context(
        &self,
        Parameters(req): Parameters<ContextRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config_path = Config::config_path(&self.repo_root);
        let config = Config::load(&config_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let stats = vector_store.get_stats(None).await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        let metadata_store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let model_dir = Config::model_cache_dir()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let embedder = Embedder::from_config(&config.embedding, &model_dir)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let context_config = ContextConfig {
            budget_lines: req.budget.unwrap_or(500),
            depth: req.depth.unwrap_or(1),
            max_coupled: req.max_coupled.unwrap_or(3),
            coupling_threshold: req.coupling_threshold.unwrap_or(0.1),
            semantic_weight: config.search.semantic_weight,
            content_mode: ContentMode::Full, // Always full content for MCP
            search_limit: req.limit.unwrap_or(20),
        };

        let assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
        let bundle = assembler
            .assemble(&req.query, req.repo.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = ContextResponse {
            query: bundle.query,
            budget: ContextBudgetInfo {
                max_lines: bundle.budget.max_lines,
                used_lines: bundle.budget.used_lines,
            },
            files: bundle.files.iter().map(|f| ContextFileOutput {
                path: f.path.clone(),
                language: f.language.clone(),
                relevance: match f.relevance {
                    FileRelevance::Direct => "direct".to_string(),
                    FileRelevance::Coupled => "coupled".to_string(),
                },
                score: f.score,
                coupled_to: f.coupled_to.clone(),
                chunks: f.chunks.iter().map(|c| ContextChunkOutput {
                    name: c.name.clone(),
                    chunk_type: c.chunk_type.to_string(),
                    start_line: c.start_line,
                    end_line: c.end_line,
                    score: c.score,
                    match_type: c.match_type.map(|mt| match mt {
                        MatchType::Semantic => "semantic".to_string(),
                        MatchType::Keyword => "keyword".to_string(),
                        MatchType::Hybrid => "hybrid".to_string(),
                    }),
                    content: c.content.clone(),
                }).collect(),
            }).collect(),
            summary: ContextSummaryOutput {
                total_files: bundle.summary.total_files,
                total_chunks: bundle.summary.total_chunks,
                direct_hits: bundle.summary.direct_hits,
                coupled_additions: bundle.summary.coupled_additions,
            },
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

        let vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Verify file exists in index via LanceDB
        if vector_store
            .get_file(&req.file)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .is_none()
        {
            return Err(McpError::invalid_params(
                format!("File not found in index: {}", req.file),
                None,
            ));
        }

        // Coupling data is in SQLite
        let store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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

    /// Find symbol references
    #[tool(description = "Find the definition and all usages of a symbol by name. Returns the definition location (file, line, signature) and all usage sites across the codebase. Best for: 'where is parse_config defined?', 'who calls handle_request?', 'find all uses of Config struct'.")]
    async fn find_refs(
        &self,
        Parameters(req): Parameters<FindRefsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(20);

        let mut vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut analyzer = RefAnalyzer::new(&mut vector_store);
        let refs = analyzer
            .find_refs(&req.symbol, req.r#type.as_deref(), limit, req.repo.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = FindRefsResponse {
            symbol: req.symbol,
            definition: refs.definition.map(|d| SymbolDefinitionOutput {
                name: d.name,
                chunk_type: d.chunk_type.to_string(),
                file_path: d.file_path,
                start_line: d.start_line,
                end_line: d.end_line,
                signature: d.signature,
            }),
            usage_count: refs.usages.len(),
            usages: refs
                .usages
                .iter()
                .map(|u| SymbolUsageOutput {
                    file_path: u.file_path.clone(),
                    line: u.line,
                    context: u.context.clone(),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// List symbols in a file
    #[tool(description = "List all symbols (functions, structs, traits, etc.) defined in a file. Returns each symbol's name, type, line range, and signature. Best for: 'what functions are in main.rs?', 'list all structs in config.rs', 'show me the API of this module'.")]
    async fn list_symbols(
        &self,
        Parameters(req): Parameters<ListSymbolsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mut vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let analyzer = RefAnalyzer::new(&mut vector_store);
        let file_symbols = analyzer
            .list_symbols(&req.file, req.repo.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = ListSymbolsResponse {
            file: file_symbols.path,
            count: file_symbols.symbols.len(),
            symbols: file_symbols
                .symbols
                .iter()
                .map(|s| SymbolItemOutput {
                    name: s.name.clone(),
                    chunk_type: s.chunk_type.to_string(),
                    start_line: s.start_line,
                    end_line: s.end_line,
                    signature: s.signature.clone(),
                })
                .collect(),
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

    /// Identify code hotspots
    #[tool(description = "Identify code hotspots — files that are both frequently changed (high churn) and complex. Hotspots are the riskiest parts of a codebase: they change often and are hard to change safely. Score is the geometric mean of normalized churn and AST complexity. Best for: 'which files need refactoring?', 'find risky code', 'where are the maintenance bottlenecks?'.")]
    async fn hotspots(
        &self,
        Parameters(req): Parameters<HotspotsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let since = req.since.as_deref().unwrap_or("1 year ago");
        let limit = req.limit.unwrap_or(20);
        let threshold = req.threshold.unwrap_or(0.0);

        let git = GitAnalyzer::new(&self.repo_root)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let churn_map = git
            .get_file_churn(Some(since))
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if churn_map.is_empty() {
            let response = HotspotsResponse {
                count: 0,
                since: since.to_string(),
                hotspots: vec![],
            };
            let json = serde_json::to_string_pretty(&response)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let mut analyzer = ComplexityAnalyzer::new()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let max_churn = churn_map.values().copied().max().unwrap_or(1) as f32;
        let mut hotspots: Vec<HotspotItem> = Vec::new();

        for (file_path, churn) in &churn_map {
            let language = Self::detect_language(file_path);
            if matches!(
                language.as_str(),
                "unknown" | "markdown" | "json" | "yaml" | "toml" | "c"
            ) {
                continue;
            }

            let abs_path = self.repo_root.join(file_path);
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let complexity = match analyzer.analyze_file(file_path, &content, &language) {
                Ok(fc) => fc.complexity,
                Err(_) => continue,
            };

            let churn_norm = (*churn as f32) / max_churn;
            let score = (churn_norm * complexity).sqrt();

            if score >= threshold {
                hotspots.push(HotspotItem {
                    file: file_path.clone(),
                    score,
                    churn: *churn,
                    complexity,
                    language,
                });
            }
        }

        hotspots
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hotspots.truncate(limit);

        let response = HotspotsResponse {
            count: hotspots.len(),
            since: since.to_string(),
            hotspots,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Diff-aware review context
    #[tool(description = "Assemble review context from a git diff. Given a diff specification (unstaged changes, staged changes, branch comparison, or commit range), finds the indexed code chunks that overlap with the changed lines and expands via temporal coupling. Returns a budget-aware context bundle with changed-file annotations. Ideal for code review: 'what do I need to understand to review these changes?'")]
    async fn review(
        &self,
        Parameters(req): Parameters<ReviewRequest>,
    ) -> Result<CallToolResult, McpError> {
        let config_path = Config::config_path(&self.repo_root);
        let config = Config::load(&config_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let vector_store = self.open_vector_store().await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let stats = vector_store.get_stats(None).await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if stats.total_chunks == 0 {
            return Ok(CallToolResult::success(vec![Content::text(
                "No indexed content. Run `bobbin index` first.",
            )]));
        }

        let metadata_store = self.open_metadata_store()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let model_dir = Config::model_cache_dir()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let embedder = Embedder::from_config(&config.embedding, &model_dir)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Parse diff spec from the request
        let diff_spec = parse_diff_spec(req.diff.as_deref());
        let diff_description = describe_diff_spec(&diff_spec);

        let git = GitAnalyzer::new(&self.repo_root)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let diff_files = git
            .get_diff_files(&diff_spec)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        if diff_files.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                r#"{"error": "no_changes", "message": "No changes found for the specified diff."}"#,
            )]));
        }

        let seeds = crate::search::review::map_diff_to_chunks(
            &diff_files,
            &vector_store,
            req.repo.as_deref(),
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let context_config = ContextConfig {
            budget_lines: req.budget.unwrap_or(500),
            depth: req.depth.unwrap_or(1),
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: config.search.semantic_weight,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
        let bundle = assembler
            .assemble_from_seeds(&diff_description, seeds, req.repo.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let response = ReviewResponse {
            diff_description,
            changed_files: diff_files
                .iter()
                .map(|f| ReviewChangedFile {
                    path: f.path.clone(),
                    status: f.status.to_string(),
                    added_lines: f.added_lines.len(),
                    removed_lines: f.removed_lines.len(),
                })
                .collect(),
            budget: ContextBudgetInfo {
                max_lines: bundle.budget.max_lines,
                used_lines: bundle.budget.used_lines,
            },
            files: bundle.files.iter().map(|f| ContextFileOutput {
                path: f.path.clone(),
                language: f.language.clone(),
                relevance: match f.relevance {
                    FileRelevance::Direct => "direct".to_string(),
                    FileRelevance::Coupled => "coupled".to_string(),
                },
                score: f.score,
                coupled_to: f.coupled_to.clone(),
                chunks: f.chunks.iter().map(|c| ContextChunkOutput {
                    name: c.name.clone(),
                    chunk_type: c.chunk_type.to_string(),
                    start_line: c.start_line,
                    end_line: c.end_line,
                    score: c.score,
                    match_type: c.match_type.map(|mt| match mt {
                        MatchType::Semantic => "semantic".to_string(),
                        MatchType::Keyword => "keyword".to_string(),
                        MatchType::Hybrid => "hybrid".to_string(),
                    }),
                    content: c.content.clone(),
                }).collect(),
            }).collect(),
            summary: ContextSummaryOutput {
                total_files: bundle.summary.total_files,
                total_chunks: bundle.summary.total_chunks,
                direct_hits: bundle.summary.direct_hits,
                coupled_additions: bundle.summary.coupled_additions,
            },
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Project primer/overview
    #[tool(description = "Get an LLM-friendly overview of Bobbin with live index statistics. Shows what Bobbin does, architecture, available commands, and MCP tools. Use 'section' to get a specific part, or 'brief' for a compact summary. Always includes live stats when the index is initialized.")]
    async fn prime(
        &self,
        Parameters(req): Parameters<PrimeRequest>,
    ) -> Result<CallToolResult, McpError> {
        const PRIMER: &str = include_str!("../../docs/primer.md");

        let primer_text = if let Some(ref section) = req.section {
            Self::extract_primer_section(PRIMER, section)
        } else if req.brief.unwrap_or(false) {
            Self::extract_primer_brief(PRIMER)
        } else {
            PRIMER.to_string()
        };

        // Gather live stats
        let stats = match self.open_vector_store().await {
            Ok(store) => match store.get_stats(None).await {
                Ok(s) => Some(PrimeStats {
                    total_files: s.total_files,
                    total_chunks: s.total_chunks,
                    total_embeddings: s.total_embeddings,
                    languages: s
                        .languages
                        .iter()
                        .map(|l| PrimeLanguageStats {
                            language: l.language.clone(),
                            file_count: l.file_count,
                            chunk_count: l.chunk_count,
                        })
                        .collect(),
                    last_indexed: s.last_indexed.and_then(|ts| {
                        chrono::DateTime::from_timestamp(ts, 0).map(|t| t.to_rfc3339())
                    }),
                }),
                Err(_) => None,
            },
            Err(_) => None,
        };

        let response = PrimeResponse {
            primer: primer_text,
            section: req.section,
            initialized: true, // Server only runs when initialized
            stats,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

impl BobbinMcpServer {
    fn extract_primer_brief(primer: &str) -> String {
        let mut result = String::new();
        let mut heading_count = 0;

        for line in primer.lines() {
            if line.starts_with("## ") {
                heading_count += 1;
                if heading_count > 1 {
                    break;
                }
            }
            result.push_str(line);
            result.push('\n');
        }

        result.trim_end().to_string()
    }

    fn extract_primer_section(primer: &str, query: &str) -> String {
        let query_lower = query.to_lowercase();
        let sections = [
            "what bobbin does",
            "architecture",
            "supported languages",
            "key commands",
            "mcp tools",
            "quick start",
            "configuration",
        ];

        let target = sections
            .iter()
            .find(|s| s.contains(&query_lower.as_str()) || query_lower.contains(*s))
            .copied()
            .unwrap_or(query_lower.as_str());

        let mut result = String::new();
        let mut capturing = false;

        for line in primer.lines() {
            if line.starts_with("## ") {
                if capturing {
                    break;
                }
                let heading = line.trim_start_matches('#').trim().to_lowercase();
                if heading.contains(target) || target.contains(heading.as_str()) {
                    capturing = true;
                }
            }

            if capturing {
                result.push_str(line);
                result.push('\n');
            }
        }

        if result.is_empty() {
            format!(
                "Section '{}' not found. Available sections: {}",
                query,
                sections.join(", ")
            )
        } else {
            result.trim_end().to_string()
        }
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
                grep for exact pattern matching, find_refs to find symbol definitions and usages, \
                list_symbols to see all symbols in a file, related for finding coupled files, and read_chunk to \
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
                .await
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
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(GetPromptResult {
            description: Some(format!("Explore codebase with focus on: {}", focus)),
            messages: vec![PromptMessage::new_text(PromptMessageRole::User, prompt_text)],
        })
    }
}

/// Parse a diff spec string into a DiffSpec enum.
///
/// Accepts: "unstaged" (default), "staged", "branch:<name>", or a commit range like "HEAD~3..HEAD".
fn parse_diff_spec(spec: Option<&str>) -> crate::index::git::DiffSpec {
    use crate::index::git::DiffSpec;
    match spec {
        None | Some("unstaged") | Some("") => DiffSpec::Unstaged,
        Some("staged") => DiffSpec::Staged,
        Some(s) if s.starts_with("branch:") => DiffSpec::Branch(s[7..].to_string()),
        Some(range) => DiffSpec::Range(range.to_string()),
    }
}

/// Human-readable description of a DiffSpec.
fn describe_diff_spec(spec: &crate::index::git::DiffSpec) -> String {
    use crate::index::git::DiffSpec;
    match spec {
        DiffSpec::Unstaged => "unstaged changes".to_string(),
        DiffSpec::Staged => "staged changes".to_string(),
        DiffSpec::Branch(b) => format!("branch: {}", b),
        DiffSpec::Range(r) => format!("range: {}", r),
    }
}

/// Run the MCP server on stdio transport
pub async fn run_server(repo_root: PathBuf) -> Result<()> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    let server = BobbinMcpServer::new(repo_root)?;

    let service = server.serve(stdio()).await?;

    service.waiting().await?;

    Ok(())
}
