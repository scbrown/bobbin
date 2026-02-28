//! HTTP request handlers for the Bobbin REST API.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::analysis::complexity::ComplexityAnalyzer;
use crate::analysis::impact::{ImpactAnalyzer, ImpactConfig, ImpactMode, ImpactSignal};
use crate::analysis::refs::RefAnalyzer;
use crate::analysis::similar::{SimilarTarget, SimilarityAnalyzer};
use crate::config::Config;
use crate::index::{Embedder, GitAnalyzer};
use crate::search::context::{BridgeMode, ContentMode, ContextAssembler, ContextConfig, FileRelevance};
use crate::search::{HybridSearch, SemanticSearch};
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{ChunkType, MatchType, SearchResult};

use crate::access::RepoFilter;

use super::AppState;

/// Build a RepoFilter from app state and an optional role query param.
fn resolve_filter(state: &AppState, role: Option<&str>) -> RepoFilter {
    let resolved = RepoFilter::resolve_role(role);
    RepoFilter::from_config(&state.config.access, &resolved)
}

/// Build the axum router with all routes
pub(super) fn router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post};
    use tower_http::cors::CorsLayer;
    use tower_http::trace::TraceLayer;

    axum::Router::new()
        .route("/", get(ui_page))
        .route("/search", get(search))
        .route("/grep", get(grep))
        .route("/context", get(context))
        .route("/chunk/{id}", get(get_chunk))
        .route("/read", get(read_chunk))
        .route("/related", get(related))
        .route("/refs", get(find_refs))
        .route("/symbols", get(list_symbols))
        .route("/hotspots", get(hotspots))
        .route("/impact", get(impact))
        .route("/review", get(review))
        .route("/similar", get(similar))
        .route("/prime", get(prime))
        .route("/beads", get(search_beads))
        .route("/archive/search", get(archive_search))
        .route("/archive/entry/{id}", get(archive_entry))
        .route("/archive/recent", get(archive_recent))
        .route("/status", get(status))
        .route("/healthz", get(healthz))
        .route("/repos", get(list_repos))
        .route("/repos/{name}/files", get(list_repo_files))
        .route("/commands", get(list_commands))
        .route("/metrics", get(metrics))
        .route("/webhook/push", post(webhook_push))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Error response body
#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

/// Map anyhow errors to HTTP 500 responses
fn internal_error(err: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!("Internal error: {:#}", err);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: err.to_string(),
        }),
    )
}

// -- / (UI) --

/// Serve the embedded web UI
async fn ui_page() -> Html<&'static str> {
    Html(include_str!("ui.html"))
}

// -- /search --

#[derive(Deserialize)]
struct SearchParams {
    /// Search query
    q: String,
    /// Search mode: hybrid (default), semantic, keyword
    mode: Option<String>,
    /// Filter by chunk type
    r#type: Option<String>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Filter by repository name
    repo: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
struct SearchResponse {
    query: String,
    mode: String,
    count: usize,
    results: Vec<SearchResultItem>,
}

#[derive(Serialize)]
struct SearchResultItem {
    file_path: String,
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    score: f32,
    match_type: Option<String>,
    language: String,
    content_preview: String,
}

pub(super) async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let mode = params.mode.as_deref().unwrap_or("hybrid");

    let type_filter = params
        .r#type
        .as_deref()
        .map(parse_chunk_type)
        .transpose()
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
        })?;

    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Ok(Json(SearchResponse {
            query: params.q,
            mode: mode.to_string(),
            count: 0,
            results: vec![],
        }));
    }

    let search_limit = if type_filter.is_some() {
        limit * 3
    } else {
        limit
    };

    let repo_filter = params.repo.as_deref();

    let results = match mode {
        "keyword" => vector_store
            .search_fts(&params.q, search_limit, repo_filter)
            .await
            .map_err(|e| internal_error(e.into()))?,

        "semantic" | "hybrid" => {
            let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
            let embedder = Embedder::load(&model_dir, &state.config.embedding.model)
                .map_err(|e| internal_error(e.into()))?;

            if mode == "semantic" {
                let mut search = SemanticSearch::new(embedder, vector_store);
                search
                    .search(&params.q, search_limit, repo_filter)
                    .await
                    .map_err(|e| internal_error(e.into()))?
            } else {
                let mut search = HybridSearch::new(
                    embedder,
                    vector_store,
                    state.config.search.semantic_weight,
                );
                search
                    .search(&params.q, search_limit, repo_filter)
                    .await
                    .map_err(|e| internal_error(e.into()))?
            }
        }

        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: format!(
                        "Invalid mode: {}. Use 'hybrid', 'semantic', or 'keyword'",
                        mode
                    ),
                }),
            ));
        }
    };

    // Apply role-based access filtering
    let access = resolve_filter(&state, params.role.as_deref());
    let results = access.filter_vec(results, |r| RepoFilter::repo_from_path(&r.chunk.file_path));

    let filtered: Vec<_> = if let Some(ref chunk_type) = type_filter {
        results
            .into_iter()
            .filter(|r| &r.chunk.chunk_type == chunk_type)
            .take(limit)
            .collect()
    } else {
        results.into_iter().take(limit).collect()
    };

    Ok(Json(SearchResponse {
        query: params.q,
        mode: mode.to_string(),
        count: filtered.len(),
        results: filtered.iter().map(to_search_item).collect(),
    }))
}

// -- /chunk/{id} --

#[derive(Serialize)]
struct ChunkResponse {
    id: String,
    file_path: String,
    chunk_type: String,
    name: Option<String>,
    start_line: u32,
    end_line: u32,
    language: String,
    content: String,
}

pub(super) async fn get_chunk(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ChunkResponse>, (StatusCode, Json<ErrorBody>)> {
    let store = open_vector_store(&state).await.map_err(internal_error)?;

    let chunk = store
        .get_chunk_by_id(&id)
        .await
        .map_err(|e| internal_error(e.into()))?;

    match chunk {
        Some(c) => Ok(Json(ChunkResponse {
            id: c.id,
            file_path: c.file_path,
            chunk_type: c.chunk_type.to_string(),
            name: c.name,
            start_line: c.start_line,
            end_line: c.end_line,
            language: c.language,
            content: c.content,
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("Chunk not found: {}", id),
            }),
        )),
    }
}

// -- /status --

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    index: crate::types::IndexStats,
    sources: crate::config::SourcesConfig,
}

// -- /healthz (lightweight liveness probe — does NOT query the index) --

pub(super) async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

// -- /repos --

#[derive(Serialize)]
struct ReposListResponse {
    count: usize,
    repos: Vec<RepoSummary>,
}

#[derive(Serialize)]
struct RepoSummary {
    name: String,
    file_count: u64,
    chunk_count: u64,
    languages: Vec<crate::types::LanguageStats>,
}

pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReposListResponse>, (StatusCode, Json<ErrorBody>)> {
    let store = open_vector_store(&state).await.map_err(internal_error)?;

    let repos = store
        .get_all_repos()
        .await
        .map_err(|e| internal_error(e.into()))?;

    let mut summaries = Vec::new();
    for repo_name in &repos {
        let stats = store
            .get_stats(Some(repo_name))
            .await
            .map_err(|e| internal_error(e.into()))?;
        summaries.push(RepoSummary {
            name: repo_name.clone(),
            file_count: stats.total_files,
            chunk_count: stats.total_chunks,
            languages: stats.languages,
        });
    }

    // Sort by chunk count descending
    summaries.sort_by(|a, b| b.chunk_count.cmp(&a.chunk_count));

    Ok(Json(ReposListResponse {
        count: summaries.len(),
        repos: summaries,
    }))
}

// -- /repos/{name}/files --

#[derive(Serialize)]
struct RepoFilesResponse {
    repo: String,
    count: usize,
    files: Vec<RepoFileItem>,
}

#[derive(Serialize)]
struct RepoFileItem {
    path: String,
    language: String,
}

pub(super) async fn list_repo_files(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<RepoFilesResponse>, (StatusCode, Json<ErrorBody>)> {
    let store = open_vector_store(&state).await.map_err(internal_error)?;

    let files = store
        .get_all_file_paths(Some(&name))
        .await
        .map_err(|e| internal_error(e.into()))?;

    let items: Vec<RepoFileItem> = files
        .into_iter()
        .map(|p| {
            let lang = detect_language(&p);
            RepoFileItem {
                path: p,
                language: lang,
            }
        })
        .collect();

    Ok(Json(RepoFilesResponse {
        repo: name,
        count: items.len(),
        files: items,
    }))
}

// -- /commands (user-defined convenience commands) --

#[derive(Serialize)]
struct CommandsListResponse {
    count: usize,
    commands: std::collections::BTreeMap<String, CommandEntry>,
}

#[derive(Serialize)]
struct CommandEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    command: String,
    args: Vec<String>,
    expands_to: String,
}

pub(super) async fn list_commands(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CommandsListResponse>, (StatusCode, Json<ErrorBody>)> {
    let commands = crate::commands::load_commands(&state.repo_root)
        .map_err(|e| internal_error(e.into()))?;

    let entries: std::collections::BTreeMap<String, CommandEntry> = commands
        .into_iter()
        .map(|(name, def)| {
            let expands_to = {
                let mut parts = vec![format!("bobbin {}", def.command)];
                for arg in &def.args {
                    if arg.contains(' ') {
                        parts.push(format!("\"{}\"", arg));
                    } else {
                        parts.push(arg.clone());
                    }
                }
                parts.join(" ")
            };
            (
                name,
                CommandEntry {
                    description: def.description,
                    command: def.command,
                    args: def.args,
                    expands_to,
                },
            )
        })
        .collect();

    Ok(Json(CommandsListResponse {
        count: entries.len(),
        commands: entries,
    }))
}

// -- /status --

pub(super) async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorBody>)> {
    let store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;

    Ok(Json(StatusResponse {
        status: "ok".to_string(),
        index: stats,
        sources: state.config.sources.clone(),
    }))
}

// -- /metrics (Prometheus) --

pub(super) async fn metrics(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let store = match open_vector_store(&state).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                "# Failed to open vector store\nbobbin_up 0\n".to_string(),
            );
        }
    };

    let stats = store.get_stats(None).await.ok();

    let mut out = String::new();
    out.push_str("# HELP bobbin_up Whether bobbin is running.\n");
    out.push_str("# TYPE bobbin_up gauge\n");
    out.push_str("bobbin_up 1\n");

    if let Some(s) = stats {
        out.push_str("# HELP bobbin_index_files_total Total indexed files.\n");
        out.push_str("# TYPE bobbin_index_files_total gauge\n");
        out.push_str(&format!("bobbin_index_files_total {}\n", s.total_files));
        out.push_str("# HELP bobbin_index_chunks_total Total indexed chunks.\n");
        out.push_str("# TYPE bobbin_index_chunks_total gauge\n");
        out.push_str(&format!("bobbin_index_chunks_total {}\n", s.total_chunks));
        out.push_str("# HELP bobbin_index_embeddings_total Total embeddings.\n");
        out.push_str("# TYPE bobbin_index_embeddings_total gauge\n");
        out.push_str(&format!(
            "bobbin_index_embeddings_total {}\n",
            s.total_embeddings
        ));
    }

    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        out,
    )
}

// -- /webhook/push --

/// Forgejo push webhook payload (subset of fields we care about)
#[derive(Deserialize)]
struct ForgejoPushPayload {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    repository: Option<RepoInfo>,
}

#[derive(Deserialize)]
struct RepoInfo {
    full_name: Option<String>,
}

#[derive(Serialize)]
struct WebhookResponse {
    status: String,
    message: String,
}

pub(super) async fn webhook_push(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ForgejoPushPayload>,
) -> impl IntoResponse {
    let repo_name = payload
        .repository
        .as_ref()
        .and_then(|r| r.full_name.as_deref())
        .unwrap_or("unknown");

    let git_ref = payload.git_ref.as_deref().unwrap_or("unknown");

    tracing::info!(
        "Webhook push received: repo={}, ref={}",
        repo_name,
        git_ref
    );

    // Spawn background re-index task
    let repo_root = state.repo_root.clone();
    let config = state.config.clone();
    tokio::spawn(async move {
        tracing::info!("Starting background re-index from webhook");
        if let Err(e) = run_incremental_index(repo_root, config).await {
            tracing::error!("Background re-index failed: {:#}", e);
        } else {
            tracing::info!("Background re-index completed");
        }
    });

    Json(WebhookResponse {
        status: "accepted".to_string(),
        message: format!("Re-index queued for push to {}", git_ref),
    })
}

// -- /grep --

#[derive(Deserialize)]
struct GrepParams {
    /// Pattern to search for
    pattern: String,
    /// Case insensitive search
    ignore_case: Option<bool>,
    /// Use regex matching
    regex: Option<bool>,
    /// Filter by chunk type
    r#type: Option<String>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Filter by repository name
    repo: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
struct GrepResponse {
    pattern: String,
    count: usize,
    results: Vec<GrepResultItem>,
}

#[derive(Serialize)]
struct GrepResultItem {
    file_path: String,
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    score: f32,
    language: String,
    content_preview: String,
    matching_lines: Vec<MatchingLine>,
}

#[derive(Serialize)]
struct MatchingLine {
    line_number: u32,
    content: String,
}

pub(super) async fn grep(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GrepParams>,
) -> Result<Json<GrepResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let ignore_case = params.ignore_case.unwrap_or(false);
    let use_regex = params.regex.unwrap_or(false);

    let type_filter = params
        .r#type
        .as_deref()
        .map(parse_chunk_type)
        .transpose()
        .map_err(|e| bad_request(e.to_string()))?;

    let regex_pattern = if use_regex {
        let pat = if ignore_case {
            format!("(?i){}", params.pattern)
        } else {
            params.pattern.clone()
        };
        Some(Regex::new(&pat).map_err(|e| bad_request(format!("Invalid regex: {}", e)))?)
    } else {
        None
    };

    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Ok(Json(GrepResponse {
            pattern: params.pattern,
            count: 0,
            results: vec![],
        }));
    }

    // Build FTS query
    let fts_query = if use_regex {
        let cleaned: String = params
            .pattern
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == ' ' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let words: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|w| w.len() >= 2)
            .collect();
        if words.is_empty() {
            params.pattern.clone()
        } else {
            words.join(" OR ")
        }
    } else {
        params.pattern.clone()
    };

    let search_limit = if type_filter.is_some() || use_regex {
        limit * 5
    } else {
        limit
    };

    let results = vector_store
        .search_fts(&fts_query, search_limit, params.repo.as_deref())
        .await
        .map_err(|e| internal_error(e.into()))?;

    // Apply role-based access filtering
    let access = resolve_filter(&state, params.role.as_deref());
    let results = access.filter_vec(results, |r| RepoFilter::repo_from_path(&r.chunk.file_path));

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
                r.chunk.content.contains(&params.pattern)
                    || r.chunk
                        .name
                        .as_ref()
                        .is_some_and(|n| n.contains(&params.pattern))
            } else {
                true
            }
        })
        .take(limit)
        .collect();

    let response = GrepResponse {
        pattern: params.pattern.clone(),
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
                content_preview: truncate(&r.chunk.content, 200),
                matching_lines: find_matching_lines(
                    &r.chunk.content,
                    &params.pattern,
                    regex_pattern.as_ref(),
                    ignore_case,
                    r.chunk.start_line,
                ),
            })
            .collect(),
    };

    Ok(Json(response))
}

// -- /context --

#[derive(Deserialize)]
struct ContextParams {
    /// Task description query
    q: String,
    /// Max lines budget (default 500)
    budget: Option<usize>,
    /// Coupling expansion depth (default 1)
    depth: Option<u32>,
    /// Max coupled files per seed (default 3)
    max_coupled: Option<usize>,
    /// Max initial search results (default 20)
    limit: Option<usize>,
    /// Min coupling threshold (default 0.1)
    coupling_threshold: Option<f32>,
    /// Filter by repository
    repo: Option<String>,
}

#[derive(Serialize)]
struct ContextResponse {
    query: String,
    budget: ContextBudgetInfo,
    files: Vec<ContextFileOutput>,
    summary: ContextSummaryOutput,
}

#[derive(Serialize)]
struct ContextBudgetInfo {
    max_lines: usize,
    used_lines: usize,
}

#[derive(Serialize)]
struct ContextFileOutput {
    path: String,
    language: String,
    relevance: String,
    score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    coupled_to: Vec<String>,
    chunks: Vec<ContextChunkOutput>,
}

#[derive(Serialize)]
struct ContextChunkOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Serialize)]
struct ContextSummaryOutput {
    total_files: usize,
    total_chunks: usize,
    direct_hits: usize,
    coupled_additions: usize,
    bridged_additions: usize,
    source_files: usize,
    doc_files: usize,
}

pub(super) async fn context(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ContextParams>,
) -> Result<Json<ContextResponse>, (StatusCode, Json<ErrorBody>)> {
    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Ok(Json(ContextResponse {
            query: params.q,
            budget: ContextBudgetInfo {
                max_lines: params.budget.unwrap_or(500),
                used_lines: 0,
            },
            files: vec![],
            summary: ContextSummaryOutput {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
            },
        }));
    }

    let metadata_store = open_metadata_store(&state).map_err(internal_error)?;

    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    let context_config = ContextConfig {
        budget_lines: params.budget.unwrap_or(500),
        depth: params.depth.unwrap_or(1),
        max_coupled: params.max_coupled.unwrap_or(3),
        coupling_threshold: params.coupling_threshold.unwrap_or(0.1),
        semantic_weight: state.config.search.semantic_weight,
        content_mode: ContentMode::Full,
        search_limit: params.limit.unwrap_or(20),
        doc_demotion: state.config.search.doc_demotion,
        recency_half_life_days: state.config.search.recency_half_life_days,
        recency_weight: state.config.search.recency_weight,
        rrf_k: state.config.search.rrf_k,
        bridge_mode: BridgeMode::default(),
        bridge_boost_factor: 0.3,
    };

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let bundle = assembler
        .assemble(&params.q, params.repo.as_deref())
        .await
        .map_err(internal_error)?;

    Ok(Json(ContextResponse {
        query: bundle.query,
        budget: ContextBudgetInfo {
            max_lines: bundle.budget.max_lines,
            used_lines: bundle.budget.used_lines,
        },
        files: bundle.files.iter().map(to_context_file).collect(),
        summary: to_context_summary(&bundle.summary),
    }))
}

// -- /read --

#[derive(Deserialize)]
struct ReadChunkParams {
    /// File path (relative to repo root)
    file: String,
    /// Start line
    start_line: u32,
    /// End line
    end_line: u32,
    /// Context lines before/after (default 0)
    context: Option<u32>,
}

#[derive(Serialize)]
struct ReadChunkResponse {
    file: String,
    start_line: u32,
    end_line: u32,
    actual_start_line: u32,
    actual_end_line: u32,
    content: String,
    language: String,
}

pub(super) async fn read_chunk(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ReadChunkParams>,
) -> Result<Json<ReadChunkResponse>, (StatusCode, Json<ErrorBody>)> {
    let ctx = params.context.unwrap_or(0);
    let (content, actual_start, actual_end) =
        read_file_lines(&state.repo_root, &params.file, params.start_line, params.end_line, ctx)
            .map_err(internal_error)?;

    Ok(Json(ReadChunkResponse {
        file: params.file.clone(),
        start_line: params.start_line,
        end_line: params.end_line,
        actual_start_line: actual_start,
        actual_end_line: actual_end,
        content,
        language: detect_language(&params.file),
    }))
}

// -- /related --

#[derive(Deserialize)]
struct RelatedParams {
    /// File path to find related files for
    file: String,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Min coupling score threshold (default 0.0)
    threshold: Option<f32>,
}

#[derive(Serialize)]
struct RelatedResponse {
    file: String,
    related: Vec<RelatedFile>,
}

#[derive(Serialize)]
struct RelatedFile {
    path: String,
    score: f32,
    co_changes: u32,
}

pub(super) async fn related(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RelatedParams>,
) -> Result<Json<RelatedResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let threshold = params.threshold.unwrap_or(0.0);

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    // Verify file exists in index
    if vector_store
        .get_file(&params.file)
        .await
        .map_err(|e| internal_error(e.into()))?
        .is_none()
    {
        return Err(bad_request(format!(
            "File not found in index: {}",
            params.file
        )));
    }

    let store = open_metadata_store(&state).map_err(internal_error)?;
    let couplings = store
        .get_coupling(&params.file, limit)
        .map_err(internal_error)?;

    let related: Vec<RelatedFile> = couplings
        .into_iter()
        .filter(|c| c.score >= threshold)
        .map(|c| {
            let other_path = if c.file_a == params.file {
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

    Ok(Json(RelatedResponse {
        file: params.file,
        related,
    }))
}

// -- /refs --

#[derive(Deserialize)]
struct FindRefsParams {
    /// Symbol name to find references for
    symbol: String,
    /// Filter by symbol type
    r#type: Option<String>,
    /// Max usage results (default 20)
    limit: Option<usize>,
    /// Filter by repository
    repo: Option<String>,
}

#[derive(Serialize)]
struct FindRefsResponse {
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<SymbolDefinitionOutput>,
    usage_count: usize,
    usages: Vec<SymbolUsageOutput>,
}

#[derive(Serialize)]
struct SymbolDefinitionOutput {
    name: String,
    chunk_type: String,
    file_path: String,
    start_line: u32,
    end_line: u32,
    signature: String,
}

#[derive(Serialize)]
struct SymbolUsageOutput {
    file_path: String,
    line: u32,
    context: String,
}

pub(super) async fn find_refs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FindRefsParams>,
) -> Result<Json<FindRefsResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(20);

    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let mut analyzer = RefAnalyzer::new(&mut vector_store);
    let refs = analyzer
        .find_refs(
            &params.symbol,
            params.r#type.as_deref(),
            limit,
            params.repo.as_deref(),
        )
        .await
        .map_err(internal_error)?;

    Ok(Json(FindRefsResponse {
        symbol: params.symbol,
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
    }))
}

// -- /symbols --

#[derive(Deserialize)]
struct ListSymbolsParams {
    /// File path (relative to repo root)
    file: String,
    /// Filter by repository
    repo: Option<String>,
}

#[derive(Serialize)]
struct ListSymbolsResponse {
    file: String,
    count: usize,
    symbols: Vec<SymbolItemOutput>,
}

#[derive(Serialize)]
struct SymbolItemOutput {
    name: String,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    signature: String,
}

pub(super) async fn list_symbols(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListSymbolsParams>,
) -> Result<Json<ListSymbolsResponse>, (StatusCode, Json<ErrorBody>)> {
    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let analyzer = RefAnalyzer::new(&mut vector_store);
    let file_symbols = analyzer
        .list_symbols(&params.file, params.repo.as_deref())
        .await
        .map_err(internal_error)?;

    Ok(Json(ListSymbolsResponse {
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
    }))
}

// -- /hotspots --

#[derive(Deserialize)]
struct HotspotsParams {
    /// Time window (e.g. "6 months ago", default "1 year ago")
    since: Option<String>,
    /// Max results (default 20)
    limit: Option<usize>,
    /// Min score threshold (default 0.0)
    threshold: Option<f32>,
}

#[derive(Serialize)]
struct HotspotsResponse {
    count: usize,
    since: String,
    hotspots: Vec<HotspotItem>,
}

#[derive(Serialize)]
struct HotspotItem {
    file: String,
    score: f32,
    churn: u32,
    complexity: f32,
    language: String,
}

pub(super) async fn hotspots(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HotspotsParams>,
) -> Result<Json<HotspotsResponse>, (StatusCode, Json<ErrorBody>)> {
    let since = params.since.as_deref().unwrap_or("1 year ago");
    let limit = params.limit.unwrap_or(20);
    let threshold = params.threshold.unwrap_or(0.0);

    let git = GitAnalyzer::new(&state.repo_root).map_err(internal_error)?;
    let churn_map = git.get_file_churn(Some(since)).map_err(internal_error)?;

    if churn_map.is_empty() {
        return Ok(Json(HotspotsResponse {
            count: 0,
            since: since.to_string(),
            hotspots: vec![],
        }));
    }

    let mut analyzer = ComplexityAnalyzer::new().map_err(internal_error)?;
    let max_churn = churn_map.values().copied().max().unwrap_or(1) as f32;
    let mut hotspot_items: Vec<HotspotItem> = Vec::new();

    for (file_path, churn) in &churn_map {
        let language = detect_language(file_path);
        if matches!(
            language.as_str(),
            "unknown" | "markdown" | "json" | "yaml" | "toml" | "c"
        ) {
            continue;
        }

        let abs_path = state.repo_root.join(file_path);
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
            hotspot_items.push(HotspotItem {
                file: file_path.clone(),
                score,
                churn: *churn,
                complexity,
                language,
            });
        }
    }

    hotspot_items
        .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hotspot_items.truncate(limit);

    Ok(Json(HotspotsResponse {
        count: hotspot_items.len(),
        since: since.to_string(),
        hotspots: hotspot_items,
    }))
}

// -- /impact --

#[derive(Deserialize)]
struct ImpactParams {
    /// File path or file:function target
    target: String,
    /// Transitive depth (default 1)
    depth: Option<u32>,
    /// Signal mode: combined, coupling, semantic, deps
    mode: Option<String>,
    /// Max results (default 15)
    limit: Option<usize>,
    /// Min score threshold (default 0.1)
    threshold: Option<f32>,
    /// Filter by repository
    repo: Option<String>,
}

#[derive(Serialize)]
struct ImpactResponse {
    target: String,
    mode: String,
    depth: u32,
    count: usize,
    results: Vec<ImpactResultItem>,
}

#[derive(Serialize)]
struct ImpactResultItem {
    file: String,
    signal: String,
    score: f32,
    reason: String,
}

pub(super) async fn impact(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ImpactParams>,
) -> Result<Json<ImpactResponse>, (StatusCode, Json<ErrorBody>)> {
    let depth = params.depth.unwrap_or(1);
    let mode_str = params.mode.as_deref().unwrap_or("combined");
    let limit = params.limit.unwrap_or(15);
    let threshold = params.threshold.unwrap_or(0.1);

    let mode = match mode_str {
        "combined" => ImpactMode::Combined,
        "coupling" => ImpactMode::Coupling,
        "semantic" => ImpactMode::Semantic,
        "deps" => ImpactMode::Deps,
        _ => {
            return Err(bad_request(format!(
                "Invalid mode: {}. Use: combined, coupling, semantic, deps",
                mode_str
            )));
        }
    };

    let impact_config = ImpactConfig {
        mode,
        threshold,
        limit,
    };

    let metadata_store = open_metadata_store(&state).map_err(internal_error)?;
    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    let mut analyzer = ImpactAnalyzer::new(metadata_store, vector_store, embedder);
    let results = analyzer
        .analyze(&params.target, &impact_config, depth, params.repo.as_deref())
        .await
        .map_err(internal_error)?;

    let signal_name = |s: &ImpactSignal| -> &'static str {
        match s {
            ImpactSignal::Coupling { .. } => "coupling",
            ImpactSignal::Semantic { .. } => "semantic",
            ImpactSignal::Dependency => "deps",
            ImpactSignal::Combined => "combined",
        }
    };

    Ok(Json(ImpactResponse {
        target: params.target,
        mode: mode_str.to_string(),
        depth,
        count: results.len(),
        results: results
            .iter()
            .map(|r| ImpactResultItem {
                file: r.path.clone(),
                signal: signal_name(&r.signal).to_string(),
                score: r.score,
                reason: r.reason.clone(),
            })
            .collect(),
    }))
}

// -- /review --

#[derive(Deserialize)]
struct ReviewParams {
    /// Diff spec: "unstaged", "staged", "branch:<name>", or commit range
    diff: Option<String>,
    /// Max lines budget (default 500)
    budget: Option<usize>,
    /// Coupling expansion depth (default 1)
    depth: Option<u32>,
    /// Filter by repository
    repo: Option<String>,
}

#[derive(Serialize)]
struct ReviewResponse {
    diff_description: String,
    changed_files: Vec<ReviewChangedFile>,
    budget: ContextBudgetInfo,
    files: Vec<ContextFileOutput>,
    summary: ContextSummaryOutput,
}

#[derive(Serialize)]
struct ReviewChangedFile {
    path: String,
    status: String,
    added_lines: usize,
    removed_lines: usize,
}

pub(super) async fn review(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ReviewParams>,
) -> Result<Json<ReviewResponse>, (StatusCode, Json<ErrorBody>)> {
    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Err(internal_error(anyhow::anyhow!(
            "No indexed content. Run `bobbin index` first."
        )));
    }

    let metadata_store = open_metadata_store(&state).map_err(internal_error)?;
    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    let diff_spec = parse_diff_spec(params.diff.as_deref());
    let diff_description = describe_diff_spec(&diff_spec);

    let git = GitAnalyzer::new(&state.repo_root).map_err(internal_error)?;
    let diff_files = git.get_diff_files(&diff_spec).map_err(internal_error)?;

    if diff_files.is_empty() {
        return Ok(Json(ReviewResponse {
            diff_description,
            changed_files: vec![],
            budget: ContextBudgetInfo {
                max_lines: params.budget.unwrap_or(500),
                used_lines: 0,
            },
            files: vec![],
            summary: ContextSummaryOutput {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
            },
        }));
    }

    let seeds = crate::search::review::map_diff_to_chunks(
        &diff_files,
        &vector_store,
        params.repo.as_deref(),
    )
    .await
    .map_err(internal_error)?;

    let context_config = ContextConfig {
        budget_lines: params.budget.unwrap_or(500),
        depth: params.depth.unwrap_or(1),
        max_coupled: 3,
        coupling_threshold: 0.1,
        semantic_weight: state.config.search.semantic_weight,
        content_mode: ContentMode::Full,
        search_limit: 20,
        doc_demotion: state.config.search.doc_demotion,
        recency_half_life_days: state.config.search.recency_half_life_days,
        recency_weight: state.config.search.recency_weight,
        rrf_k: state.config.search.rrf_k,
        bridge_mode: BridgeMode::default(),
        bridge_boost_factor: 0.3,
    };

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let bundle = assembler
        .assemble_from_seeds(&diff_description, seeds, params.repo.as_deref())
        .await
        .map_err(internal_error)?;

    Ok(Json(ReviewResponse {
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
        files: bundle.files.iter().map(to_context_file).collect(),
        summary: to_context_summary(&bundle.summary),
    }))
}

// -- /similar --

#[derive(Deserialize)]
struct SimilarParams {
    /// Target chunk ref or text (required unless scan=true)
    target: Option<String>,
    /// Scan for duplicates across codebase
    scan: Option<bool>,
    /// Min similarity threshold
    threshold: Option<f32>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Filter by repository
    repo: Option<String>,
    /// Cross-repo comparison in scan mode
    cross_repo: Option<bool>,
}

#[derive(Serialize)]
struct SimilarResponse {
    mode: String,
    threshold: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    count: usize,
    results: Vec<SimilarResultItem>,
    clusters: Vec<SimilarClusterItem>,
}

#[derive(Serialize)]
struct SimilarResultItem {
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    similarity: f32,
    language: String,
    explanation: String,
}

#[derive(Serialize)]
struct SimilarClusterItem {
    representative: SimilarChunkRef,
    avg_similarity: f32,
    member_count: usize,
    members: Vec<SimilarResultItem>,
}

#[derive(Serialize)]
struct SimilarChunkRef {
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    language: String,
}

pub(super) async fn similar(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SimilarParams>,
) -> Result<Json<SimilarResponse>, (StatusCode, Json<ErrorBody>)> {
    let scan = params.scan.unwrap_or(false);
    let limit = params.limit.unwrap_or(10);
    let cross_repo = params.cross_repo.unwrap_or(false);

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Ok(Json(SimilarResponse {
            mode: if scan { "scan" } else { "single" }.to_string(),
            threshold: params.threshold.unwrap_or(0.85),
            target: params.target,
            count: 0,
            results: vec![],
            clusters: vec![],
        }));
    }

    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    let mut analyzer = SimilarityAnalyzer::new(embedder, vector_store);
    let repo_filter = params.repo.as_deref();

    let response = if scan {
        let threshold = params.threshold.unwrap_or(0.90);
        let clusters = analyzer
            .scan_duplicates(threshold, limit, repo_filter, cross_repo)
            .await
            .map_err(internal_error)?;

        SimilarResponse {
            mode: "scan".to_string(),
            threshold,
            target: None,
            count: clusters.len(),
            results: vec![],
            clusters: clusters
                .iter()
                .map(|c| SimilarClusterItem {
                    representative: SimilarChunkRef {
                        file_path: c.representative.file_path.clone(),
                        name: c.representative.name.clone(),
                        chunk_type: c.representative.chunk_type.to_string(),
                        start_line: c.representative.start_line,
                        end_line: c.representative.end_line,
                        language: c.representative.language.clone(),
                    },
                    avg_similarity: c.avg_similarity,
                    member_count: c.members.len(),
                    members: c
                        .members
                        .iter()
                        .map(|m| SimilarResultItem {
                            file_path: m.chunk.file_path.clone(),
                            name: m.chunk.name.clone(),
                            chunk_type: m.chunk.chunk_type.to_string(),
                            start_line: m.chunk.start_line,
                            end_line: m.chunk.end_line,
                            similarity: m.similarity,
                            language: m.chunk.language.clone(),
                            explanation: m.explanation.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    } else {
        let target_str = params
            .target
            .as_deref()
            .ok_or_else(|| bad_request("Either 'target' or 'scan=true' is required".to_string()))?;

        let threshold = params.threshold.unwrap_or(0.85);
        let target = parse_similar_target(target_str);

        let results = analyzer
            .find_similar(&target, threshold, limit, repo_filter)
            .await
            .map_err(internal_error)?;

        SimilarResponse {
            mode: "single".to_string(),
            threshold,
            target: Some(target_str.to_string()),
            count: results.len(),
            results: results
                .iter()
                .map(|r| SimilarResultItem {
                    file_path: r.chunk.file_path.clone(),
                    name: r.chunk.name.clone(),
                    chunk_type: r.chunk.chunk_type.to_string(),
                    start_line: r.chunk.start_line,
                    end_line: r.chunk.end_line,
                    similarity: r.similarity,
                    language: r.chunk.language.clone(),
                    explanation: r.explanation.clone(),
                })
                .collect(),
            clusters: vec![],
        }
    };

    Ok(Json(response))
}

// -- /prime --

#[derive(Deserialize)]
struct PrimeParams {
    /// Specific section to show
    section: Option<String>,
    /// Show brief overview only
    brief: Option<bool>,
}

#[derive(Serialize)]
struct PrimeResponse {
    primer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<PrimeStats>,
}

#[derive(Serialize)]
struct PrimeStats {
    total_files: u64,
    total_chunks: u64,
    total_embeddings: u64,
    languages: Vec<PrimeLanguageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_indexed: Option<String>,
}

#[derive(Serialize)]
struct PrimeLanguageStats {
    language: String,
    file_count: u64,
    chunk_count: u64,
}

pub(super) async fn prime(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrimeParams>,
) -> Result<Json<PrimeResponse>, (StatusCode, Json<ErrorBody>)> {
    const PRIMER: &str = include_str!("../../docs/primer.md");

    let primer_text = if let Some(ref section) = params.section {
        extract_primer_section(PRIMER, section)
    } else if params.brief.unwrap_or(false) {
        extract_primer_brief(PRIMER)
    } else {
        PRIMER.to_string()
    };

    let stats = match open_vector_store(&state).await {
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
                last_indexed: s
                    .last_indexed
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|t| t.to_rfc3339())),
            }),
            Err(_) => None,
        },
        Err(_) => None,
    };

    Ok(Json(PrimeResponse {
        primer: primer_text,
        section: params.section,
        initialized: true,
        stats,
    }))
}

// -- /archive/search --

#[derive(Deserialize)]
struct ArchiveSearchParams {
    q: String,
    mode: Option<String>,
    limit: Option<usize>,
    after: Option<String>,
    before: Option<String>,
    channel: Option<String>,
}

#[derive(Serialize)]
struct ArchiveSearchResponse {
    query: String,
    mode: String,
    results: Vec<ArchiveResultItem>,
    total: usize,
}

#[derive(Serialize)]
struct ArchiveResultItem {
    id: String,
    content: String,
    source: String,
    timestamp: String,
    score: f32,
    file_path: String,
}

pub(super) async fn archive_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ArchiveSearchParams>,
) -> Result<Json<ArchiveSearchResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let mode = params.mode.as_deref().unwrap_or("hybrid");

    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    // Search with extra results to filter down
    let search_results = match mode {
        "keyword" => vector_store
            .search_fts(&params.q, limit * 3, None)
            .await
            .map_err(|e| internal_error(e.into()))?,
        "semantic" => {
            let mut search = SemanticSearch::new(embedder, vector_store);
            search
                .search(&params.q, limit * 3, None)
                .await
                .map_err(|e| internal_error(e.into()))?
        }
        _ => {
            let mut search =
                HybridSearch::new(embedder, vector_store, state.config.search.semantic_weight);
            search
                .search(&params.q, limit * 3, None)
                .await
                .map_err(|e| internal_error(e.into()))?
        }
    };

    // Filter to transcript chunks only
    let mut filtered: Vec<SearchResult> = search_results
        .into_iter()
        .filter(|r| r.chunk.language == "transcript")
        .collect();

    // Apply date filters on file_path (archive:YYYY/MM/DD/...)
    if let Some(ref after) = params.after {
        filtered.retain(|r| {
            extract_date_from_archive_path(&r.chunk.file_path)
                .is_some_and(|d| d.as_str() >= after.as_str())
        });
    }
    if let Some(ref before) = params.before {
        filtered.retain(|r| {
            extract_date_from_archive_path(&r.chunk.file_path)
                .is_some_and(|d| d.as_str() <= before.as_str())
        });
    }

    // Apply channel filter on chunk name (channel/id format)
    if let Some(ref channel) = params.channel {
        filtered.retain(|r| {
            r.chunk
                .name
                .as_ref()
                .is_some_and(|n| n.starts_with(&format!("{}/", channel)))
        });
    }

    filtered.truncate(limit);
    let total = filtered.len();

    let results: Vec<ArchiveResultItem> = filtered
        .iter()
        .map(|r| ArchiveResultItem {
            id: r.chunk.name.clone().unwrap_or_default(),
            content: r.chunk.content.clone(),
            source: r
                .chunk
                .name
                .as_ref()
                .and_then(|n| n.split('/').next())
                .unwrap_or("")
                .to_string(),
            timestamp: extract_date_from_archive_path(&r.chunk.file_path)
                .unwrap_or_default(),
            score: r.score,
            file_path: r.chunk.file_path.clone(),
        })
        .collect();

    Ok(Json(ArchiveSearchResponse {
        query: params.q,
        mode: mode.to_string(),
        results,
        total,
    }))
}

// -- /archive/entry/{id} --

#[derive(Serialize)]
struct ArchiveEntryResponse {
    id: String,
    content: String,
    source: String,
    file_path: String,
}

pub(super) async fn archive_entry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ArchiveEntryResponse>, (StatusCode, Json<ErrorBody>)> {
    if !state.config.archive.enabled || state.config.archive.archive_path.is_empty() {
        return Err(bad_request("Intent archive not configured".to_string()));
    }

    let archive_root = std::path::Path::new(&state.config.archive.archive_path);

    // Walk the archive to find the record by ID in filename
    let record = find_record_by_id(archive_root, &id);

    match record {
        Some((content, rel_path)) => {
            let source = content
                .lines()
                .find(|l| l.trim().starts_with("channel:"))
                .map(|l| l.trim().trim_start_matches("channel:").trim().to_string())
                .unwrap_or_default();

            // Extract body after frontmatter
            let body = extract_body(&content).unwrap_or_default();

            Ok(Json(ArchiveEntryResponse {
                id,
                content: body,
                source,
                file_path: format!("archive:{}", rel_path),
            }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("Record not found: {}", id),
            }),
        )),
    }
}

// -- /archive/recent --

#[derive(Deserialize)]
struct ArchiveRecentParams {
    after: String,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ArchiveRecentResponse {
    results: Vec<ArchiveResultItem>,
    total: usize,
}

pub(super) async fn archive_recent(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ArchiveRecentParams>,
) -> Result<Json<ArchiveRecentResponse>, (StatusCode, Json<ErrorBody>)> {
    if !state.config.archive.enabled || state.config.archive.archive_path.is_empty() {
        return Err(bad_request("Intent archive not configured".to_string()));
    }

    let limit = params.limit.unwrap_or(50);
    let archive_root = std::path::Path::new(&state.config.archive.archive_path);

    let mut records: Vec<(String, String, String)> = Vec::new(); // (id, content, rel_path)
    collect_recent_records(archive_root, archive_root, &params.after, &mut records);

    // Sort by path (date-partitioned, so lexicographic = chronological)
    records.sort_by(|a, b| a.2.cmp(&b.2));
    records.truncate(limit);

    let total = records.len();
    let results: Vec<ArchiveResultItem> = records
        .into_iter()
        .map(|(id, content, rel_path)| {
            let source = content
                .lines()
                .find(|l| l.trim().starts_with("channel:"))
                .map(|l| l.trim().trim_start_matches("channel:").trim().to_string())
                .unwrap_or_default();
            let body = extract_body(&content).unwrap_or_default();
            let timestamp =
                extract_date_from_archive_path(&format!("archive:{}", rel_path))
                    .unwrap_or_default();

            ArchiveResultItem {
                id,
                content: body,
                source,
                timestamp,
                score: 1.0,
                file_path: format!("archive:{}", rel_path),
            }
        })
        .collect();

    Ok(Json(ArchiveRecentResponse { results, total }))
}

/// Extract a date string from an archive path like "archive:2026/02/17/hi-xxx.md"
fn extract_date_from_archive_path(path: &str) -> Option<String> {
    let after_prefix = path.strip_prefix("archive:")?;
    // Path format: YYYY/MM/DD/filename.md
    let parts: Vec<&str> = after_prefix.splitn(4, '/').collect();
    if parts.len() >= 3 {
        Some(format!("{}-{}-{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Find a record file by ID (searches for filename containing the ID)
fn find_record_by_id(
    root: &std::path::Path,
    id: &str,
) -> Option<(String, String)> {
    find_record_recursive(root, root, id)
}

fn find_record_recursive(
    root: &std::path::Path,
    dir: &std::path::Path,
    id: &str,
) -> Option<(String, String)> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_record_recursive(root, &path, id) {
                return Some(found);
            }
        } else if path.file_stem().is_some_and(|s| s.to_string_lossy().contains(id)) {
            let content = std::fs::read_to_string(&path).ok()?;
            let rel = path.strip_prefix(root).ok()?;
            return Some((content, rel.to_string_lossy().to_string()));
        }
    }
    None
}

/// Collect archive records whose date path is >= the `after` date
fn collect_recent_records(
    root: &std::path::Path,
    dir: &std::path::Path,
    after: &str,
    results: &mut Vec<(String, String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recent_records(root, &path, after, results);
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = match path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().to_string(),
                Err(_) => continue,
            };
            let date = extract_date_from_archive_path(&format!("archive:{}", rel));
            if date.as_deref().is_some_and(|d| d >= after) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let id = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    results.push((id, content, rel));
                }
            }
        }
    }
}

/// Extract body text after YAML frontmatter
fn extract_body(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Some(content.to_string());
    }
    let close = trimmed[3..].find("\n---")?;
    let body_start = 3 + close + 4;
    let body = if body_start < trimmed.len() {
        trimmed[body_start..].trim()
    } else {
        ""
    };
    Some(body.to_string())
}

// -- /beads --

#[derive(Deserialize)]
struct SearchBeadsParams {
    /// Natural language search query
    q: String,
    /// Filter by priority (1-4)
    priority: Option<i32>,
    /// Filter by status
    status: Option<String>,
    /// Filter by assignee
    assignee: Option<String>,
    /// Filter by rig name
    rig: Option<String>,
    /// Filter by issue type (bug, task, feature, etc.)
    issue_type: Option<String>,
    /// Filter by label
    label: Option<String>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Enrich with live Dolt data (default true)
    enrich: Option<bool>,
    /// Compact mode - omit snippet (default true)
    compact: Option<bool>,
}

#[derive(Serialize)]
struct SearchBeadsResponse {
    query: String,
    count: usize,
    results: Vec<BeadResultItem>,
}

#[derive(Serialize)]
struct BeadResultItem {
    bead_id: String,
    title: String,
    priority: String,
    status: String,
    issue_type: String,
    assignee: String,
    owner: String,
    rig: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    relevance_score: f32,
    match_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
}

pub(super) async fn search_beads(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchBeadsParams>,
) -> Result<Json<SearchBeadsResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let should_enrich = params.enrich.unwrap_or(true);
    let compact = params.compact.unwrap_or(true);

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let model_dir = Config::model_cache_dir().map_err(|e| internal_error(e.into()))?;
    let embedder =
        Embedder::from_config(&state.config.embedding, &model_dir).map_err(internal_error)?;

    let mut search =
        HybridSearch::new(embedder, vector_store, state.config.search.semantic_weight);

    let search_results = search
        .search(&params.q, limit * 5, None)
        .await
        .map_err(|e| internal_error(e.into()))?;

    // Filter to only Issue chunks
    let mut filtered: Vec<SearchResult> = search_results
        .into_iter()
        .filter(|r| r.chunk.chunk_type == ChunkType::Issue)
        .collect();

    // Apply rig filter
    if let Some(ref rig) = params.rig {
        let prefix = format!("beads:{}:", rig);
        filtered.retain(|r| r.chunk.file_path.starts_with(&prefix));
    }

    // Fetch live metadata from Dolt
    let live_metadata = if should_enrich && state.config.beads.enabled {
        let bead_ids: Vec<(String, String)> = filtered
            .iter()
            .filter_map(|r| {
                let parts: Vec<&str> = r.chunk.file_path.splitn(3, ':').collect();
                if parts.len() == 3 {
                    Some((parts[1].to_string(), parts[2].to_string()))
                } else {
                    None
                }
            })
            .collect();

        crate::index::beads::fetch_bead_metadata(&state.config.beads, &bead_ids)
            .await
            .unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    // Apply all filters
    let has_filters = params.status.is_some() || params.priority.is_some()
        || params.assignee.is_some() || params.issue_type.is_some() || params.label.is_some();
    if has_filters {
        filtered.retain(|r| {
            let bead_id = r.chunk.file_path.split(':').nth(2).unwrap_or("");
            if let Some(meta) = live_metadata.get(bead_id) {
                if let Some(ref status) = params.status {
                    if meta.status != *status {
                        return false;
                    }
                }
                if let Some(priority) = params.priority {
                    if meta.priority != priority {
                        return false;
                    }
                }
                if let Some(ref assignee) = params.assignee {
                    let meta_assignee = meta.assignee.as_deref().unwrap_or("unassigned");
                    if !meta_assignee.contains(assignee.as_str()) {
                        return false;
                    }
                }
                if let Some(ref issue_type) = params.issue_type {
                    if meta.issue_type != *issue_type {
                        return false;
                    }
                }
                if let Some(ref label) = params.label {
                    if !meta.labels.iter().any(|l| l.contains(label.as_str())) {
                        return false;
                    }
                }
                true
            } else {
                let content = &r.chunk.content;
                if let Some(ref status) = params.status {
                    if !content.contains(&format!("Status: {}", status)) {
                        return false;
                    }
                }
                if let Some(priority) = params.priority {
                    if !content.contains(&format!("Priority: P{}", priority)) {
                        return false;
                    }
                }
                if let Some(ref assignee) = params.assignee {
                    if !content.contains(&format!("Assignee: {}", assignee)) {
                        return false;
                    }
                }
                true
            }
        });
    }

    // Boost relevance scores: title match and status weighting
    let query_lower = params.q.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
    for result in &mut filtered {
        let mut boost: f32 = 1.0;

        // Title match boost
        if let Some(ref name) = result.chunk.name {
            let title_lower = name.to_lowercase();
            let matching_terms = query_terms.iter()
                .filter(|t| title_lower.contains(**t))
                .count();
            if matching_terms > 0 {
                boost += 0.3 * (matching_terms as f32 / query_terms.len().max(1) as f32);
            }
        }

        // Status boost: open/in_progress are more actionable
        let bead_id = result.chunk.file_path.split(':').nth(2).unwrap_or("");
        if let Some(meta) = live_metadata.get(bead_id) {
            match meta.status.as_str() {
                "in_progress" | "hooked" => boost += 0.15,
                "open" | "blocked" => boost += 0.1,
                "closed" => boost -= 0.1,
                _ => {}
            }
        }

        result.score = (result.score * boost).min(1.0);
    }

    // Re-sort by boosted score
    filtered.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    filtered.truncate(limit);

    let results: Vec<BeadResultItem> = filtered
        .iter()
        .map(|r| {
            let parts: Vec<&str> = r.chunk.file_path.splitn(3, ':').collect();
            let rig = if parts.len() >= 2 { parts[1] } else { "" };
            let bead_id = if parts.len() == 3 { parts[2] } else { &r.chunk.file_path };

            let match_type = r.match_type.as_ref()
                .map(|mt| format!("{:?}", mt).to_lowercase())
                .unwrap_or_else(|| "hybrid".to_string());

            if let Some(meta) = live_metadata.get(bead_id) {
                let snippet = if compact {
                    None
                } else {
                    Some(clean_bead_snippet(&r.chunk.content, 200))
                };

                BeadResultItem {
                    bead_id: bead_id.to_string(),
                    title: meta.title.clone(),
                    priority: format!("P{}", meta.priority),
                    status: meta.status.clone(),
                    issue_type: meta.issue_type.clone(),
                    assignee: meta.assignee.clone().unwrap_or_else(|| "unassigned".to_string()),
                    owner: meta.owner.clone(),
                    rig: rig.to_string(),
                    labels: meta.labels.clone(),
                    created_at: meta.created_at.clone(),
                    relevance_score: r.score,
                    match_type,
                    snippet,
                }
            } else {
                let content = &r.chunk.content;
                let snippet = if compact {
                    None
                } else {
                    Some(clean_bead_snippet(content, 200))
                };

                BeadResultItem {
                    bead_id: bead_id.to_string(),
                    title: r.chunk.name.clone().unwrap_or_default(),
                    priority: extract_bead_field(content, "Priority: "),
                    status: extract_bead_field(content, "Status: "),
                    issue_type: "task".to_string(),
                    assignee: extract_bead_field(content, "Assignee: "),
                    owner: String::new(),
                    rig: rig.to_string(),
                    labels: Vec::new(),
                    created_at: None,
                    relevance_score: r.score,
                    match_type,
                    snippet,
                }
            }
        })
        .collect();

    Ok(Json(SearchBeadsResponse {
        query: params.q,
        count: results.len(),
        results,
    }))
}

// -- Helpers --

fn bad_request(msg: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::BAD_REQUEST, Json(ErrorBody { error: msg }))
}

async fn open_vector_store(state: &AppState) -> anyhow::Result<VectorStore> {
    let lance_path = Config::lance_path(&state.repo_root);
    VectorStore::open(&lance_path).await
}

fn parse_chunk_type(s: &str) -> anyhow::Result<crate::types::ChunkType> {
    use crate::types::ChunkType;
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
        "issue" | "bead" => Ok(ChunkType::Issue),
        "other" => Ok(ChunkType::Other),
        _ => anyhow::bail!("Unknown chunk type: {}", s),
    }
}

fn to_search_item(r: &crate::types::SearchResult) -> SearchResultItem {
    SearchResultItem {
        file_path: r.chunk.file_path.clone(),
        name: r.chunk.name.clone(),
        chunk_type: r.chunk.chunk_type.to_string(),
        start_line: r.chunk.start_line,
        end_line: r.chunk.end_line,
        score: r.score,
        match_type: r.match_type.map(|mt| match mt {
            MatchType::Semantic => "semantic".to_string(),
            MatchType::Keyword => "keyword".to_string(),
            MatchType::Hybrid => "hybrid".to_string(),
        }),
        language: r.chunk.language.clone(),
        content_preview: truncate(&r.chunk.content, 300),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{}...", t.trim_end())
    }
}

fn open_metadata_store(state: &AppState) -> anyhow::Result<MetadataStore> {
    let db_path = Config::db_path(&state.repo_root);
    MetadataStore::open(&db_path)
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
    repo_root: &std::path::Path,
    file: &str,
    start: u32,
    end: u32,
    context: u32,
) -> anyhow::Result<(String, u32, u32)> {
    let file_path = repo_root.join(file);
    if !file_path.exists() {
        anyhow::bail!("File not found: {}", file);
    }

    let content = std::fs::read_to_string(&file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u32;

    let actual_start = start.saturating_sub(context).max(1);
    let actual_end = (end + context).min(total_lines);

    let start_idx = (actual_start - 1) as usize;
    let end_idx = actual_end as usize;

    let selected = if end_idx <= lines.len() {
        lines[start_idx..end_idx].join("\n")
    } else {
        lines[start_idx..].join("\n")
    };

    Ok((selected, actual_start, actual_end))
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

fn to_context_file(f: &crate::search::context::ContextFile) -> ContextFileOutput {
    ContextFileOutput {
        path: f.path.clone(),
        language: f.language.clone(),
        relevance: match f.relevance {
            FileRelevance::Direct => "direct".to_string(),
            FileRelevance::Coupled => "coupled".to_string(),
            FileRelevance::Bridged => "bridged".to_string(),
        },
        score: f.score,
        coupled_to: f.coupled_to.clone(),
        chunks: f
            .chunks
            .iter()
            .map(|c| ContextChunkOutput {
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
            })
            .collect(),
    }
}

fn to_context_summary(s: &crate::search::context::ContextSummary) -> ContextSummaryOutput {
    ContextSummaryOutput {
        total_files: s.total_files,
        total_chunks: s.total_chunks,
        direct_hits: s.direct_hits,
        coupled_additions: s.coupled_additions,
        bridged_additions: s.bridged_additions,
        source_files: s.source_files,
        doc_files: s.doc_files,
    }
}

fn parse_diff_spec(spec: Option<&str>) -> crate::index::git::DiffSpec {
    use crate::index::git::DiffSpec;
    match spec {
        None | Some("unstaged") | Some("") => DiffSpec::Unstaged,
        Some("staged") => DiffSpec::Staged,
        Some(s) if s.starts_with("branch:") => DiffSpec::Branch(s[7..].to_string()),
        Some(range) => DiffSpec::Range(range.to_string()),
    }
}

fn describe_diff_spec(spec: &crate::index::git::DiffSpec) -> String {
    use crate::index::git::DiffSpec;
    match spec {
        DiffSpec::Unstaged => "unstaged changes".to_string(),
        DiffSpec::Staged => "staged changes".to_string(),
        DiffSpec::Branch(b) => format!("branch: {}", b),
        DiffSpec::Range(r) => format!("range: {}", r),
    }
}

fn parse_similar_target(s: &str) -> SimilarTarget {
    if let Some(colon_pos) = s.find(':') {
        let before = &s[..colon_pos];
        if before.contains('.') || before.contains('/') {
            return SimilarTarget::ChunkRef(s.to_string());
        }
    }
    SimilarTarget::Text(s.to_string())
}

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

fn extract_bead_field(content: &str, prefix: &str) -> String {
    content
        .lines()
        .find(|line| line.contains(prefix))
        .and_then(|line| {
            let start = line.find(prefix)? + prefix.len();
            let rest = &line[start..];
            let end = rest.find(" | ").unwrap_or(rest.len());
            Some(rest[..end].trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Clean a bead snippet by removing metadata lines already in structured fields.
fn clean_bead_snippet(content: &str, max_len: usize) -> String {
    let cleaned: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("Status: ") && trimmed.contains(" | "))
                && !trimmed.starts_with("Priority: P")
                && !trimmed.starts_with("Assignee: ")
                && !trimmed.starts_with("Comments:")
                && !trimmed.starts_with("--- ")
                && !trimmed.starts_with("Notes:")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let trimmed = cleaned.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &trimmed[..end])
    }
}

/// Run incremental indexing (used by webhook handler)
async fn run_incremental_index(
    repo_root: PathBuf,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    use crate::index::{Embedder, Parser};
    use ignore::WalkBuilder;
    use sha2::{Digest, Sha256};
    use std::path::Path;

    let source_dir = &repo_root;
    let lance_path = Config::lance_path(&repo_root);
    let mut vector_store = VectorStore::open(&lance_path).await?;

    let model_dir = Config::model_cache_dir()?;
    let mut embedder = Embedder::load(&model_dir, &config.embedding.model)?;
    let mut parser = Parser::new()?;

    // Collect files
    let mut walker = WalkBuilder::new(source_dir);
    walker.hidden(true).git_ignore(config.index.use_gitignore);

    let include_globs: Vec<glob::Pattern> = config
        .index
        .include
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();
    let exclude_globs: Vec<glob::Pattern> = config
        .index
        .exclude
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();

    let mut files_to_index = Vec::new();
    for entry in walker.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel_path = path
            .strip_prefix(source_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let matches_include = include_globs.is_empty()
            || include_globs.iter().any(|g| g.matches(&rel_path));
        let matches_exclude = exclude_globs.iter().any(|g| g.matches(&rel_path));

        if matches_include && !matches_exclude {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
            let needs = vector_store
                .needs_reindex(&rel_path, &hash)
                .await
                .unwrap_or(true);
            if needs {
                files_to_index.push((rel_path, content, hash));
            }
        }
    }

    tracing::info!(
        "Webhook index: {} files need re-indexing",
        files_to_index.len()
    );

    for (rel_path, content, hash) in &files_to_index {
        let chunks = match parser.parse_file(Path::new(rel_path), content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", rel_path, e);
                continue;
            }
        };
        if chunks.is_empty() {
            continue;
        }

        vector_store.delete_by_file(&[rel_path.clone()]).await?;

        let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings = embedder.embed_batch(&texts).await?;
        let contexts: Vec<Option<String>> = vec![None; chunks.len()];
        let now = chrono::Utc::now().to_rfc3339();

        vector_store
            .insert(&chunks, &embeddings, &contexts, "default", hash, &now)
            .await?;
    }

    Ok(())
}
