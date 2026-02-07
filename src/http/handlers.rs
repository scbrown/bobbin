//! HTTP request handlers for the Bobbin REST API.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::index::Embedder;
use crate::search::{HybridSearch, SemanticSearch};
use crate::storage::VectorStore;
use crate::types::MatchType;

use super::AppState;

/// Build the axum router with all routes
pub(super) fn router(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post};
    use tower_http::cors::CorsLayer;
    use tower_http::trace::TraceLayer;

    axum::Router::new()
        .route("/search", get(search))
        .route("/chunk/{id}", get(get_chunk))
        .route("/status", get(status))
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
}

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
    }))
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

// -- Helpers --

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
        let embeddings = embedder.embed_batch(&texts)?;
        let contexts: Vec<Option<String>> = vec![None; chunks.len()];
        let now = chrono::Utc::now().to_rfc3339();

        vector_store
            .insert(&chunks, &embeddings, &contexts, "default", hash, &now)
            .await?;
    }

    Ok(())
}
