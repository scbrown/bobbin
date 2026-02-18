use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::index::Embedder;
use crate::index::git::GitAnalyzer;
use crate::search::hybrid::apply_recency_boost;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{Chunk, ChunkType, FileCategory, MatchType, classify_file};

/// Configuration for context assembly
pub struct ContextConfig {
    pub budget_lines: usize,
    pub depth: u32,
    pub max_coupled: usize,
    pub coupling_threshold: f32,
    pub semantic_weight: f32,
    pub content_mode: ContentMode,
    pub search_limit: usize,
    /// Demotion factor for Documentation/Config files in search ranking.
    /// Applied as a multiplier to RRF scores: 1.0 = no demotion, 0.5 = half score.
    /// Source/Test files are unaffected. Default: 0.5.
    pub doc_demotion: f32,
    /// Half-life for recency decay in days (0.0 = disabled)
    pub recency_half_life_days: f32,
    /// Recency weight (0.0 = disabled, 0.3 = default)
    pub recency_weight: f32,
    /// RRF constant k. Default: 60.0.
    pub rrf_k: f32,
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
    pub category: FileCategory,
    pub score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub coupled_to: Vec<String>,
    pub chunks: Vec<ContextChunk>,
}

/// A chunk within a context file
#[derive(Debug, Clone, Serialize)]
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
    pub bridged_additions: usize,
    pub source_files: usize,
    pub doc_files: usize,
    /// Raw cosine similarity of the top semantic search result (before RRF normalization).
    /// Used by the gate_threshold check to decide whether to inject context at all.
    pub top_semantic_score: f32,
}

/// How a file was found
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRelevance {
    Direct,
    Coupled,
    /// Found via git blame provenance bridging from a documentation chunk
    Bridged,
}

/// How a seed chunk was discovered
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SeedSource {
    /// Found via hybrid search (semantic + keyword)
    Search {
        match_type: MatchType,
    },
    /// Found via git diff overlap
    Diff {
        status: String,
        added_lines: usize,
        removed_lines: usize,
    },
}

/// An externally-provided seed for context assembly.
///
/// Wraps a `Chunk` with a relevance score and provenance information.
/// Both search-based and diff-based workflows produce `SeedChunk`s
/// that feed into `ContextAssembler::assemble_from_seeds()`.
#[derive(Debug, Clone, Serialize)]
pub struct SeedChunk {
    pub chunk: Chunk,
    pub score: f32,
    pub source: SeedSource,
}

/// Assembles task-relevant context from search and git history
pub struct ContextAssembler {
    embedder: Embedder,
    vector_store: VectorStore,
    metadata_store: MetadataStore,
    config: ContextConfig,
    git_analyzer: Option<GitAnalyzer>,
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
    indexed_at: Option<i64>,
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
            git_analyzer: None,
        }
    }

    /// Set the git analyzer for doc→source provenance bridging
    pub fn with_git_analyzer(mut self, git_analyzer: GitAnalyzer) -> Self {
        self.git_analyzer = Some(git_analyzer);
        self
    }

    /// Assemble a context bundle for the given query using hybrid search.
    ///
    /// This is the standard entry point: runs hybrid search to find seeds,
    /// then expands via coupling and applies budget constraints.
    pub async fn assemble(mut self, query: &str, repo: Option<&str>) -> Result<ContextBundle> {
        // Phase 1: Seed via hybrid search
        let (seed_results, top_semantic_score) = self.run_hybrid_search(query, repo).await?;

        // Convert internal SeedResults to public SeedChunks
        let seeds: Vec<SeedChunk> = seed_results
            .into_iter()
            .map(|r| SeedChunk {
                chunk: Chunk {
                    id: r.chunk_id,
                    file_path: r.file_path,
                    language: r.language,
                    name: r.name,
                    chunk_type: r.chunk_type,
                    start_line: r.start_line,
                    end_line: r.end_line,
                    content: r.content,
                },
                score: r.score,
                source: SeedSource::Search {
                    match_type: r.match_type.unwrap_or(MatchType::Hybrid),
                },
            })
            .collect();

        let mut bundle = self.assemble_from_seeds(query, seeds, repo).await?;
        bundle.summary.top_semantic_score = top_semantic_score;
        Ok(bundle)
    }

    /// Assemble a context bundle from externally-provided seed chunks.
    ///
    /// This is the generalized entry point that accepts pre-computed seeds
    /// (from search, diff analysis, or any other source) and runs coupling
    /// expansion + budget assembly on them.
    pub async fn assemble_from_seeds(
        mut self,
        query: &str,
        seeds: Vec<SeedChunk>,
        repo: Option<&str>,
    ) -> Result<ContextBundle> {
        // Collect unique files from seeds
        let seed_files: HashSet<String> =
            seeds.iter().map(|s| s.chunk.file_path.clone()).collect();

        // Convert SeedChunks to internal SeedResults for assembly
        let seed_results: Vec<SeedResult> = seeds
            .into_iter()
            .map(|s| {
                let match_type = match &s.source {
                    SeedSource::Search { match_type } => Some(*match_type),
                    SeedSource::Diff { .. } => None,
                };
                SeedResult {
                    chunk_id: s.chunk.id,
                    file_path: s.chunk.file_path,
                    language: s.chunk.language,
                    name: s.chunk.name,
                    chunk_type: s.chunk.chunk_type,
                    start_line: s.chunk.start_line,
                    end_line: s.chunk.end_line,
                    content: s.chunk.content,
                    score: s.score,
                    match_type,
                    indexed_at: None, // External seeds don't carry indexed_at
                }
            })
            .collect();

        // Phase 2: Expand via temporal coupling
        let coupled_chunks = self
            .expand_coupling(&seed_files, repo)
            .await?;

        // Phase 2b: Bridge docs to source via git blame provenance
        let bridged_chunks = self
            .bridge_docs_via_provenance(&seed_results, repo)
            .await?;

        // Phase 3: Assemble with budget
        assemble_bundle(query, &self.config, seed_results, coupled_chunks, bridged_chunks)
    }

    /// Bridge documentation chunks to source files via git blame provenance.
    ///
    /// For each seed that is a documentation file, blame its line range to find
    /// the commits that introduced those lines, then find source files changed
    /// in those same commits. Returns additional SeedChunks for the discovered
    /// source files, marked with Bridged relevance.
    async fn bridge_docs_via_provenance(
        &mut self,
        seeds: &[SeedResult],
        repo: Option<&str>,
    ) -> Result<Vec<CoupledChunkInfo>> {
        let git = match &self.git_analyzer {
            Some(g) => g,
            None => return Ok(vec![]),
        };

        let mut bridged_chunks: Vec<CoupledChunkInfo> = Vec::new();
        let mut seen_files: HashSet<String> = HashSet::new();

        // Collect files already in seeds to avoid re-adding them
        for seed in seeds {
            seen_files.insert(seed.file_path.clone());
        }

        for seed in seeds {
            let category = classify_file(&seed.file_path);
            if category != FileCategory::Documentation {
                continue;
            }

            // Blame the matching chunk's line range
            let blame_entries = match git.blame_lines(&seed.file_path, seed.start_line, seed.end_line) {
                Ok(entries) => entries,
                Err(_) => continue, // Silent fail — never block injection
            };

            // Deduplicate commit hashes
            let commit_hashes: HashSet<String> = blame_entries
                .into_iter()
                .map(|e| e.commit_hash)
                .collect();

            for hash in &commit_hashes {
                // Find source files changed in the same commit
                let commit_files = match git.get_commit_files(hash) {
                    Ok(files) => files,
                    Err(_) => continue,
                };

                for file_path in commit_files {
                    let file_category = classify_file(&file_path);
                    if file_category != FileCategory::Source {
                        continue;
                    }
                    if seen_files.contains(&file_path) {
                        continue;
                    }
                    seen_files.insert(file_path.clone());

                    // Fetch chunks for the bridged source file
                    let chunks = match self.vector_store.get_chunks_for_file(&file_path, repo).await {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    for chunk in chunks {
                        bridged_chunks.push(CoupledChunkInfo {
                            chunk_id: chunk.id.clone(),
                            file_path: chunk.file_path.clone(),
                            language: chunk.language.clone(),
                            name: chunk.name.clone(),
                            chunk_type: chunk.chunk_type,
                            start_line: chunk.start_line,
                            end_line: chunk.end_line,
                            content: chunk.content.clone(),
                            coupling_score: seed.score, // Use doc chunk's score as proxy
                            coupled_to: seed.file_path.clone(),
                        });
                    }
                }
            }
        }

        Ok(bridged_chunks)
    }

    /// Expand seed files via temporal coupling relationships.
    async fn expand_coupling(
        &mut self,
        seed_files: &HashSet<String>,
        repo: Option<&str>,
    ) -> Result<Vec<CoupledChunkInfo>> {
        let mut coupled_chunks: Vec<CoupledChunkInfo> = Vec::new();

        if self.config.depth > 0 {
            let mut seen_coupled_files: HashSet<String> = HashSet::new();

            for seed_file in seed_files {
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
                    if seed_files.contains(other_file)
                        || seen_coupled_files.contains(other_file)
                    {
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

        Ok(coupled_chunks)
    }

    /// Run hybrid search manually to avoid ownership issues with HybridSearch.
    /// Returns `(results, top_semantic_score)` where `top_semantic_score` is the
    /// raw cosine similarity of the best semantic match before RRF normalization.
    async fn run_hybrid_search(
        &mut self,
        query: &str,
        repo: Option<&str>,
    ) -> Result<(Vec<SeedResult>, f32)> {
        let fetch_limit = self.config.search_limit * 2;

        // Semantic search uses raw query (embeddings handle natural language)
        let query_embedding = self.embedder.embed(query).await?;
        let semantic_results = self
            .vector_store
            .search(&query_embedding, fetch_limit, repo)
            .await?;
        // FTS uses preprocessed query (stopwords removed for better BM25)
        let keyword_query = crate::search::preprocess::preprocess_for_keywords(query);
        let keyword_results = self
            .vector_store
            .search_fts(&keyword_query, fetch_limit, repo)
            .await
            .unwrap_or_default();

        // Capture raw cosine similarity of the top semantic result before RRF
        let top_semantic_score = semantic_results
            .first()
            .map(|r| r.score)
            .unwrap_or(0.0);

        // RRF combination
        let k = self.config.rrf_k;
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
                        indexed_at: result.indexed_at,
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
                        indexed_at: result.indexed_at,
                    },
                    rrf_score,
                ));
        }

        // Apply category-based demotion and recency boost: doc/config files get
        // demoted so source/test files rank higher when RRF scores are close.
        // Recency boost favors recently-indexed content over stale results.
        let doc_demotion = self.config.doc_demotion;
        let recency_hl = self.config.recency_half_life_days;
        let recency_w = self.config.recency_weight;
        let mut combined: Vec<_> = scores
            .into_values()
            .map(|(result, score)| {
                let category = classify_file(&result.file_path);
                let demoted = match category {
                    FileCategory::Documentation | FileCategory::Config => score * doc_demotion,
                    _ => score,
                };
                let adjusted = apply_recency_boost(demoted, result.indexed_at, recency_hl, recency_w);
                (result, adjusted)
            })
            .collect();
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Normalize RRF scores to [0, 1] so downstream threshold filters
        // (which expect similarity-scale scores) work correctly.
        let max_score = combined.first().map(|(_, s)| *s).unwrap_or(1.0);

        Ok((
            combined
                .into_iter()
                .take(self.config.search_limit)
                .map(|(mut result, score)| {
                    result.score = if max_score > 0.0 {
                        score / max_score
                    } else {
                        0.0
                    };
                    result
                })
                .collect(),
            top_semantic_score,
        ))
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

/// Assemble a context bundle from seed, coupled, and bridged results (pure logic, no I/O)
fn assemble_bundle(
    query: &str,
    config: &ContextConfig,
    seed_results: Vec<SeedResult>,
    coupled_chunks: Vec<CoupledChunkInfo>,
    bridged_chunks: Vec<CoupledChunkInfo>,
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
                path: file_path.clone(),
                language,
                relevance: FileRelevance::Direct,
                category: classify_file(&file_path),
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
                path: file_path.clone(),
                language,
                relevance: FileRelevance::Coupled,
                category: classify_file(&file_path),
                score: file_score,
                coupled_to,
                chunks: file_chunks,
            });
        }
    }

    // Group bridged chunks by file (same structure as coupled)
    let mut bridged_files: HashMap<String, (Vec<CoupledChunkInfo>, HashSet<String>)> =
        HashMap::new();
    for chunk in bridged_chunks {
        let entry = bridged_files
            .entry(chunk.file_path.clone())
            .or_insert_with(|| (Vec::new(), HashSet::new()));
        entry.1.insert(chunk.coupled_to.clone());
        entry.0.push(chunk);
    }

    let mut bridged_file_list: Vec<(String, Vec<CoupledChunkInfo>, HashSet<String>)> =
        bridged_files
            .into_iter()
            .map(|(path, (chunks, sources))| (path, chunks, sources))
            .collect();
    bridged_file_list.sort_by(|a, b| {
        let max_a = a.1.iter().map(|c| c.coupling_score).fold(f32::NEG_INFINITY, f32::max);
        let max_b = b.1.iter().map(|c| c.coupling_score).fold(f32::NEG_INFINITY, f32::max);
        max_b.partial_cmp(&max_a).unwrap()
    });

    let mut bridged_addition_count: usize = 0;

    // Add bridged chunks (source files discovered via doc provenance)
    for (file_path, mut chunks, sources) in bridged_file_list {
        if used_lines >= budget {
            break;
        }

        chunks.sort_by_key(|c| c.start_line);

        let language = chunks.first().map(|c| c.language.clone()).unwrap_or_default();
        let file_score = chunks.iter().map(|c| c.coupling_score).fold(f32::NEG_INFINITY, f32::max);

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
            bridged_addition_count += 1;

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
                path: file_path.clone(),
                language,
                relevance: FileRelevance::Bridged,
                category: classify_file(&file_path),
                score: file_score,
                coupled_to,
                chunks: file_chunks,
            });
        }
    }

    let total_chunks = context_files.iter().map(|f| f.chunks.len()).sum();
    let total_files = context_files.len();
    let source_files = context_files.iter().filter(|f| f.category == FileCategory::Source || f.category == FileCategory::Test).count();
    let doc_files = context_files.iter().filter(|f| f.category == FileCategory::Documentation).count();

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
            bridged_additions: bridged_addition_count,
            source_files,
            doc_files,
            top_semantic_score: 0.0,
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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        let seeds = vec![
            make_seed("c1", "a.rs", 1, 6, 0.9),  // 6 lines
            make_seed("c2", "b.rs", 1, 8, 0.8),  // 8 lines - won't fit (6+8 > 10)
            make_seed("c3", "c.rs", 1, 3, 0.7),  // 3 lines - fits (6+3 = 9 <= 10)
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![], vec![]).unwrap();

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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        let seeds = vec![
            make_seed("c1", "a.rs", 1, 5, 0.9),
            make_seed("c1", "a.rs", 1, 5, 0.8), // duplicate chunk ID
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![], vec![]).unwrap();
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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        let seeds = vec![make_seed("c1", "a.rs", 1, 5, 0.9)];

        let bundle = assemble_bundle("test", &config, seeds, vec![], vec![]).unwrap();
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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        let seeds = vec![make_seed("c1", "a.rs", 1, 5, 0.9)];
        let coupled = vec![make_coupled("c2", "b.rs", 1, 5, 0.5, "a.rs")];

        let bundle = assemble_bundle("test", &config, seeds, coupled, vec![]).unwrap();

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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        let seeds = vec![
            make_seed("c2", "a.rs", 20, 30, 0.8),
            make_seed("c1", "a.rs", 1, 10, 0.9),
        ];

        let bundle = assemble_bundle("test", &config, seeds, vec![], vec![]).unwrap();
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
            doc_demotion: 0.5,
            rrf_k: 60.0,
            recency_half_life_days: 0.0,
            recency_weight: 0.0,
        };

        // A chunk of 15 lines with budget of 20 - capped at 10 (50%)
        let seeds = vec![make_seed("c1", "a.rs", 1, 15, 0.9)];

        let bundle = assemble_bundle("test", &config, seeds, vec![], vec![]).unwrap();
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
            indexed_at: None,
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
