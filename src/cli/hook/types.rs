use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Config;

/// Claude Code UserPromptSubmit hook input (subset of fields we need)
#[derive(Deserialize)]
pub(super) struct HookInput {
    /// The user's prompt text
    #[serde(default)]
    pub(super) prompt: String,
    /// Working directory when the hook was invoked
    #[serde(default)]
    pub(super) cwd: String,
    /// Claude Code session ID (used as metrics source identity)
    #[serde(default)]
    pub(super) session_id: String,
}

/// Generate a unique injection_id for a context injection.
/// Format: `inj-<8 hex chars>` (compact, unique per query+time).
pub(super) fn generate_context_injection_id(query: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(query.as_bytes());
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    let hash = hex::encode(hasher.finalize());
    format!("inj-{}", &hash[..8])
}

/// Claude Code PostToolUse hook input
#[derive(Deserialize)]
pub(super) struct PostToolUseInput {
    /// Tool name (e.g., "Write", "Edit", "Bash")
    #[serde(default)]
    pub(super) tool_name: String,
    /// Tool input parameters
    #[serde(default)]
    pub(super) tool_input: serde_json::Value,
    /// Working directory when the hook was invoked
    #[serde(default)]
    pub(super) cwd: String,
    /// Claude Code session ID
    #[serde(default)]
    pub(super) session_id: String,
}

/// Claude Code PostToolUseFailure hook input
#[derive(Deserialize)]
pub(super) struct PostToolUseFailureInput {
    /// Tool name (e.g., "Bash", "Write", "Edit")
    #[serde(default)]
    pub(super) tool_name: String,
    /// Tool input parameters
    #[serde(default)]
    pub(super) tool_input: serde_json::Value,
    /// Error message from the failed tool
    #[serde(default)]
    pub(super) error: String,
    /// Working directory when the hook was invoked
    #[serde(default)]
    pub(super) cwd: String,
    /// Claude Code session ID
    #[serde(default)]
    pub(super) session_id: String,
}

/// Walk up from `start` looking for a directory containing `.bobbin/config.toml`.
pub(super) fn find_bobbin_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if Config::config_path(&dir).exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Input JSON from Claude Code SessionStart hook
#[derive(Deserialize)]
pub(super) struct SessionStartInput {
    #[serde(default)]
    pub(super) source: String,
    #[serde(default)]
    pub(super) cwd: String,
    /// Claude Code session ID (used as metrics source identity)
    #[serde(default)]
    pub(super) session_id: String,
}

/// Output JSON for Claude Code hook response
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HookResponse {
    pub(super) hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HookSpecificOutput {
    pub(super) hook_event_name: String,
    pub(super) additional_context: String,
}

/// A file with its symbols for display
pub(super) struct FileSymbolInfo {
    pub(super) path: String,
    pub(super) symbols: Vec<SymbolInfo>,
}

pub(super) struct SymbolInfo {
    pub(super) name: String,
}
