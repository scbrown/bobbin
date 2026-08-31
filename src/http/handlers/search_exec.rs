//! The two search executors behind the `/search` handler.
//!
//! Split from `search` under the file-size ratchet. The seam is by ROLE: the
//! handler owns request shaping, response assembly and the spotlight join;
//! these two own how a query is actually run against the index, single or
//! OR-expanded.

use super::*;
use crate::search::{HybridSearch, SemanticSearch};
use crate::types::SearchResult;
use axum::http::StatusCode;
use axum::Json;

/// Execute a single search query against the given mode.
pub(super) async fn execute_single_search(
    state: &AppState,
    query: &str,
    mode: &str,
    limit: usize,
    repo_filter: Option<&str>,
    combined_filter: Option<&str>,
) -> Result<Vec<SearchResult>, (StatusCode, Json<ErrorBody>)> {
    let vector_store = open_vector_store(state).await.map_err(internal_error)?;

    match mode {
        "keyword" => match vector_store
            .search_fts_filtered(query, limit, repo_filter, combined_filter)
            .await
        {
            Ok(results) => Ok(results),
            Err(error) => {
                crate::operational_metrics::record_fts_search_error(mode);
                Err(internal_error(error.into()))
            }
        },

        "semantic" | "hybrid" => {
            let embedder = state.get_embedder().await.map_err(internal_error)?.clone();

            if mode == "semantic" {
                let mut search = SemanticSearch::new(embedder, vector_store);
                search
                    .search_filtered(query, limit, repo_filter, combined_filter)
                    .await
                    .map_err(|error| internal_error(error.into()))
            } else {
                let mut search =
                    HybridSearch::from_config(embedder, vector_store, &state.config.search);
                search
                    .search_filtered(query, limit, repo_filter, combined_filter)
                    .await
                    .map_err(|error| {
                        if error.to_string().contains("FTS") {
                            crate::operational_metrics::record_fts_search_error(mode);
                        }
                        internal_error(error.into())
                    })
            }
        }

        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: format!(
                    "Invalid mode: {}. Use 'hybrid', 'semantic', or 'keyword'",
                    mode
                ),
            }),
        )),
    }
}

/// Execute OR-branched search: run each branch, merge results by best score per chunk.
pub(super) async fn execute_or_search(
    state: &AppState,
    branches: &[String],
    mode: &str,
    limit: usize,
    repo_filter: Option<&str>,
    combined_filter: Option<&str>,
) -> Result<Vec<SearchResult>, (StatusCode, Json<ErrorBody>)> {
    use std::collections::HashMap;

    let mut best_by_id: HashMap<String, SearchResult> = HashMap::new();

    for branch in branches {
        let results =
            execute_single_search(state, branch, mode, limit, repo_filter, combined_filter).await?;

        for result in results {
            let id = result.chunk.id.clone();
            match best_by_id.get(&id) {
                Some(existing) if existing.score >= result.score => {}
                _ => {
                    best_by_id.insert(id, result);
                }
            }
        }
    }

    // Sort merged results by score descending
    let mut merged: Vec<SearchResult> = best_by_id.into_values().collect();
    merged.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(merged)
}
