//! Archive and beads search handlers.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::search::{HybridSearch, SemanticSearch};
use crate::types::{ChunkType, SearchResult};

use super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};

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
        filtered.retain(|r| {
            &r.chunk.language == source || r.chunk.file_path.starts_with(&prefix)
        });
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
                while i > 0 && !key.is_char_boundary(i) { i -= 1; }
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

    let limit = params.limit.unwrap_or(50);
    let paths = archive_source_paths(&state.config.archive);
    if paths.is_empty() {
        return Err(bad_request("No archive sources configured".to_string()));
    }

    // (source_name, id, content, rel_path, sort_date)
    let mut records: Vec<(String, String, String, String, String)> = Vec::new();

    for (source_name, source_path) in &paths {
        // Apply source filter early
        if let Some(ref filter) = params.source {
            if source_name != filter {
                continue;
            }
        }
        let archive_root = std::path::Path::new(source_path);
        let mut source_records: Vec<(String, String, String)> = Vec::new();
        // Default to 30 days ago if no after date provided
        let default_after = {
            let now = chrono::Utc::now();
            let thirty_days_ago = now - chrono::Duration::days(30);
            thirty_days_ago.format("%Y-%m-%d").to_string()
        };
        let after = params.after.as_deref().unwrap_or(&default_after);
        collect_recent_records(archive_root, archive_root, after, &mut source_records);
        for (id, content, rel_path) in source_records {
            // Extract sort date: prefer frontmatter timestamp, fallback to path date
            let sort_date = extract_timestamp_from_frontmatter(&content)
                .or_else(|| extract_date_from_archive_path(&format!("_:{}", rel_path)))
                .unwrap_or_default();
            records.push((source_name.clone(), id, content, rel_path, sort_date));
        }
    }

    // Sort by extracted date descending (newest first), then by path for ties
    records.sort_by(|a, b| b.4.cmp(&a.4).then_with(|| b.3.cmp(&a.3)));

    // Content-based dedup: same design doc often stored by multiple agents.
    // Dedup on BODY (after frontmatter extraction) since duplicate records
    // have different frontmatter (IDs, timestamps, agents) but identical bodies.
    {
        let mut seen_content = std::collections::HashSet::new();
        records.retain(|(_, _, content, _, _)| {
            let body = extract_body(content).unwrap_or_default();
            let key = body.trim().to_lowercase();
            // Truncate at a char boundary (floor_char_boundary avoids UTF-8 panic)
            let end = if key.len() > 200 {
                // Find the last char boundary at or before byte 200
                let mut i = 200;
                while i > 0 && !key.is_char_boundary(i) { i -= 1; }
                i
            } else {
                key.len()
            };
            let dedup_key = &key[..end];
            seen_content.insert(dedup_key.to_string())
        });
    }

    records.truncate(limit);

    let total = records.len();
    let results: Vec<ArchiveResultItem> = records
        .into_iter()
        .map(|(source_name, id, content, rel_path, sort_date)| {
            let body = extract_body(&content).unwrap_or_default();
            let prefixed_path = format!("{}:{}", source_name, rel_path);
            // Use the already-extracted sort_date for timestamp
            let timestamp = if sort_date.is_empty() {
                extract_date_from_archive_path(&prefixed_path).unwrap_or_default()
            } else {
                sort_date
            };

            ArchiveResultItem {
                id,
                content: body,
                source: source_name,
                timestamp,
                score: 1.0,
                file_path: prefixed_path,
            }
        })
        .collect();

    Ok(Json(ArchiveRecentResponse { results, total }))
}

// ---------------------------------------------------------------------------
// /beads
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct SearchBeadsParams {
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
pub(super) struct SearchBeadsResponse {
    query: String,
    count: usize,
    results: Vec<BeadResultItem>,
}

#[derive(Serialize)]
pub(super) struct BeadResultItem {
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

    let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

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

// ---------------------------------------------------------------------------
// Archive helpers
// ---------------------------------------------------------------------------

/// Extract a date string from an archive path like "{source}:YYYY/MM/DD/..."
///
/// Handles any source prefix (hla:, pensieve:, archive:, etc.)
fn extract_date_from_archive_path(path: &str) -> Option<String> {
    // Strip the source prefix (everything before and including ':')
    let after_prefix = path.split_once(':').map(|(_, rest)| rest)?;
    // Path format: YYYY/MM/DD/filename.md
    let parts: Vec<&str> = after_prefix.splitn(4, '/').collect();
    if parts.len() >= 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
    {
        Some(format!("{}-{}-{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Get the list of archive source names (language tags) from config.
fn archive_source_names(config: &crate::config::ArchiveConfig) -> Vec<String> {
    config.sources.iter().map(|s| s.name.clone()).collect()
}

/// Get (source_name, source_path) pairs from config.
fn archive_source_paths(config: &crate::config::ArchiveConfig) -> Vec<(String, String)> {
    config
        .sources
        .iter()
        .filter(|s| !s.path.is_empty())
        .map(|s| (s.name.clone(), s.path.clone()))
        .collect()
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

/// Collect archive records whose date is >= the `after` date.
///
/// Date is extracted from the path if it's date-partitioned (YYYY/MM/DD/...),
/// otherwise falls back to parsing the `timestamp:` field from YAML frontmatter.
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
            // Skip underscore-prefixed directories (_plans/, _templates/, etc.)
            // These contain static design docs, not time-series observations.
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('_') {
                continue;
            }
            collect_recent_records(root, &path, after, results);
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = match path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Try date from path first (cheap)
            let date = extract_date_from_archive_path(&format!("_:{}", rel));

            if let Some(ref d) = date {
                // Path has a date — filter without reading file
                if d.as_str() >= after {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let id = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        results.push((id, content, rel));
                    }
                }
            } else {
                // No date in path — read file and check frontmatter timestamp
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let fm_date = extract_timestamp_from_frontmatter(&content);
                    if fm_date.as_deref().is_some_and(|d| d >= after) {
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
}

/// Extract a YYYY-MM-DD date from the `timestamp:` field in YAML frontmatter.
fn extract_timestamp_from_frontmatter(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let end = trimmed[3..].find("\n---")?;
    let fm = &trimmed[3..3 + end];
    for line in fm.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("timestamp:") {
            let ts = val.trim();
            if ts.len() >= 10 {
                return Some(ts[..10].to_string());
            }
        }
    }
    None
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
