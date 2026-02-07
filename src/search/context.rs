use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{ChunkType, MatchType};

/// Configuration for context assembly
pub struct ContextConfig {
    pub budget_lines: usize,
    pub depth: u32,
    pub max_coupled: usize,
    pub coupling_threshold: f32,
    pub semantic_weight: f32,
    pub content_mode: ContentMode,
    pub search_limit: usize,
}

/// How much content to include in output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Include full chunk content
    Full,
    /// Include first 3 lines + "..."
    Preview,
    /// No content, paths/metadata only
    None,
}

/// The assembled context bundle
#[derive(Debug, Serialize)]
pub struct ContextBundle {
    pub query: String,
    pub files: Vec<ContextFile>,
    pub budget: BudgetInfo,
    pub summary: ContextSummary,
}

/// A file included in the context bundle
#[derive(Debug, Serialize)]
pub struct ContextFile {
    pub path: String,
    pub language: String,
    pub relevance: FileRelevance,
    pub score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub coupled_to: Vec<String>,
    pub chunks: Vec<ContextChunk>,
}

/// A chunk within a context file
#[derive(Debug, Serialize)]
pub struct ContextChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub chunk_type: ChunkType,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_type: Option<MatchType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Budget information for the context bundle
#[derive(Debug, Serialize)]
pub struct BudgetInfo {
    pub max_lines: usize,
    pub used_lines: usize,
}

/// Summary statistics for the context bundle
#[derive(Debug, Serialize)]
pub struct ContextSummary {
    pub total_files: usize,
    pub total_chunks: usize,
    pub direct_hits: usize,
    pub coupled_additions: usize,
}

/// How a file was found
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRelevance {
    Direct,
    Coupled,
}

/// Assembles task-relevant context from search and git history
pub struct ContextAssembler {
    embedder: Embedder,
    vector_store: VectorStore,
    metadata_store: MetadataStore,
    config: ContextConfig,
}

/// Internal struct for seed search results
struct SeedResult {
    chunk_id: String,
    file_path: String,
    language: String,
    name: Option<String>,
    chunk_type: ChunkType,
    start_line: u32,
    end_line: u32,
    content: String,
    score: f32,
    match_type: Option<MatchType>,
}

/// Internal struct for coupled chunk information
struct CoupledChunkInfo {
    chunk_id: String,
    file_path: String,
    language: String,
    name: Option<String>,
    chunk_type: ChunkType,
    start_line: u32,
    end_line: u32,
    content: String,
    coupling_score: f32,
    coupled_to: String,
}

impl ContextAssembler {
    pub fn new(
        embedder: Embedder,
        vector_store: VectorStore,
        metadata_store: MetadataStore,
        config: ContextConfig,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            metadata_store,
            config,
        }
    }

    /// Assemble a context bundle for the given query
    pub async fn assemble(mut self, query: &str, repo: Option<&str>) -> Result<ContextBundle> {
        // Phase 1: Seed via hybrid search
        let seed_results = self.run_hybrid_search(query, repo).await?;

        // Collect unique files from seed results
        let seed_files: HashSet<String> = seed_results.iter().map(|r| r.file_path.clone()).collect();

        // Phase 2: Expand via temporal coupling
        let mut coupled_chunks: Vec<CoupledChunkInfo> = Vec::new();
        if self.config.depth > 0 {
            let mut seen_coupled_files: HashSet<String> = HashSet::new();

            for seed_file in &seed_files {
                let couplings =
                    self.metadata_store
                        .get_coupling(seed_file, self.config.max_coupled)?;

                for coupling in couplings {
                    if coupling.score < self.config.coupling_threshold {
                        continue;
                    }

                    let other_file = if coupling.file_a == *seed_file {
                        &coupling.file_b
                    } else {
                        &coupling.file_a
                    };

                    // Skip files already in seed results or already fetched
                    if seed_files.contains(other_file) || seen_coupled_files.contains(other_file) {
                        continue;
                    }
                    seen_coupled_files.insert(other_file.to_string());

                    let chunks = self
                        .vector_store
                        .get_chunks_for_file(other_file, repo)
                        .await?;

                    for chunk in chunks {
                        coupled_chunks.push(CoupledChunkInfo {
                            chunk_id: chunk.id.clone(),
                            file_path: chunk.file_path.clone(),
                            language: chunk.language.clone(),
                            name: chunk.name.clone(),
                            chunk_type: chunk.chunk_type,
                            start_line: chunk.start_line,
                            end_line: chunk.end_line,
                            content: chunk.content.clone(),
                            coupling_score: coupling.score,
                            coupled_to: seed_file.clone(),
                        });
                    }
                }
            }
        }

        // Phase 3: Assemble with budget
        assemble_bundle(query, &self.config, seed_results, coupled_chunks)
    }

    /// Run hybrid search manually to avoid ownership issues with HybridSearch
    async fn run_hybrid_search(
        &mut self,
        query: &str,
        repo: Option<&str>,
    ) -> Result<Vec<SeedResult>> {
        let fetch_limit = self.config.search_limit * 2;

        let query_embedding = self.embedder.embed(query).await?;
        let semantic_results = self
            .vector_store
            .search(&query_embedding, fetch_limit, repo)
            .await?;
        let keyword_results = self
            .vector_store
            .search_fts(query, fetch_limit, repo)
            .await?;

        // RRF combination
        let k = 60.0_f32;
        let keyword_weight = 1.0 - self.config.semantic_weight;
        let mut scores: HashMap<String, (SeedResult, f32)> = HashMap::new();

        for (rank, result) in semantic_results.into_iter().enumerate() {
            let rrf_score = self.config.semantic_weight / (k + rank as f32 + 1.0);
            scores.insert(
                result.chunk.id.clone(),
                (
                    SeedResult {
                        chunk_id: result.chunk.id,
                        file_path: result.chunk.file_path,
                        language: result.chunk.language,
                        name: result.chunk.name,
                        chunk_type: result.chunk.chunk_type,
                        start_line: result.chunk.start_line,
                        end_line: result.chunk.end_line,
                        content: result.chunk.content,
                        score: 0.0,
                        match_type: result.match_type,
                    },
                    rrf_score,
                ),
            );
        }

        for (rank, result) in keyword_results.into_iter().enumerate() {
            let rrf_score = keyword_weight / (k + rank as f32 + 1.0);
            scores
                .entry(result.chunk.id.clone())
                .and_modify(|(existing, score)| {
                    *score += rrf_score;
                    existing.match_type = Some(MatchType::Hybrid);
                })
                .or_insert((
                    SeedResult {
                        chunk_id: result.chunk.id,
                        file_path: result.chunk.file_path,
                        language: result.chunk.language,
                        name: result.chunk.name,
                        chunk_type: result.chunk.chunk_type,
                        start_line: result.chunk.start_line,
                        end_line: result.chunk.end_line,
                        content: result.chunk.content,
                        score: 0.0,
                        match_type: result.match_type,
                    },
                    rrf_score,
                ));
        }

        let mut combined: Vec<_> = scores.into_values().collect();
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        Ok(combined
            .into_iter()
            .take(self.config.search_limit)
            .map(|(mut result, score)| {
                result.score = score;
                result
            })
            .collect())
    }
}

/// Format content based on content mode
fn format_content(content: &str, mode: ContentMode) -> Option<String> {
    match mode {
        ContentMode::Full => Some(content.to_string()),
        ContentMode::Preview => {
            let lines: Vec<&str> = content.lines().take(3).collect();
            let preview = lines.join("\n");
            if content.lines().count() > 3 {
                Some(format!("{}...", preview))
            } else {
                Some(preview)
            }
        }
        ContentMode::None => None,
    }
}

/// Assemble a context bundle from seed and coupled results (pure logic, no I/O)
fn assemble_bundle(
    query: &str,
    config: &ContextConfig,
    seed_results: Vec<SeedResult>,
    coupled_chunks: Vec<CoupledChunkInfo>,
) -> Result<ContextBundle> {
    let budget = config.budget_lines;
    let max_chunk_lines = budget / 2; // Cap individual chunks at 50% of budget
    let mut used_lines: usize = 0;
    let mut seen_chunk_ids: HashSet<String> = HashSet::new();

    // Group seed results by file
    let mut direct_files: HashMap<String, Vec<SeedResult>> = HashMap::new();
    for result in seed_results {
        direct_files
            .entry(result.file_path.clone())
            .or_default()
            .push(result);
    }

    // Sort files by highest score
    let mut direct_file_list: Vec<(String, Vec<SeedResult>)> =
        direct_files.into_iter().collect();
    direct_file_list.sort_by(|a, b| {
        let max_a = a
            .1
            .iter()
            .map(|r| r.score)
            .fold(f32::NEG_INFINITY, f32::max);
        let max_b = b
            .1
            .iter()
            .map(|r| r.score)
            .fold(f32::NEG_INFINITY, f32::max);
        max_b.partial_cmp(&max_a).unwrap()
    });

    let mut context_files: Vec<ContextFile> = Vec::new();
    let mut direct_hit_count: usize = 0;

    // Add direct hit chunks
    for (file_path, mut results) in direct_file_list {
        results.sort_by_key(|r| r.start_line);

        let language = results
            .first()
            .map(|r| r.language.clone())
            .unwrap_or_default();

        let file_score = results
            .iter()
            .map(|r| r.score)
            .fold(f32::NEG_INFINITY, f32::max);

        let mut file_chunks = Vec::new();
        for result in results {
            if seen_chunk_ids.contains(&result.chunk_id) {
                continue;
            }

            let chunk_lines = (result.end_line - result.start_line + 1) as usize;
            let capped_lines = chunk_lines.min(max_chunk_lines);

            if used_lines + capped_lines > budget {
                break;
            }

            seen_chunk_ids.insert(result.chunk_id.clone());
            used_lines += capped_lines;
            direct_hit_count += 1;

            file_chunks.push(ContextChunk {
                name: result.name.clone(),
                chunk_type: result.chunk_type,
                start_line: result.start_line,
                end_line: result.end_line,
                score: result.score,
                match_type: result.match_type,
                content: format_content(&result.content, config.content_mode),
            });
        }

        if !file_chunks.is_empty() {
            context_files.push(ContextFile {
                path: file_path,
                language,
                relevance: FileRelevance::Direct,
                score: file_score,
                coupled_to: vec![],
                chunks: file_chunks,
            });
        }
    }

    // Group coupled chunks by file
    let mut coupled_files: HashMap<String, (Vec<CoupledChunkInfo>, HashSet<String>)> =
        HashMap::new();
    for chunk in coupled_chunks {
        let entry = coupled_files
            .entry(chunk.file_path.clone())
            .or_insert_with(|| (Vec::new(), HashSet::new()));
        entry.1.insert(chunk.coupled_to.clone());
        entry.0.push(chunk);
    }

    // Sort coupled files by coupling score
    let mut coupled_file_list: Vec<(String, Vec<CoupledChunkInfo>, HashSet<String>)> =
        coupled_files
            .into_iter()
            .map(|(path, (chunks, sources))| (path, chunks, sources))
            .collect();
    coupled_file_list.sort_by(|a, b| {
        let max_a = a
            .1
            .iter()
            .map(|c| c.coupling_score)
            .fold(f32::NEG_INFINITY, f32::max);
        let max_b = b
            .1
            .iter()
            .map(|c| c.coupling_score)
            .fold(f32::NEG_INFINITY, f32::max);
        max_b.partial_cmp(&max_a).unwrap()
    });

    let mut coupled_addition_count: usize = 0;

    // Add coupled chunks
    for (file_path, mut chunks, sources) in coupled_file_list {
        if used_lines >= budget {
            break;
        }

        chunks.sort_by_key(|c| c.start_line);

        let language = chunks
            .first()
            .map(|c| c.language.clone())
            .unwrap_or_default();

        let file_score = chunks
            .iter()
            .map(|c| c.coupling_score)
            .fold(f32::NEG_INFINITY, f32::max);

        let mut file_chunks = Vec::new();
        for chunk in chunks {
            if seen_chunk_ids.contains(&chunk.chunk_id) {
                continue;
            }

            let chunk_lines = (chunk.end_line - chunk.start_line + 1) as usize;
            let capped_lines = chunk_lines.min(max_chunk_lines);

            if used_lines + capped_lines > budget {
                break;
            }

            seen_chunk_ids.insert(chunk.chunk_id.clone());
            used_lines += capped_lines;
            coupled_addition_count += 1;

            file_chunks.push(ContextChunk {
                name: chunk.name.clone(),
                chunk_type: chunk.chunk_type,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                score: chunk.coupling_score,
                match_type: None,
                content: format_content(&chunk.content, config.content_mode),
            });
        }

        if !file_chunks.is_empty() {
            let mut coupled_to: Vec<String> = sources.into_iter().collect();
            coupled_to.sort();

            context_files.push(ContextFile {
                path: file_path,
                language,
                relevance: FileRelevance::Coupled,
                score: file_score,
                coupled_to,
                chunks: file_chunks,
            });
        }
    }

    let total_chunks = context_files.iter().map(|f| f.chunks.len()).sum();
    let total_files = context_files.len();

    Ok(ContextBundle {
        query: query.to_string(),
        files: context_files,
        budget: BudgetInfo {
            max_lines: budget,
            used_lines,
        },
        summary: ContextSummary {
            total_files,
            total_chunks,
            direct_hits: direct_hit_count,
            coupled_additions: coupled_addition_count,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_mode_full() {
        let result = format_content("line1\nline2\nline3\nline4\nline5", ContentMode::Full);
        assert_eq!(
            result,
            Some("line1\nline2\nline3\nline4\nline5".to_string())
        );
    }

    #[test]
    fn test_content_mode_preview_long() {
        let result = format_content("line1\nline2\nline3\nline4\nline5", ContentMode::Preview);
        assert_eq!(result, Some("line1\nline2\nline3...".to_string()));
    }

    #[test]
    fn test_content_mode_preview_short() {
        let result = format_content("line1\nline2", ContentMode::Preview);
        assert_eq!(result, Some("line1\nline2".to_string()));
    }

    #[test]
    fn test_content_mode_none() {
        let result = format_content("line1\nline2\nline3", ContentMode::None);
        assert!(result.is_none());
    }

    #[test]
    fn test_budget_enforcement() {
        let config = ContextConfig {
            budget_lines: 10,
            depth: 0,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let seeds = vec![
            make_seed("c1", "a.rs", 1, 6, 0.9),  // 6 lines
            make_seed("c2", "b.rs", 1, 8, 0.8),  // 8 lines - won't fit (6+8 > 10)
            make_seed("c3", "c.rs", 1, 3, 0.7),  // 3 lines - fits (6+3 = 9 <= 10)
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![]).unwrap();

        assert!(bundle.budget.used_lines <= bundle.budget.max_lines);
        assert_eq!(bundle.budget.max_lines, 10);
    }

    #[test]
    fn test_deduplication() {
        let config = ContextConfig {
            budget_lines: 500,
            depth: 0,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let seeds = vec![
            make_seed("c1", "a.rs", 1, 5, 0.9),
            make_seed("c1", "a.rs", 1, 5, 0.8), // duplicate chunk ID
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![]).unwrap();
        assert_eq!(bundle.summary.total_chunks, 1);
    }

    #[test]
    fn test_no_coupled_when_empty() {
        let config = ContextConfig {
            budget_lines: 500,
            depth: 0,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let seeds = vec![make_seed("c1", "a.rs", 1, 5, 0.9)];

        let bundle = assemble_bundle("test", &config, seeds, vec![]).unwrap();
        assert_eq!(bundle.summary.coupled_additions, 0);
    }

    #[test]
    fn test_file_ordering_direct_before_coupled() {
        let config = ContextConfig {
            budget_lines: 500,
            depth: 1,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let seeds = vec![make_seed("c1", "a.rs", 1, 5, 0.9)];
        let coupled = vec![make_coupled("c2", "b.rs", 1, 5, 0.5, "a.rs")];

        let bundle = assemble_bundle("test", &config, seeds, coupled).unwrap();

        assert_eq!(bundle.files.len(), 2);
        assert_eq!(bundle.files[0].relevance, FileRelevance::Direct);
        assert_eq!(bundle.files[1].relevance, FileRelevance::Coupled);
    }

    #[test]
    fn test_chunks_sorted_by_start_line_within_file() {
        let config = ContextConfig {
            budget_lines: 500,
            depth: 0,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        let seeds = vec![
            make_seed("c2", "a.rs", 20, 30, 0.8),
            make_seed("c1", "a.rs", 1, 10, 0.9),
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![]).unwrap();
        assert_eq!(bundle.files.len(), 1);
        assert_eq!(bundle.files[0].chunks[0].start_line, 1);
        assert_eq!(bundle.files[0].chunks[1].start_line, 20);
    }

    #[test]
    fn test_max_chunk_cap_at_50_percent() {
        let config = ContextConfig {
            budget_lines: 20,
            depth: 0,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: 0.7,
            content_mode: ContentMode::Full,
            search_limit: 20,
        };

        // A chunk of 15 lines with budget of 20 - capped at 10 (50%)
        let seeds = vec![make_seed("c1", "a.rs", 1, 15, 0.9)];

        let bundle = assemble_bundle("test", &config, seeds, vec![]).unwrap();
        // The chunk uses min(15, 10) = 10 lines of budget
        assert_eq!(bundle.budget.used_lines, 10);
    }

    fn make_seed(id: &str, file: &str, start: u32, end: u32, score: f32) -> SeedResult {
        SeedResult {
            chunk_id: id.to_string(),
            file_path: file.to_string(),
            language: "rust".to_string(),
            name: Some(format!("fn_{}", id)),
            chunk_type: ChunkType::Function,
            start_line: start,
            end_line: end,
            content: "fn test() {}".to_string(),
            score,
            match_type: Some(MatchType::Hybrid),
        }
    }

    fn make_coupled(
        id: &str,
        file: &str,
        start: u32,
        end: u32,
        coupling_score: f32,
        coupled_to: &str,
    ) -> CoupledChunkInfo {
        CoupledChunkInfo {
            chunk_id: id.to_string(),
            file_path: file.to_string(),
            language: "rust".to_string(),
            name: Some(format!("fn_{}", id)),
            chunk_type: ChunkType::Function,
            start_line: start,
            end_line: end,
            content: "fn test() {}".to_string(),
            coupling_score,
            coupled_to: coupled_to.to_string(),
        }
    }
}
