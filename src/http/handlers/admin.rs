//! Admin handlers: status, healthz, metrics, prime, suggest, repos, groups, files.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{
    bad_request, detect_language, internal_error, open_vector_store, AppState, ErrorBody,
};

// ---------------------------------------------------------------------------
// /healthz
// ---------------------------------------------------------------------------

pub(super) async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// /status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct StatusResponse {
    status: String,
    index: crate::types::IndexStats,
    sources: crate::config::SourcesConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo_path_prefix: Option<String>,
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
        sources: state.resolved_sources.clone(),
        repo_path_prefix: state.config.server.repo_path_prefix.clone(),
    }))
}

// ---------------------------------------------------------------------------
// /metrics
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// /repos
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ReposParams {
    /// Filter by named repo group
    group: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ReposListResponse {
    count: usize,
    repos: Vec<RepoSummary>,
}

#[derive(Serialize)]
pub(super) struct RepoSummary {
    name: String,
    file_count: u64,
    chunk_count: u64,
    languages: Vec<crate::types::LanguageStats>,
}

pub(super) async fn list_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ReposParams>,
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

    // Apply role-based access filtering
    let access = super::resolve_filter(&state, params.role.as_deref());
    summaries.retain(|r| access.is_allowed(&r.name));

    // Apply group filtering
    if let Some(ref group_name) = params.group {
        if let Some(group_repos) = state.config.resolve_group(group_name) {
            summaries.retain(|r| group_repos.iter().any(|g| g == &r.name));
        } else {
            return Err(bad_request(format!("Unknown group '{}'", group_name)));
        }
    }

    // Sort by chunk count descending
    summaries.sort_by(|a, b| b.chunk_count.cmp(&a.chunk_count));

    Ok(Json(ReposListResponse {
        count: summaries.len(),
        repos: summaries,
    }))
}

// ---------------------------------------------------------------------------
// /groups
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct GroupsResponse {
    count: usize,
    groups: Vec<GroupItem>,
}

#[derive(Serialize)]
pub(super) struct GroupItem {
    name: String,
    repos: Vec<String>,
}

pub(super) async fn list_groups(
    State(state): State<Arc<AppState>>,
) -> Json<GroupsResponse> {
    let groups: Vec<GroupItem> = state
        .config
        .groups
        .iter()
        .map(|g| GroupItem {
            name: g.name.clone(),
            repos: g.repos.clone(),
        })
        .collect();

    Json(GroupsResponse {
        count: groups.len(),
        groups,
    })
}

// ---------------------------------------------------------------------------
// /repos/{name}/files
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct RepoFilesParams {
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct RepoFilesResponse {
    repo: String,
    count: usize,
    files: Vec<RepoFileItem>,
}

#[derive(Serialize)]
pub(super) struct RepoFileItem {
    path: String,
    language: String,
}

pub(super) async fn list_repo_files(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<RepoFilesParams>,
) -> Result<Json<RepoFilesResponse>, (StatusCode, Json<ErrorBody>)> {
    // Check role-based access for this repo
    let access = super::resolve_filter(&state, params.role.as_deref());
    if !access.is_allowed(&name) {
        return Err(bad_request(format!("Repo not accessible: {}", name)));
    }

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

// ---------------------------------------------------------------------------
// /prime
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct PrimeParams {
    /// Specific section to show
    section: Option<String>,
    /// Show brief overview only
    brief: Option<bool>,
}

#[derive(Serialize)]
pub(super) struct PrimeResponse {
    primer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<PrimeStats>,
}

#[derive(Serialize)]
pub(super) struct PrimeStats {
    total_files: u64,
    total_chunks: u64,
    total_embeddings: u64,
    languages: Vec<PrimeLanguageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_indexed: Option<String>,
}

#[derive(Serialize)]
pub(super) struct PrimeLanguageStats {
    language: String,
    file_count: u64,
    chunk_count: u64,
}

pub(super) async fn prime(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrimeParams>,
) -> Result<Json<PrimeResponse>, (StatusCode, Json<ErrorBody>)> {
    const PRIMER: &str = include_str!("../../../docs/primer.md");

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

// ---------------------------------------------------------------------------
// /suggest
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct SuggestParams {
    /// Filter field to suggest values for: repo, lang, type, group, tag
    field: String,
    /// Optional prefix to filter suggestions
    #[serde(default)]
    q: Option<String>,
    /// Role for access filtering
    #[serde(default)]
    role: Option<String>,
    /// Max results (default 20)
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
pub(super) struct SuggestResponse {
    field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    count: usize,
    values: Vec<String>,
}

pub(super) async fn suggest(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SuggestParams>,
) -> Result<Json<SuggestResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(20);
    let prefix = params.q.as_deref().unwrap_or("").to_lowercase();

    let mut values: Vec<String> = match params.field.as_str() {
        "repo" => {
            let store = open_vector_store(&state).await.map_err(internal_error)?;
            let repos = store
                .get_all_repos()
                .await
                .map_err(|e| internal_error(e.into()))?;
            let access = super::resolve_filter(&state, params.role.as_deref());
            repos.into_iter().filter(|r| access.is_allowed(r)).collect()
        }
        "lang" | "language" => {
            let store = open_vector_store(&state).await.map_err(internal_error)?;
            store
                .get_all_languages()
                .await
                .map_err(|e| internal_error(e.into()))?
        }
        "type" => {
            vec![
                "function", "method", "class", "struct", "enum", "interface",
                "module", "impl", "trait", "doc", "section", "table",
                "code_block", "commit", "issue", "other",
            ]
            .into_iter()
            .map(String::from)
            .collect()
        }
        "group" => {
            state.config.groups.iter().map(|g| g.name.clone()).collect()
        }
        "tag" => {
            let store = open_vector_store(&state).await.map_err(internal_error)?;
            let counts = store
                .get_tag_counts()
                .await
                .map_err(|e| internal_error(e.into()))?;
            counts.into_iter().map(|(tag, _)| tag).collect()
        }
        _ => {
            return Err(bad_request(format!(
                "Unknown field '{}'. Use: repo, lang, type, group, tag",
                params.field
            )));
        }
    };

    // Apply prefix filter
    if !prefix.is_empty() {
        values.retain(|v| v.to_lowercase().starts_with(&prefix));
    }
    values.truncate(limit);

    Ok(Json(SuggestResponse {
        field: params.field,
        query: params.q,
        count: values.len(),
        values,
    }))
}

// ---------------------------------------------------------------------------
// Primer helpers
// ---------------------------------------------------------------------------

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
