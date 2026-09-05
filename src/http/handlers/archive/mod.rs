//! Archive and beads search handlers.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::search::{HybridSearch, SemanticSearch};
use crate::types::SearchResult;

use super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};

mod beads;
mod helpers;

pub(super) use beads::*;
use helpers::*;

// ---------------------------------------------------------------------------
// /archive/search
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ArchiveSearchParams {
    q: String,
    mode: Option<String>,
    limit: Option<usize>,
    after: Option<String>,
    before: Option<String>,
    /// Filter by name_field value (e.g., channel name for HLA, agent name for Pensieve)
    #[serde(rename = "filter")]
    name_filter: Option<String>,
    /// Filter by archive source name (e.g., "hla", "pensieve")
    source: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ArchiveSearchResponse {
    query: String,
    mode: String,
    results: Vec<ArchiveResultItem>,
    total: usize,
}

#[derive(Serialize)]
pub(super) struct ArchiveResultItem {
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

    // Collect valid archive source names for filtering
    let archive_languages: Vec<String> = archive_source_names(&state.config.archive);
    if archive_languages.is_empty() {
        return Err(bad_request("No archive sources configured".to_string()));
    }

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

    // Build a SQL filter to search only archive-language chunks directly in LanceDB.
    // Archive records may be indexed as language='archive' (new format) or as their
    // source name (e.g., 'hla', 'pensieve') for backward compatibility.
    let mut all_langs = archive_languages.clone();
    if !all_langs.contains(&"archive".to_string()) {
        all_langs.push("archive".to_string());
    }

    // When source filter is active, narrow the SQL language filter to just that source
    // (plus "archive" for dual-indexed records). This prevents the search limit from
    // being consumed by results from other sources before post-filtering.
    let search_langs = if let Some(ref source) = params.source {
        let mut langs = vec![source.clone()];
        if !langs.contains(&"archive".to_string()) {
            langs.push("archive".to_string());
        }
        langs
    } else {
        all_langs.clone()
    };

    let lang_filter = if search_langs.len() == 1 {
        format!("language = '{}'", search_langs[0].replace('\'', "''"))
    } else {
        let quoted: Vec<String> = search_langs
            .iter()
            .map(|l| format!("'{}'", l.replace('\'', "''")))
            .collect();
        format!("language IN ({})", quoted.join(", "))
    };
    let lang_filter_ref = lang_filter.as_str();

    // Search with language filter pushed into LanceDB query
    let search_results = match mode {
        "keyword" => vector_store
            .search_fts_filtered(&params.q, limit, None, Some(lang_filter_ref))
            .await
            .map_err(|e| internal_error(e.into()))?,
        "semantic" => {
            let mut search = SemanticSearch::new(embedder, vector_store);
            search
                .search_filtered(&params.q, limit, None, Some(lang_filter_ref))
                .await
                .map_err(|e| internal_error(e.into()))?
        }
        _ => {
            let mut search =
                HybridSearch::new(embedder, vector_store, state.config.search.semantic_weight);
            search
                .search_filtered(&params.q, limit, None, Some(lang_filter_ref))
                .await
                .map_err(|e| internal_error(e.into()))?
        }
    };

    // Post-filter by language (redundant safety check — LanceDB filter should handle this)
    let mut filtered: Vec<SearchResult> = search_results
        .into_iter()
        .filter(|r| all_langs.contains(&r.chunk.language))
        .collect();

    // Apply source filter (e.g., source=hla to only get HLA results).
    // Check both the language field (old format: language='hla') and the
    // file_path prefix (new format: language='archive', path='hla:...')
    if let Some(ref source) = params.source {
        let prefix = format!("{}:", source);
        filtered.retain(|r| &r.chunk.language == source || r.chunk.file_path.starts_with(&prefix));
    }

    // Apply date filters on file_path ({source}:YYYY/MM/DD/...)
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

    // Apply name_field filter on chunk name (e.g., "telegram/" or "aegis/crew/arnold/")
    if let Some(ref name_filter) = params.name_filter {
        filtered.retain(|r| {
            r.chunk
                .name
                .as_ref()
                .is_some_and(|n| n.starts_with(&format!("{}/", name_filter)))
        });
    }

    // Content-based dedup: HLA and pensieve often capture the same message
    // multiple times with different IDs. Keep the highest-scoring version.
    // Dedup on BODY (after frontmatter extraction) since duplicate records
    // have different frontmatter (IDs, timestamps, agents) but identical bodies.
    {
        let mut seen_content = std::collections::HashSet::new();
        filtered.retain(|r| {
            // Strip frontmatter before comparing — duplicates differ only in metadata
            let body = extract_body(&r.chunk.content).unwrap_or_default();
            let key = body.trim().to_lowercase();
            // Truncate at a char boundary (avoids UTF-8 panic on multi-byte chars)
            let end = if key.len() > 200 {
                let mut i = 200;
                while i > 0 && !key.is_char_boundary(i) {
                    i -= 1;
                }
                i
            } else {
                key.len()
            };
            let dedup_key = &key[..end];
            seen_content.insert(dedup_key.to_string())
        });
    }

    filtered.truncate(limit);
    let total = filtered.len();

    let results: Vec<ArchiveResultItem> = filtered
        .iter()
        .map(|r| ArchiveResultItem {
            id: r.chunk.name.clone().unwrap_or_default(),
            content: r.chunk.content.clone(),
            source: r.chunk.language.clone(),
            timestamp: extract_date_from_archive_path(&r.chunk.file_path).unwrap_or_default(),
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

// ---------------------------------------------------------------------------
// /archive/entry/{id}
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct ArchiveEntryResponse {
    id: String,
    content: String,
    source: String,
    file_path: String,
}

pub(super) async fn archive_entry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ArchiveEntryResponse>, (StatusCode, Json<ErrorBody>)> {
    if !state.config.archive.enabled {
        return Err(bad_request("Archive not configured".to_string()));
    }

    // Search all configured source paths for the record
    let paths = archive_source_paths(&state.config.archive);
    if paths.is_empty() {
        return Err(bad_request("No archive sources configured".to_string()));
    }

    for (source_name, source_path) in &paths {
        let archive_root = std::path::Path::new(source_path);
        if let Some((content, rel_path)) = find_record_by_id(archive_root, &id) {
            let body = extract_body(&content).unwrap_or_default();
            return Ok(Json(ArchiveEntryResponse {
                id,
                content: body,
                source: source_name.clone(),
                file_path: format!("{}:{}", source_name, rel_path),
            }));
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Record not found: {}", id),
        }),
    ))
}

// ---------------------------------------------------------------------------
// /archive/recent
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ArchiveRecentParams {
    /// Only return records after this date (YYYY-MM-DD). Defaults to 30 days ago.
    after: Option<String>,
    limit: Option<usize>,
    /// Filter by archive source name (e.g., "hla", "pensieve")
    source: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ArchiveRecentResponse {
    results: Vec<ArchiveResultItem>,
    total: usize,
}

pub(super) async fn archive_recent(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ArchiveRecentParams>,
) -> Result<Json<ArchiveRecentResponse>, (StatusCode, Json<ErrorBody>)> {
    if !state.config.archive.enabled {
        return Err(bad_request("Archive not configured".to_string()));
    }
    if archive_source_paths(&state.config.archive).is_empty() {
        return Err(bad_request("No archive sources configured".to_string()));
    }

    let records = crate::index::archive::collect_recent(
        &state.config.archive,
        params.after.as_deref(),
        params.source.as_deref(),
        params.limit.unwrap_or(50),
    );

    let total = records.len();
    let results: Vec<ArchiveResultItem> = records
        .into_iter()
        .map(|r| ArchiveResultItem {
            id: r.id,
            content: r.body,
            source: r.source,
            timestamp: r.timestamp,
            score: 1.0,
            file_path: r.file_path,
        })
        .collect();

    Ok(Json(ArchiveRecentResponse { results, total }))
}
