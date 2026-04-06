//! Tags and bundles handlers.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::tags::{BundleConfig, BundleRef, RefTarget, TagsConfig};

use super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};

// ---------------------------------------------------------------------------
// /tags
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct TagsParams {
    /// Filter: show files for a specific tag
    tag: Option<String>,
    /// Filter: show tags for a specific file
    file: Option<String>,
}

#[derive(Serialize)]
pub(super) struct TagsResponse {
    /// Tag name → chunk count (when listing all or filtering by file)
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<TagCount>>,
    /// File paths (when filtering by tag)
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<String>>,
    /// Total tagged / untagged chunks
    tagged_chunks: u64,
    untagged_chunks: u64,
}

#[derive(Serialize)]
pub(super) struct TagCount {
    tag: String,
    count: usize,
}

pub(super) async fn tags(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TagsParams>,
) -> Result<Json<TagsResponse>, (StatusCode, Json<ErrorBody>)> {
    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let (tagged, untagged) = vector_store
        .count_tagged_chunks()
        .await
        .map_err(|e| internal_error(e.into()))?;

    if let Some(ref tag) = params.tag {
        // Return files that have this tag
        let files = vector_store
            .get_files_by_tag(tag)
            .await
            .map_err(|e| internal_error(e.into()))?;

        return Ok(Json(TagsResponse {
            tags: None,
            files: Some(files),
            tagged_chunks: tagged,
            untagged_chunks: untagged,
        }));
    }

    // Default: return all tag counts
    let counts = vector_store
        .get_tag_counts()
        .await
        .map_err(|e| internal_error(e.into()))?;

    let tag_counts: Vec<TagCount> = counts
        .into_iter()
        .map(|(tag, count)| TagCount { tag, count })
        .collect();

    // If ?file= filter, narrow to tags on that file's chunks
    if let Some(ref _file) = params.file {
        // get_tag_counts doesn't support file filter yet — return all for now
        // TODO: add file-scoped tag query to VectorStore
    }

    Ok(Json(TagsResponse {
        tags: Some(tag_counts),
        files: None,
        tagged_chunks: tagged,
        untagged_chunks: untagged,
    }))
}

// ---------------------------------------------------------------------------
// Bundle endpoints
// ---------------------------------------------------------------------------

/// Load bundles from tags config (local + global fallback).
fn load_bundles(state: &AppState) -> Vec<BundleConfig> {
    let local_path = TagsConfig::tags_path(&state.repo_root);
    let mut config = TagsConfig::load_or_default(&local_path);

    if config.bundles.is_empty() {
        if let Some(global_dir) = Config::global_config_dir() {
            let global_tags_path = global_dir.join("tags.toml");
            if global_tags_path.exists() {
                let global_config = TagsConfig::load_or_default(&global_tags_path);
                if !global_config.bundles.is_empty() {
                    config.bundles = global_config.bundles;
                }
            }
        }
    }

    config.bundles
}

#[derive(Deserialize)]
pub(super) struct BundlesListParams {
    /// Filter bundles by repo
    repo: Option<String>,
}

#[derive(Serialize)]
pub(super) struct BundleListItem {
    name: String,
    slug: String,
    description: String,
    keywords: Vec<String>,
    file_count: usize,
    ref_count: usize,
    doc_count: usize,
    repos: Vec<String>,
    includes: Vec<String>,
    parent: Option<String>,
}

#[derive(Serialize)]
pub(super) struct BundlesListResponse {
    bundles: Vec<BundleListItem>,
    total: usize,
}

/// GET /bundles — list all bundles with optional repo filter
pub(super) async fn bundles_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BundlesListParams>,
) -> Result<Json<BundlesListResponse>, (StatusCode, Json<ErrorBody>)> {
    let all_bundles = load_bundles(&state);

    let filtered: Vec<&BundleConfig> = if let Some(ref repo) = params.repo {
        all_bundles
            .iter()
            .filter(|b| b.repos.is_empty() || b.repos.contains(repo))
            .collect()
    } else {
        all_bundles.iter().collect()
    };

    let items: Vec<BundleListItem> = filtered
        .iter()
        .map(|b| BundleListItem {
            name: b.name.clone(),
            slug: b.slug(),
            description: b.description.clone(),
            keywords: b.keywords.clone(),
            file_count: b.member_files().len(),
            ref_count: b.refs.len(),
            doc_count: b.docs.len(),
            repos: b.repos.clone(),
            includes: b.includes.clone(),
            parent: b.parent_name().map(|s| s.to_string()),
        })
        .collect();

    let total = items.len();
    Ok(Json(BundlesListResponse {
        bundles: items,
        total,
    }))
}

#[derive(Serialize)]
pub(super) struct BundleRefItem {
    raw: String,
    file: Option<String>,
    target: Option<String>,
    repo: Option<String>,
}

#[derive(Serialize)]
pub(super) struct BundleDetailResponse {
    name: String,
    slug: String,
    description: String,
    keywords: Vec<String>,
    files: Vec<String>,
    refs: Vec<BundleRefItem>,
    docs: Vec<String>,
    includes: Vec<String>,
    repos: Vec<String>,
    member_files: Vec<String>,
    children: Vec<BundleListItem>,
}

/// GET /bundles/{name} — show bundle detail
pub(super) async fn bundles_show(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<BundleDetailResponse>, (StatusCode, Json<ErrorBody>)> {
    let all_bundles = load_bundles(&state);

    // Resolve name (handle b: prefix, slug, etc.)
    let resolved = resolve_bundle_name_http(&name, &all_bundles);

    let bundle = all_bundles
        .iter()
        .find(|b| b.name == resolved)
        .ok_or_else(|| {
            let available: Vec<String> = all_bundles.iter().map(|b| b.name.clone()).collect();
            bad_request(format!(
                "Bundle '{}' not found. Available: {}",
                name,
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            ))
        })?;

    let refs: Vec<BundleRefItem> = bundle
        .refs
        .iter()
        .map(|r| {
            if let Some(parsed) = BundleRef::parse(r) {
                BundleRefItem {
                    raw: r.clone(),
                    file: Some(parsed.file),
                    target: Some(match &parsed.target {
                        RefTarget::WholeFile => "file".to_string(),
                        RefTarget::Symbol(s) => format!("symbol:{}", s),
                        RefTarget::Heading(h) => format!("heading:{}", h),
                    }),
                    repo: parsed.repo,
                }
            } else {
                BundleRefItem {
                    raw: r.clone(),
                    file: None,
                    target: None,
                    repo: None,
                }
            }
        })
        .collect();

    let children: Vec<BundleListItem> = all_bundles
        .iter()
        .filter(|b| b.parent_name() == Some(&bundle.name))
        .map(|b| BundleListItem {
            name: b.name.clone(),
            slug: b.slug(),
            description: b.description.clone(),
            keywords: b.keywords.clone(),
            file_count: b.member_files().len(),
            ref_count: b.refs.len(),
            doc_count: b.docs.len(),
            repos: b.repos.clone(),
            includes: b.includes.clone(),
            parent: b.parent_name().map(|s| s.to_string()),
        })
        .collect();

    Ok(Json(BundleDetailResponse {
        name: bundle.name.clone(),
        slug: bundle.slug(),
        description: bundle.description.clone(),
        keywords: bundle.keywords.clone(),
        files: bundle.files.clone(),
        refs,
        docs: bundle.docs.clone(),
        includes: bundle.includes.clone(),
        repos: bundle.repos.clone(),
        member_files: bundle.member_files(),
        children,
    }))
}

/// Resolve bundle name from URL path segment.
fn resolve_bundle_name_http(input: &str, bundles: &[BundleConfig]) -> String {
    let name = input.strip_prefix("b:").unwrap_or(input);

    // Direct name match
    if bundles.iter().any(|b| b.name == name) {
        return name.to_string();
    }

    // Slug match
    if let Some(b) = bundles.iter().find(|b| b.slug() == name) {
        return b.name.clone();
    }

    // Try converting hyphens to slashes (slug → name)
    let as_path = name.replace('-', "/");
    if bundles.iter().any(|b| b.name == as_path) {
        return as_path;
    }

    name.to_string()
}
