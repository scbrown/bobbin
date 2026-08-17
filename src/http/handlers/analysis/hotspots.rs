//! Hotspot and churn analysis endpoints.
//!
//! Split out of the former `analysis.rs` (bobbin-aoz).

//! Analysis handlers: related, refs, symbols, hotspots, impact.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::analysis::backend::{IndexBackend, StructuralBackend};
use crate::analysis::complexity::ComplexityAnalyzer;
use crate::analysis::impact::{ImpactConfig, ImpactMode, ImpactSignal};
use crate::index::GitAnalyzer;

use super::super::{
    bad_request, detect_language, internal_error, open_metadata_store, open_vector_store,
    AppState, ErrorBody,
};

// ---------------------------------------------------------------------------
// /hotspots
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct HotspotsParams {
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
pub(crate) struct HotspotsResponse {
    count: usize,
    since: String,
    hotspots: Vec<HotspotItem>,
}

#[derive(Serialize)]
pub(crate) struct HotspotItem {
    file: String,
    score: f32,
    churn: u32,
    complexity: f32,
    language: String,
}

pub(crate) async fn hotspots(
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

    let access = super::super::resolve_filter(&state, params.role.as_deref());
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
pub(crate) struct ImpactParams {
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
pub(crate) struct ImpactResponse {
    target: String,
    mode: String,
    depth: u32,
    count: usize,
    results: Vec<ImpactResultItem>,
}

#[derive(Serialize)]
pub(crate) struct ImpactResultItem {
    file: String,
    signal: String,
    score: f32,
    reason: String,
}

pub(crate) async fn impact(
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

    let mut metadata_store = open_metadata_store(&state).map_err(internal_error)?;
    let mut vector_store = open_vector_store(&state).await.map_err(internal_error)?;
    let mut embedder = state.get_embedder().await.map_err(internal_error)?.clone();

    let mut backend =
        IndexBackend::with_impact(&mut vector_store, &mut metadata_store, &mut embedder);
    let results = backend
        .impact(&params.target, &impact_config, depth, params.repo.as_deref())
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

    let access = super::super::resolve_filter(&state, params.role.as_deref());
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
