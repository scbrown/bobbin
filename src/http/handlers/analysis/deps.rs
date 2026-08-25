//! `/deps` — import dependencies for a file.
//!
//! The HTTP half of the MCP `dependencies` tool (#55). Both call the same
//! `VectorStore::get_dependencies` / `get_dependents`, so the two surfaces
//! answer from one source rather than two implementations of "what does this
//! file import".
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};

#[derive(Deserialize)]
pub(crate) struct DepsParams {
    /// File path to show dependencies for
    file: String,
    /// Show what imports this file instead of what it imports
    reverse: Option<bool>,
    /// Show both directions
    both: Option<bool>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct DepsResponse {
    file: String,
    /// What this file imports. Omitted — not empty — when the caller asked
    /// only for the reverse direction, so "not asked" and "none found" stay
    /// distinguishable.
    #[serde(skip_serializing_if = "Option::is_none")]
    imports: Option<Vec<DependencyItem>>,
    /// What imports this file. Omitted when not asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    imported_by: Option<Vec<DependencyItem>>,
}

#[derive(Serialize)]
pub(crate) struct DependencyItem {
    /// The other file's path, or the raw import statement when the importer
    /// could not be resolved to a file in the index.
    path: String,
    dep_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    resolved: bool,
}

pub(crate) async fn deps(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DepsParams>,
) -> Result<Json<DepsResponse>, (StatusCode, Json<ErrorBody>)> {
    let both = params.both.unwrap_or(false);
    let reverse = params.reverse.unwrap_or(false);
    let show_imports = !reverse || both;
    let show_dependents = reverse || both;

    let access = super::super::resolve_filter(&state, params.role.as_deref());
    if !access.is_path_allowed(&params.file) {
        return Err(bad_request(format!(
            "File not found in index: {}",
            params.file
        )));
    }

    let vector_store = open_vector_store(&state).await.map_err(internal_error)?;

    let imports = if show_imports {
        let deps = vector_store
            .get_dependencies(&params.file)
            .await
            .map_err(|e| internal_error(e.into()))?;
        Some(
            deps.into_iter()
                // An unresolved import is a bare statement, not a path, so it
                // cannot leak a file the role may not see; a resolved one can.
                .filter(|d| !d.resolved || access.is_path_allowed(&d.file_b))
                .map(|d| DependencyItem {
                    path: if d.resolved {
                        d.file_b
                    } else {
                        d.import_statement.clone()
                    },
                    dep_type: d.dep_type,
                    symbol: d.symbol,
                    resolved: d.resolved,
                })
                .collect(),
        )
    } else {
        None
    };

    let imported_by = if show_dependents {
        let deps = vector_store
            .get_dependents(&params.file)
            .await
            .map_err(|e| internal_error(e.into()))?;
        Some(
            deps.into_iter()
                .filter(|d| access.is_path_allowed(&d.file_a))
                .map(|d| DependencyItem {
                    path: d.file_a,
                    dep_type: d.dep_type,
                    symbol: d.symbol,
                    resolved: d.resolved,
                })
                .collect(),
        )
    } else {
        None
    };

    Ok(Json(DepsResponse {
        file: params.file,
        imports,
        imported_by,
    }))
}
