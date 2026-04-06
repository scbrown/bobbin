//! Review handler — diff-aware context assembly.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::index::GitAnalyzer;
use crate::search::context::{BridgeMode, ContentMode, ContextAssembler, ContextConfig};

use super::{
    internal_error, open_metadata_store, open_vector_store, to_context_file, to_context_summary,
    AppState, ContextBudgetInfo, ContextFileOutput, ContextSummaryOutput, ErrorBody,
};

// ---------------------------------------------------------------------------
// /review
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ReviewParams {
    /// Diff spec: "unstaged", "staged", "branch:<name>", or commit range
    diff: Option<String>,
    /// Max lines budget (default 500)
    budget: Option<usize>,
    /// Coupling expansion depth (default 1)
    depth: Option<u32>,
    /// Filter by repository
    repo: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ReviewResponse {
    diff_description: String,
    changed_files: Vec<ReviewChangedFile>,
    budget: ContextBudgetInfo,
    files: Vec<ContextFileOutput>,
    summary: ContextSummaryOutput,
}

#[derive(Serialize)]
pub(super) struct ReviewChangedFile {
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
    let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

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
                top_semantic_score: 0.0,
                intent: None,
                gate_boost: None,
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
        extra_filter: None,
        tags_config: Some(state.tags_config.clone()),
        role: params.role.clone(),
        file_type_rules: state.config.file_types.clone(),
        repo_affinity: None,
        repo_affinity_boost: 2.0,
        max_bridged_files: 3,
        max_bridged_chunks_per_file: 2,
        repo_path_prefix: state.config.server.repo_path_prefix.clone(),
        ..ContextConfig::default()
    };

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let bundle = assembler
        .assemble_from_seeds(&diff_description, seeds, params.repo.as_deref())
        .await
        .map_err(internal_error)?;

    let access = super::resolve_filter(&state, params.role.as_deref());
    let filtered_files: Vec<ContextFileOutput> = bundle
        .files
        .iter()
        .filter(|f| access.is_path_allowed(&f.path))
        .map(to_context_file)
        .collect();

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
        files: filtered_files,
        summary: to_context_summary(&bundle.summary),
    }))
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
