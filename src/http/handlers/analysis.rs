//! Analysis handlers: related, refs, symbols, hotspots, impact.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::analysis::complexity::ComplexityAnalyzer;
use crate::analysis::impact::{ImpactAnalyzer, ImpactConfig, ImpactMode, ImpactSignal};
use crate::analysis::refs::RefAnalyzer;
use crate::index::GitAnalyzer;

use super::{
    bad_request, detect_language, internal_error, open_metadata_store, open_vector_store,
    AppState, ErrorBody,
};

// ---------------------------------------------------------------------------
// /related
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct RelatedParams {
    /// File path to find related files for
    file: String,
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
        .filter(|r| access.is_path_allowed(&r.path))
        .collect();

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
    let repo_name = params.repo.as_deref()
        .unwrap_or_else(|| RepoFilter::repo_from_path(&params.file));
    if !access.is_allowed(repo_name) {
        return Err(bad_request(format!("Repo not accessible: {}", repo_name)));
    }

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

// ---------------------------------------------------------------------------
// /hotspots
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct HotspotsParams {
    /// Time window (e.g. "6 months ago", default "1 year ago")
    since: Option<String>,
    /// Max results (default 20)
    limit: Option<usize>,
    /// Min score threshold (default 0.0)
    threshold: Option<f32>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct HotspotsResponse {
    count: usize,
    since: String,
    hotspots: Vec<HotspotItem>,
}

#[derive(Serialize)]
pub(super) struct HotspotItem {
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

    let access = super::resolve_filter(&state, params.role.as_deref());
    hotspot_items.retain(|h| access.is_path_allowed(&h.file));
    hotspot_items
        .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hotspot_items.truncate(limit);

    Ok(Json(HotspotsResponse {
        count: hotspot_items.len(),
        since: since.to_string(),
        hotspots: hotspot_items,
    }))
}

// ---------------------------------------------------------------------------
// /impact
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ImpactParams {
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
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ImpactResponse {
    target: String,
    mode: String,
    depth: u32,
    count: usize,
    results: Vec<ImpactResultItem>,
}

#[derive(Serialize)]
pub(super) struct ImpactResultItem {
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
    let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

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

    let access = super::resolve_filter(&state, params.role.as_deref());
    let filtered_results: Vec<ImpactResultItem> = results
        .iter()
        .filter(|r| access.is_path_allowed(&r.path))
        .map(|r| ImpactResultItem {
            file: r.path.clone(),
            signal: signal_name(&r.signal).to_string(),
            score: r.score,
            reason: r.reason.clone(),
        })
        .collect();

    Ok(Json(ImpactResponse {
        target: params.target,
        mode: mode_str.to_string(),
        depth,
        count: filtered_results.len(),
        results: filtered_results,
    }))
}
