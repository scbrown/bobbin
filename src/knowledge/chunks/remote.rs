use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::types::{Chunk, ChunkEdge};

const QUIPU_CLIENT: &str = "ingest-cron";

#[cfg(not(test))]
// Replacing a large repository snapshot is a real remote write transaction, not
// a health probe. Production-scale snapshots can legitimately exceed the old
// 15-second deadline while the remote service remains responsive to small reads.
const TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(not(test))]
const PROMOTE_DEADLINE: Duration = Duration::from_secs(900);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(test)]
const PROMOTE_DEADLINE: Duration = Duration::from_secs(1);
const PART_BYTES: usize = 256 * 1024;
const MAX_ATTEMPTS: usize = 3;

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
    let turtle = super::generate_chunk_turtle(chunks, edges, repo_name);
    let snapshot = format!("bobbin-chunks:{repo_name}");
    let content_hash = sha256(turtle.as_bytes());
    let upload_id = sha256(format!("{snapshot}\n{content_hash}").as_bytes());
    let parts: Vec<&[u8]> = turtle.as_bytes().chunks(PART_BYTES).collect();
    anyhow::ensure!(!parts.is_empty(), "refusing an empty chunk snapshot upload");
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .context("building remote Quipu client")?;
    let base = endpoint.trim_end_matches('/');

    for (part_number, bytes) in parts.iter().enumerate() {
        let payload = std::str::from_utf8(bytes).context("chunk snapshot is not UTF-8")?;
        let body = serde_json::json!({
            "upload_id": upload_id, "snapshot": snapshot, "content_hash": content_hash,
            "total_parts": parts.len(), "total_bytes": turtle.len(),
            "part_number": part_number, "part_hash": sha256(bytes), "payload": payload,
            "actor": "bobbin", "source": format!("bobbin chunk index: {repo_name}"),
        });
        post_with_retries(&client, &format!("{base}/knot/stage"), token, &body).await?;
    }

    let promote_url = format!("{base}/knot/promote");
    let promote_body = serde_json::json!({"upload_id": upload_id});
    let started = tokio::time::Instant::now();
    let result = loop {
        let result = post_with_retries(&client, &promote_url, token, &promote_body).await?;
        if result.get("pending").and_then(|v| v.as_bool()) != Some(true) {
            break result;
        }
        anyhow::ensure!(
            started.elapsed() < PROMOTE_DEADLINE,
            "remote Quipu snapshot promotion remained pending for {} seconds",
            PROMOTE_DEADLINE.as_secs()
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    if result.get("conforms").and_then(|v| v.as_bool()) == Some(false) {
        anyhow::bail!("remote Quipu refused chunk snapshot by SHACL: {result}");
    }
    if result.get("replaced").and_then(|v| v.as_bool()) != Some(true)
        || result.get("promoted").and_then(|v| v.as_bool()) != Some(true)
        || result.get("content_hash").and_then(|v| v.as_str()) != Some(content_hash.as_str())
    {
        anyhow::bail!(
            "remote Quipu did not confirm the exact promoted snapshot; refusing success: {result}"
        );
    }
    Ok((
        result["tx_id"].as_i64().unwrap_or(-1),
        result["count"].as_u64().unwrap_or(0) as usize,
    ))
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

async fn post_with_retries(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    post_with_retries_timeout(client, url, token, body, TIMEOUT).await
}

async fn post_with_retries_timeout(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: &serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let mut last_error = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match client
            .post(url)
            .timeout(timeout)
            .header("X-Quipu-Client", QUIPU_CLIENT)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                if status.is_success() {
                    return serde_json::from_str(&text)
                        .with_context(|| format!("parsing remote Quipu response from {url}"));
                }
                // A deterministic 4xx cannot improve on retry. 5xx is
                // indeterminate and safe to retry because stage/promote are
                // content-addressed and idempotent.
                if status.is_client_error() {
                    anyhow::bail!(
                        "remote Quipu returned HTTP {status}: {}",
                        text.chars().take(300).collect::<String>()
                    );
                }
                last_error = Some(format!(
                    "HTTP {status}: {}",
                    text.chars().take(300).collect::<String>()
                ));
            }
            Err(error) => last_error = Some(error.to_string()),
        }
        if attempt < MAX_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
        }
    }
    anyhow::bail!(
        "POST {url} failed after {MAX_ATTEMPTS} idempotent attempts: {}",
        last_error.unwrap_or_else(|| "unknown error".into())
    )
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
        struct Seen(Arc<Mutex<Vec<(HeaderMap, serde_json::Value)>>>);
        async fn stage(
            State(seen): State<Seen>,
            headers: HeaderMap,
            Json(body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            seen.0.lock().unwrap().push((headers, body));
            Json(serde_json::json!({"idempotent": false}))
        }
        async fn promote(
            State(seen): State<Seen>,
            headers: HeaderMap,
            Json(body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            let content_hash = seen.0.lock().unwrap()[0].1["content_hash"].clone();
            seen.0.lock().unwrap().push((headers, body));
            Json(serde_json::json!({
                "conforms": true, "replaced": true, "promoted": true,
                "content_hash": content_hash, "tx_id": 42, "count": 7
            }))
        }
        let seen = Seen::default();
        let app = Router::new()
            .route("/knot/stage", post(stage))
            .route("/knot/promote", post(promote))
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
        let guard = seen.0.lock().unwrap();
        assert_eq!(guard.len(), 2);
        let headers = &guard[0].0;
        assert_eq!(headers["authorization"], "Bearer secret");
        assert_eq!(headers["x-quipu-client"], QUIPU_CLIENT);
        assert_eq!(guard[1].1["upload_id"], guard[0].1["upload_id"]);
        drop(guard);

        async fn stalled() -> Json<serde_json::Value> {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Json(serde_json::json!({"conforms": true, "replaced": true}))
        }
        let app = Router::new().route("/knot/stage", post(stalled));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let started = std::time::Instant::now();
        assert!(
            push_with_token(&[chunk()], &[], "repo", &format!("http://{addr}"), "secret")
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
