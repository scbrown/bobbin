use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::types::{Chunk, ChunkEdge};

const QUIPU_CLIENT: &str = "ingest-cron";

#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_millis(100);

/// Push a replaceable chunk snapshot to the configured remote Quipu ontology.
pub async fn push_chunks_to_remote_quipu(
    chunks: &[Chunk],
    edges: &[ChunkEdge],
    repo_name: &str,
    endpoint: &str,
) -> Result<(i64, usize)> {
    let token = quipu_auth_token().context(
        "quipu_push_chunks targets a remote ontology but no QUIPU_AUTH_TOKEN or readable token file is available",
    )?;
    push_with_token(chunks, edges, repo_name, endpoint, &token).await
}

async fn push_with_token(
    chunks: &[Chunk],
    edges: &[ChunkEdge],
    repo_name: &str,
    endpoint: &str,
    token: &str,
) -> Result<(i64, usize)> {
    let body = serde_json::json!({
        "turtle": super::generate_chunk_turtle(chunks, edges, repo_name),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "actor": "bobbin",
        "source": format!("bobbin chunk index: {repo_name}"),
        "replace_snapshot": true,
        "snapshot": format!("bobbin-chunks:{repo_name}"),
    });
    let url = format!("{}/knot", endpoint.trim_end_matches('/'));
    let response = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .context("building remote Quipu client")?
        .post(&url)
        .header("X-Quipu-Client", QUIPU_CLIENT)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "remote Quipu returned HTTP {status}: {}",
            text.chars().take(300).collect::<String>()
        );
    }
    let result: serde_json::Value =
        serde_json::from_str(&text).context("parsing remote Quipu /knot response")?;
    if result.get("conforms").and_then(|v| v.as_bool()) == Some(false) {
        anyhow::bail!("remote Quipu refused chunk snapshot by SHACL: {result}");
    }
    if result.get("replaced").and_then(|v| v.as_bool()) != Some(true) {
        anyhow::bail!(
            "remote Quipu did not confirm snapshot replacement; refusing an accumulating write: {result}"
        );
    }
    Ok((
        result["tx_id"].as_i64().unwrap_or(-1),
        result["count"].as_u64().unwrap_or(0) as usize,
    ))
}

fn quipu_auth_token() -> Option<String> {
    std::env::var("QUIPU_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| {
            let path = std::env::var_os("QUIPU_AUTH_TOKEN_FILE")
                .map(PathBuf::from)
                .or_else(|| {
                    directories::BaseDirs::new()
                        .map(|dirs| dirs.home_dir().join(".config/aegis/quipu_token"))
                })?;
            std::fs::read_to_string(path)
                .ok()
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

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

    #[tokio::test]
    async fn snapshot_is_authenticated_and_bounded() {
        #[derive(Clone, Default)]
        struct Seen(Arc<Mutex<Option<HeaderMap>>>);
        async fn knot(State(seen): State<Seen>, headers: HeaderMap) -> Json<serde_json::Value> {
            *seen.0.lock().unwrap() = Some(headers);
            Json(serde_json::json!({"conforms": true, "replaced": true, "tx_id": 42, "count": 7}))
        }
        let seen = Seen::default();
        let app = Router::new()
            .route("/knot", post(knot))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert_eq!(
            push_with_token(&[chunk()], &[], "repo", &format!("http://{addr}"), "secret")
                .await
                .unwrap(),
            (42, 7)
        );
        let headers = seen.0.lock().unwrap().take().unwrap();
        assert_eq!(headers["authorization"], "Bearer secret");
        assert_eq!(headers["x-quipu-client"], QUIPU_CLIENT);

        async fn stalled() -> Json<serde_json::Value> {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Json(serde_json::json!({"conforms": true, "replaced": true}))
        }
        let app = Router::new().route("/knot", post(stalled));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let started = std::time::Instant::now();
        assert!(
            push_with_token(&[chunk()], &[], "repo", &format!("http://{addr}"), "secret")
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }
}
