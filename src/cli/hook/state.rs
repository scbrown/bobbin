use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistent state for hook dedup and frequency tracking.
/// Stored in `.bobbin/hook_state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct HookState {
    #[serde(default)]
    pub(super) last_session_id: String,
    #[serde(default)]
    pub(super) last_injected_chunks: Vec<String>,
    #[serde(default)]
    pub(super) last_injection_time: String,
    #[serde(default)]
    pub(super) injection_count: u64,
    #[serde(default)]
    pub(super) chunk_frequencies: HashMap<String, ChunkFrequency>,
    #[serde(default)]
    pub(super) file_frequencies: HashMap<String, u64>,
    #[serde(default)]
    pub(super) hot_topics_generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChunkFrequency {
    pub(super) count: u64,
    pub(super) file: String,
    pub(super) name: Option<String>,
}

pub(super) fn hook_state_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".bobbin").join("hook_state.json")
}

/// Load hook state from disk. Returns default on any error.
pub(super) fn load_hook_state(repo_root: &Path) -> HookState {
    let path = hook_state_path(repo_root);
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HookState::default(),
    }
}

/// Save hook state to disk. Errors are swallowed (never block prompts).
pub(super) fn save_hook_state(repo_root: &Path, state: &HookState) {
    let path = hook_state_path(repo_root);
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, json);
    }
}

/// Compute a session ID from the context bundle's chunks.
///
/// Takes the chunk composite keys (file:start:end), filters by threshold,
/// sorts alphabetically, takes top 10, concatenates with `|`, and returns
/// the first 16 hex chars of the SHA-256 hash.
pub(super) fn compute_session_id(bundle: &crate::search::context::ContextBundle, threshold: f32) -> String {
    use sha2::{Digest, Sha256};

    let mut keys: Vec<String> = bundle
        .files
        .iter()
        .flat_map(|f| {
            f.chunks
                .iter()
                .filter(|c| c.score >= threshold)
                .map(move |c| format!("{}:{}:{}", f.path, c.start_line, c.end_line))
        })
        .collect();

    keys.sort();
    keys.truncate(10);

    let joined = keys.join("|");
    let hash = Sha256::digest(joined.as_bytes());
    hex::encode(&hash[..8]) // 8 bytes = 16 hex chars
}
