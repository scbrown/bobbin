//! Analysis handlers: related, refs, symbols, hotspots, impact.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::analysis::backend::{IndexBackend, StructuralBackend};

use super::{
    bad_request, internal_error, open_metadata_store, open_vector_store, AppState, ErrorBody,
};

mod hotspots;
pub(super) use hotspots::*;

// ---------------------------------------------------------------------------
// /related
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct RelatedParams {
    /// File path to find related files for
    file: String,
    /// Seed file's repo (disambiguates the cross-repo lookup in a shared store)
    repo: Option<String>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Min coupling score threshold (default 0.0)
    threshold: Option<f32>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct RelatedResponse {
    file: String,
    related: Vec<RelatedFile>,
}

#[derive(Serialize)]
pub(super) struct RelatedFile {
    path: String,
    score: f32,
    co_changes: u32,
    /// Repo the file lives in — set only for cross-repo coupled files (bo-oqny).
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
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

    let access = super::resolve_filter(&state, params.role.as_deref());
    let mut related: Vec<RelatedFile> = couplings
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
                repo: None,
            }
        })
        .filter(|r| access.is_path_allowed(&r.path))
        .collect();

    // Cross-repo coupled files (bo-oqny) — access-filtered inside the helper.
    let cross = crate::index::cross_repo::related_cross_repo(
        &store,
        params.repo.as_deref(),
        &params.file,
        limit,
        threshold,
        &access,
    )
    .map_err(internal_error)?;
    related.extend(cross.into_iter().map(|c| RelatedFile {
        path: c.path,
        score: c.score,
        co_changes: c.co_changes,
        repo: Some(c.repo),
    }));
    related.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    related.truncate(limit);

    Ok(Json(RelatedResponse {
        file: params.file,
        related,
    }))
}

// ---------------------------------------------------------------------------
// /refs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct FindRefsParams {
    /// Symbol name to find references for
    symbol: String,
    /// Filter by symbol type
    r#type: Option<String>,
    /// Max usage results (default 20)
    limit: Option<usize>,
    /// Filter by repository
    repo: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct FindRefsResponse {
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<SymbolDefinitionOutput>,
    usage_count: usize,
    usages: Vec<SymbolUsageOutput>,
}

#[derive(Serialize)]
pub(super) struct SymbolDefinitionOutput {
    name: String,
    chunk_type: String,
    file_path: String,
    start_line: u32,
    end_line: u32,
    signature: String,
}

#[derive(Serialize)]
pub(super) struct SymbolUsageOutput {
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
    let mut backend = IndexBackend::new(&mut vector_store);
    let refs = backend
        .find_refs(
            &params.symbol,
            params.r#type.as_deref(),
            limit,
            params.repo.as_deref(),
        )
        .await
        .map_err(internal_error)?;

    let access = super::resolve_filter(&state, params.role.as_deref());

    // Filter definition if it's in a denied repo
    let definition = refs.definition.and_then(|d| {
        if access.is_path_allowed(&d.file_path) {
            Some(SymbolDefinitionOutput {
                name: d.name,
                chunk_type: d.chunk_type.to_string(),
                file_path: d.file_path,
                start_line: d.start_line,
                end_line: d.end_line,
                signature: d.signature,
            })
        } else {
            None
        }
    });

    let usages: Vec<SymbolUsageOutput> = refs
        .usages
        .iter()
        .filter(|u| access.is_path_allowed(&u.file_path))
        .map(|u| SymbolUsageOutput {
            file_path: u.file_path.clone(),
            line: u.line,
            context: u.context.clone(),
        })
        .collect();

    Ok(Json(FindRefsResponse {
        symbol: params.symbol,
        definition,
        usage_count: usages.len(),
        usages,
    }))
}

// ---------------------------------------------------------------------------
// /symbols
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ListSymbolsParams {
    /// File path (relative to repo root)
    file: String,
    /// Filter by repository
    repo: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ListSymbolsResponse {
    file: String,
    count: usize,
    symbols: Vec<SymbolItemOutput>,
}

#[derive(Serialize)]
pub(super) struct SymbolItemOutput {
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
    use crate::access::RepoFilter;

    // Check role-based access for the file's repo
    let access = super::resolve_filter(&state, params.role.as_deref());
    let repo_name = params
        .repo
        .as_deref()
        .unwrap_or_else(|| RepoFilter::repo_from_path(&params.file));
    if !access.is_allowed(repo_name) {
        return Err(bad_request(format!("Repo not accessible: {}", repo_name)));
    }

    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let mut backend = IndexBackend::new(&mut vector_store);
    let file_symbols = backend
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
