//! Command listing and HTTP command proxy handlers.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{bad_request, internal_error, not_found, AppState, ErrorBody};

// ---------------------------------------------------------------------------
// /commands (user-defined convenience commands)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(super) struct CommandsListResponse {
    count: usize,
    commands: std::collections::BTreeMap<String, CommandEntry>,
}

#[derive(Serialize)]
pub(super) struct CommandEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    command: String,
    args: Vec<String>,
    expands_to: String,
}

pub(super) async fn list_commands(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CommandsListResponse>, (StatusCode, Json<ErrorBody>)> {
    let commands = crate::commands::load_commands(&state.repo_root)
        .map_err(|e| internal_error(e.into()))?;

    let entries: std::collections::BTreeMap<String, CommandEntry> = commands
        .into_iter()
        .map(|(name, def)| {
            let expands_to = {
                let mut parts = vec![format!("bobbin {}", def.command)];
                for arg in &def.args {
                    if arg.contains(' ') {
                        parts.push(format!("\"{}\"", arg));
                    } else {
                        parts.push(arg.clone());
                    }
                }
                parts.join(" ")
            };
            (
                name,
                CommandEntry {
                    description: def.description,
                    command: def.command,
                    args: def.args,
                    expands_to,
                },
            )
        })
        .collect();

    Ok(Json(CommandsListResponse {
        count: entries.len(),
        commands: entries,
    }))
}

// ---------------------------------------------------------------------------
// /cmd (HTTP command proxy)
// ---------------------------------------------------------------------------

/// An HTTP command definition — maps a name to an endpoint with param resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpCommandDef {
    pub description: String,
    pub endpoint: String,
    #[serde(default)]
    pub pinned: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub defaults: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub required: Vec<String>,
}

/// Storage format for HTTP commands (commands.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct HttpCommandsFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    commands: std::collections::HashMap<String, HttpCommandDef>,
}

fn default_version() -> u32 { 1 }

/// Known valid endpoints that commands can proxy to.
const VALID_CMD_ENDPOINTS: &[&str] = &[
    "/search", "/grep", "/context", "/related", "/refs", "/symbols",
    "/hotspots", "/impact", "/review", "/similar", "/prime", "/beads",
    "/archive/search", "/archive/recent", "/repos", "/groups", "/tags",
    "/suggest", "/status", "/feedback", "/feedback/stats",
];

fn load_http_commands(repo_root: &std::path::Path) -> std::collections::HashMap<String, HttpCommandDef> {
    let path = repo_root.join(".bobbin").join("commands.json");
    if !path.exists() {
        return std::collections::HashMap::new();
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return std::collections::HashMap::new();
    };
    let Ok(file) = serde_json::from_str::<HttpCommandsFile>(&content) else {
        return std::collections::HashMap::new();
    };
    file.commands
}

fn save_http_commands(
    repo_root: &std::path::Path,
    commands: &std::collections::HashMap<String, HttpCommandDef>,
) -> Result<(), anyhow::Error> {
    let path = repo_root.join(".bobbin").join("commands.json");
    let file = HttpCommandsFile {
        version: 1,
        commands: commands.clone(),
    };
    let content = serde_json::to_string_pretty(&file)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[derive(Serialize)]
pub(super) struct HttpCommandsListResponse {
    count: usize,
    commands: std::collections::HashMap<String, HttpCommandDef>,
}

/// GET /cmd — list all registered HTTP commands
pub(super) async fn list_http_commands(
    State(state): State<Arc<AppState>>,
) -> Result<Json<HttpCommandsListResponse>, (StatusCode, Json<ErrorBody>)> {
    let commands = load_http_commands(&state.repo_root);
    Ok(Json(HttpCommandsListResponse {
        count: commands.len(),
        commands,
    }))
}

/// POST /cmd — register a new HTTP command
pub(super) async fn register_http_command(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterCommandInput>,
) -> Result<(StatusCode, Json<HttpCommandDef>), (StatusCode, Json<ErrorBody>)> {
    // Validate name: lowercase alphanumeric + hyphens, 2-64 chars
    let name_re = regex::Regex::new(r"^[a-z0-9][a-z0-9-]{1,63}$").unwrap();
    if !name_re.is_match(&body.name) {
        return Err(bad_request("Command name must be 2-64 chars, lowercase alphanumeric + hyphens".into()));
    }

    // Validate endpoint
    if !VALID_CMD_ENDPOINTS.contains(&body.def.endpoint.as_str()) {
        return Err(bad_request(format!(
            "Unknown endpoint '{}'. Valid: {}",
            body.def.endpoint,
            VALID_CMD_ENDPOINTS.join(", "),
        )));
    }

    // Validate description
    if body.def.description.is_empty() || body.def.description.len() > 200 {
        return Err(bad_request("Description required (1-200 chars)".into()));
    }

    // Validate required params don't overlap with pinned
    for req in &body.def.required {
        if body.def.pinned.contains_key(req) {
            return Err(bad_request(format!(
                "Required param '{}' is already pinned (redundant)", req
            )));
        }
    }

    let mut commands = load_http_commands(&state.repo_root);
    commands.insert(body.name.clone(), body.def.clone());
    save_http_commands(&state.repo_root, &commands)
        .map_err(|e| internal_error(e.into()))?;

    Ok((StatusCode::CREATED, Json(body.def)))
}

#[derive(Deserialize)]
pub(super) struct RegisterCommandInput {
    name: String,
    #[serde(flatten)]
    def: HttpCommandDef,
}

/// DELETE /cmd/{name} — remove a command
pub(super) async fn delete_http_command(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let mut commands = load_http_commands(&state.repo_root);
    if commands.remove(&name).is_none() {
        return Err(not_found(format!("Command '{}' not found", name)));
    }
    save_http_commands(&state.repo_root, &commands)
        .map_err(|e| internal_error(e.into()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /cmd/{name}?{params} — invoke a command via internal dispatch
pub(super) async fn invoke_http_command(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    raw_query: axum::extract::RawQuery,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let commands = load_http_commands(&state.repo_root);
    let Some(cmd) = commands.get(&name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorBody { error: format!("Command '{}' not found", name) }),
        ).into_response();
    };

    // Parse caller-supplied params
    let caller_params: std::collections::HashMap<String, String> = raw_query
        .0
        .as_deref()
        .unwrap_or("")
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((
                urlencoding::decode(k).unwrap_or_default().into_owned(),
                urlencoding::decode(v).unwrap_or_default().into_owned(),
            ))
        })
        .collect();

    // Merge: pinned → defaults → caller
    let mut merged = cmd.defaults.clone();
    for (k, v) in &caller_params {
        if !cmd.pinned.contains_key(k) {
            merged.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in &cmd.pinned {
        merged.insert(k.clone(), v.clone());
    }

    // Validate required
    for req in &cmd.required {
        if !merged.contains_key(req) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Missing required parameter: {}", req),
                    "command": name,
                    "required": cmd.required,
                })),
            ).into_response();
        }
    }

    // Build query string
    let query_string: String = merged
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let uri = if query_string.is_empty() {
        cmd.endpoint.clone()
    } else {
        format!("{}?{}", cmd.endpoint, query_string)
    };

    // Dispatch through inner router
    let Some(router) = state.inner_router.get() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: "Internal router not initialized".into() }),
        ).into_response();
    };

    let req = Request::builder()
        .uri(&uri)
        .body(Body::empty())
        .unwrap();

    match router.clone().oneshot(req).await {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: format!("Internal dispatch error: {}", e) }),
        ).into_response(),
    }
}
