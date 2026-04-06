//! Similar/duplicate detection handler.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::analysis::similar::{SimilarTarget, SimilarityAnalyzer};

use super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};

// ---------------------------------------------------------------------------
// /similar
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct SimilarParams {
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
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct SimilarResponse {
    mode: String,
    threshold: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    count: usize,
    results: Vec<SimilarResultItem>,
    clusters: Vec<SimilarClusterItem>,
}

#[derive(Serialize)]
pub(super) struct SimilarResultItem {
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
pub(super) struct SimilarClusterItem {
    representative: SimilarChunkRef,
    avg_similarity: f32,
    member_count: usize,
    members: Vec<SimilarResultItem>,
}

#[derive(Serialize)]
pub(super) struct SimilarChunkRef {
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

    let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

    let mut analyzer = SimilarityAnalyzer::new(embedder, vector_store);
    let repo_filter = params.repo.as_deref();
    let access = super::resolve_filter(&state, params.role.as_deref());

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

    // Apply role-based access filtering to response
    let response = SimilarResponse {
        results: response.results.into_iter()
            .filter(|r| access.is_path_allowed(&r.file_path))
            .collect(),
        clusters: response.clusters.into_iter()
            .filter(|c| access.is_path_allowed(&c.representative.file_path))
            .map(|mut c| {
                c.members.retain(|m| access.is_path_allowed(&m.file_path));
                c.member_count = c.members.len();
                c
            })
            .collect(),
        count: 0, // recalculated below
        ..response
    };
    let response = SimilarResponse {
        count: if response.clusters.is_empty() { response.results.len() } else { response.clusters.len() },
        ..response
    };

    Ok(Json(response))
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
