//! Grep (pattern search) handler.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::{
    bad_request, internal_error, open_vector_store, parse_chunk_type, truncate, AppState,
    ErrorBody,
};

// ---------------------------------------------------------------------------
// /grep
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct GrepParams {
    /// Pattern to search for
    pattern: String,
    /// Case insensitive search
    ignore_case: Option<bool>,
    /// Use regex matching
    regex: Option<bool>,
    /// Filter by chunk type
    r#type: Option<String>,
    /// Max results (default 10)
    limit: Option<usize>,
    /// Filter by repository name
    repo: Option<String>,
    /// Filter by named repo group
    group: Option<String>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(super) struct GrepResponse {
    pattern: String,
    count: usize,
    results: Vec<GrepResultItem>,
}

#[derive(Serialize)]
pub(super) struct GrepResultItem {
    file_path: String,
    name: Option<String>,
    chunk_type: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    start_line: u32,
    end_line: u32,
    score: f32,
    language: String,
    content_preview: String,
    matching_lines: Vec<MatchingLine>,
}

#[derive(Serialize)]
pub(super) struct MatchingLine {
    line_number: u32,
    content: String,
}

pub(super) async fn grep(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GrepParams>,
) -> Result<Json<GrepResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(10);
    let ignore_case = params.ignore_case.unwrap_or(false);
    let use_regex = params.regex.unwrap_or(false);

    let type_filter = params
        .r#type
        .as_deref()
        .map(parse_chunk_type)
        .transpose()
        .map_err(|e| bad_request(e.to_string()))?;

    let regex_pattern = if use_regex {
        let pat = if ignore_case {
            format!("(?i){}", params.pattern)
        } else {
            params.pattern.clone()
        };
        Some(Regex::new(&pat).map_err(|e| bad_request(format!("Invalid regex: {}", e)))?)
    } else {
        None
    };

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = vector_store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;
    if stats.total_chunks == 0 {
        return Ok(Json(GrepResponse {
            pattern: params.pattern,
            count: 0,
            results: vec![],
        }));
    }

    // Build FTS query
    let fts_query = if use_regex {
        let cleaned: String = params
            .pattern
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == ' ' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let words: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|w| w.len() >= 2)
            .collect();
        if words.is_empty() {
            params.pattern.clone()
        } else {
            words.join(" OR ")
        }
    } else {
        params.pattern.clone()
    };

    let search_limit = if type_filter.is_some() || use_regex {
        limit * 5
    } else {
        limit
    };

    let group_sql = super::resolve_group_filter(&state, params.group.as_deref())
        .map_err(|e| bad_request(e))?;
    let results = vector_store
        .search_fts_filtered(&fts_query, search_limit, params.repo.as_deref(), group_sql.as_deref())
        .await
        .map_err(|e| internal_error(e.into()))?;

    // Apply role-based access filtering
    let access = super::resolve_filter(&state, params.role.as_deref());
    let results = access.filter_vec_by_path(results, |r| &r.chunk.file_path);

    let filtered: Vec<crate::types::SearchResult> = results
        .into_iter()
        .filter(|r| {
            if let Some(ref chunk_type) = type_filter {
                &r.chunk.chunk_type == chunk_type
            } else {
                true
            }
        })
        .filter(|r| {
            if let Some(ref re) = regex_pattern {
                re.is_match(&r.chunk.content)
                    || r.chunk.name.as_ref().is_some_and(|n| re.is_match(n))
            } else {
                true
            }
        })
        .filter(|r| {
            if !ignore_case && regex_pattern.is_none() {
                r.chunk.content.contains(&params.pattern)
                    || r.chunk
                        .name
                        .as_ref()
                        .is_some_and(|n| n.contains(&params.pattern))
            } else {
                true
            }
        })
        .take(limit)
        .collect();

    let response = GrepResponse {
        pattern: params.pattern.clone(),
        count: filtered.len(),
        results: filtered
            .iter()
            .map(|r| GrepResultItem {
                file_path: r.chunk.file_path.clone(),
                name: r.chunk.name.clone(),
                chunk_type: r.chunk.chunk_type.to_string(),
                source: crate::types::source_kind(&r.chunk.chunk_type).to_string(),
                repo: r.repo.clone(),
                start_line: r.chunk.start_line,
                end_line: r.chunk.end_line,
                score: r.score,
                language: r.chunk.language.clone(),
                content_preview: truncate(&r.chunk.content, 200),
                matching_lines: find_matching_lines(
                    &r.chunk.content,
                    &params.pattern,
                    regex_pattern.as_ref(),
                    ignore_case,
                    r.chunk.start_line,
                ),
            })
            .collect(),
    };

    Ok(Json(response))
}

fn find_matching_lines(
    content: &str,
    pattern: &str,
    regex: Option<&Regex>,
    ignore_case: bool,
    start_line: u32,
) -> Vec<MatchingLine> {
    let lines: Vec<&str> = content.lines().collect();
    let mut results = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let matches = if let Some(re) = regex {
            re.is_match(line)
        } else if ignore_case {
            line.to_lowercase().contains(&pattern.to_lowercase())
        } else {
            line.contains(pattern)
        };

        if matches {
            results.push(MatchingLine {
                line_number: start_line + idx as u32,
                content: line.to_string(),
            });
        }

        if results.len() >= 10 {
            break;
        }
    }

    results
}
