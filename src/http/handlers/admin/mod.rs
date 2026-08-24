//! Admin handlers: status, healthz, metrics, prime, suggest, repos, groups, files.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use super::{internal_error, open_vector_store, AppState, ErrorBody};

mod repos;
pub(super) use repos::*;

// ---------------------------------------------------------------------------
// /healthz
// ---------------------------------------------------------------------------

pub(super) async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// /status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct StatusResponse {
    status: String,
    index: crate::types::IndexStats,
    sources: crate::config::SourcesConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo_path_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quipu_endpoint: Option<String>,
}

pub(super) async fn status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorBody>)> {
    let store = open_vector_store(&state).await.map_err(internal_error)?;

    let stats = store
        .get_stats(None)
        .await
        .map_err(|e| internal_error(e.into()))?;

    Ok(Json(StatusResponse {
        status: "ok".to_string(),
        index: stats,
        sources: state.resolved_sources.clone(),
        repo_path_prefix: state.config.server.repo_path_prefix.clone(),
        quipu_endpoint: state.config.quipu_endpoint.clone(),
    }))
}

// ---------------------------------------------------------------------------
// /version — the deployed-commit probe
// ---------------------------------------------------------------------------
//
// Emits the build's git sha + dirty flag + feature set so a deploy is VERIFIABLE:
// the CD driver can assert the running service reports the sha it just built,
// instead of trusting a bare 200. Mirrors quipu's /version JSON shape
// ({version, git_sha, git_dirty, features}) so the CD manifest can treat both the
// same way. `knowledge` is surfaced because a featureless build silently disables
// the knowledge MCP tools — this lets a probe catch that at runtime too.

#[derive(Serialize)]
pub(super) struct VersionFeatures {
    knowledge: bool,
}

#[derive(Serialize)]
pub(super) struct VersionResponse {
    version: &'static str,
    git_sha: &'static str,
    git_dirty: bool,
    features: VersionFeatures,
}

pub(super) async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
        git_sha: env!("BOBBIN_GIT_SHA"),
        git_dirty: env!("BOBBIN_GIT_DIRTY") == "true",
        features: VersionFeatures {
            knowledge: cfg!(feature = "knowledge"),
        },
    })
}

// ---------------------------------------------------------------------------
// /metrics
// ---------------------------------------------------------------------------

pub(super) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let store = match open_vector_store(&state).await {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                "# Failed to open vector store\nbobbin_up 0\n".to_string(),
            );
        }
    };

    let stats = store.get_stats(None).await.ok();

    let mut out = String::new();
    out.push_str("# HELP bobbin_up Whether bobbin is running.\n");
    out.push_str("# TYPE bobbin_up gauge\n");
    out.push_str("bobbin_up 1\n");
    out.push_str(&crate::operational_metrics::render_fts_metrics());

    if let Some(s) = stats {
        out.push_str("# HELP bobbin_index_files_total Total indexed files.\n");
        out.push_str("# TYPE bobbin_index_files_total gauge\n");
        out.push_str(&format!("bobbin_index_files_total {}\n", s.total_files));
        out.push_str("# HELP bobbin_index_chunks_total Total indexed chunks.\n");
        out.push_str("# TYPE bobbin_index_chunks_total gauge\n");
        out.push_str(&format!("bobbin_index_chunks_total {}\n", s.total_chunks));
        out.push_str("# HELP bobbin_index_embeddings_total Total embeddings.\n");
        out.push_str("# TYPE bobbin_index_embeddings_total gauge\n");
        out.push_str(&format!(
            "bobbin_index_embeddings_total {}\n",
            s.total_embeddings
        ));
    }

    // Maintenance freshness. This is the alertable signal for a starved sweep: the
    // nightly can skip its whole prune/compact step (lock held by a contender)
    // and still exit 0, so unit success proves nothing about the store being
    // maintained. Age of the last COMPLETED sweep does.
    //
    // Read from the shared on-disk record, so it reports maintenance done by
    // ANY participant — the reindex CLI, watch, or this server — not just this
    // process. Absent series = never swept; alert on absence too.
    let status = store.maintenance_status();
    out.push_str(
        "# HELP bobbin_maintenance_last_success_timestamp_seconds \
         Unix time of the last completed prune/compact sweep of the vector store.\n",
    );
    out.push_str("# TYPE bobbin_maintenance_last_success_timestamp_seconds gauge\n");
    for (op, ts) in [
        ("prune", status.last_prune_unix),
        ("compact", status.last_compact_unix),
    ] {
        if let Some(ts) = ts {
            out.push_str(&format!(
                "bobbin_maintenance_last_success_timestamp_seconds{{op=\"{op}\"}} {ts}\n"
            ));
        }
    }

    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        out,
    )
}
