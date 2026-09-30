//! Remote named-graph publication against a mock that enforces quipu's staged-upload
//! contract (aegis-86f2v7). The first remote test's mock accepted any upload_id, which is
//! how a graph-salted id shipped and got every named-graph stage refused in production.

use super::*;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use std::sync::{Arc, Mutex};

const GRAPH: &str = "urn:test:graph:code";

fn chunk() -> Chunk {
    Chunk {
        id: "h".into(),
        file_path: "docs/a.md".into(),
        chunk_type: crate::types::ChunkType::Section,
        name: Some("A".into()),
        start_line: 1,
        end_line: 2,
        content: "body".into(),
        language: "markdown".into(),
        tags: String::new(),
    }
}

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<serde_json::Value>>>);

type Reply = (StatusCode, Json<serde_json::Value>);

fn refuse(msg: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

/// Mirrors quipu `stage_snapshot_part`: the id must be sha256(snapshot + "\n" +
/// content_hash), restated here from quipu's src/store/snapshot_upload.rs.
async fn stage(State(seen): State<Seen>, Json(body): Json<serde_json::Value>) -> Reply {
    let snapshot = body["snapshot"].as_str().unwrap_or_default();
    let content_hash = body["content_hash"].as_str().unwrap_or_default();
    let expected = sha256(format!("{snapshot}\n{content_hash}").as_bytes());
    if body["upload_id"] != expected.as_str() {
        return refuse("upload_id does not match sha256(snapshot + newline + content_hash)");
    }
    seen.0.lock().unwrap().push(body);
    (
        StatusCode::OK,
        Json(serde_json::json!({"idempotent": false})),
    )
}

async fn knot(Json(body): Json<serde_json::Value>) -> Reply {
    if body["graph"] == GRAPH {
        (StatusCode::OK, Json(serde_json::json!({"count": 0})))
    } else {
        refuse("unknown graph")
    }
}

async fn promote(State(seen): State<Seen>, Json(body): Json<serde_json::Value>) -> Reply {
    let guard = seen.0.lock().unwrap();
    if guard.is_empty() || guard[0]["upload_id"] != body["upload_id"] {
        return refuse("unknown upload_id");
    }
    let content_hash = guard[0]["content_hash"].clone();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "conforms": true, "replaced": true, "promoted": true,
            "content_hash": content_hash, "tx_id": 9, "count": 3
        })),
    )
}

#[tokio::test]
async fn named_graph_stage_uses_the_quipu_upload_id_and_carries_the_graph() {
    let seen = Seen::default();
    let app = Router::new()
        .route("/knot", post(knot))
        .route("/knot/stage", post(stage))
        .route("/knot/promote", post(promote))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let endpoint = format!("http://{addr}");
    let result = push_with_token(&[chunk()], &[], "repo", &endpoint, "t", Some(GRAPH)).await;
    assert_eq!(result.unwrap(), (9, 3));

    let staged = seen.0.lock().unwrap();
    assert!(!staged.is_empty());
    for body in staged.iter() {
        assert_eq!(
            body["graph"], GRAPH,
            "every stage must carry the target graph"
        );
    }
}
