use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// A single metric event written to `.bobbin/metrics.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEvent {
    /// RFC3339 timestamp
    pub timestamp: String,
    /// Session/caller identity (session_id, env var, or CLI flag)
    pub source: String,
    /// Event type: "command", "hook_injection", "hook_gate_skip", "hook_dedup_skip", etc.
    pub event_type: String,
    /// Command or hook name: "search", "context", "hook inject-context", etc.
    pub command: String,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Arbitrary metadata (files returned, scores, etc.)
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

const METRICS_FILE: &str = "metrics.jsonl";

fn metrics_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".bobbin").join(METRICS_FILE)
}

/// Resolve the metrics source identity.
///
/// Priority: CLI flag > env var > hook session_id > "unknown"
pub fn resolve_source(
    cli_flag: Option<&str>,
    hook_session_id: Option<&str>,
) -> String {
    if let Some(s) = cli_flag {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    if let Ok(v) = std::env::var("BOBBIN_METRICS_SOURCE") {
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(s) = hook_session_id {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    "unknown".to_string()
}

/// Append a metric event to `.bobbin/metrics.jsonl`.
///
/// Best-effort: silently ignores I/O errors (metrics are non-critical).
pub fn emit(repo_root: &Path, event: &MetricEvent) {
    let path = metrics_path(repo_root);

    // Ensure .bobbin/ exists
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let Ok(line) = serde_json::to_string(event) else {
        return;
    };

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };

    let _ = writeln!(file, "{}", line);
}

/// Read all metric events from `.bobbin/metrics.jsonl`.
pub fn read_all(repo_root: &Path) -> Vec<MetricEvent> {
    let path = metrics_path(repo_root);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Read metric events filtered by source.
pub fn read_by_source(repo_root: &Path, source: &str) -> Vec<MetricEvent> {
    read_all(repo_root)
        .into_iter()
        .filter(|e| e.source == source)
        .collect()
}

/// Clear the metrics file.
pub fn clear(repo_root: &Path) {
    let path = metrics_path(repo_root);
    let _ = std::fs::remove_file(&path);
}

/// Create a metric event with the current timestamp.
pub fn event(
    source: &str,
    event_type: &str,
    command: &str,
    duration_ms: u64,
    metadata: serde_json::Value,
) -> MetricEvent {
    MetricEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        source: source.to_string(),
        event_type: event_type.to_string(),
        command: command.to_string(),
        duration_ms,
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".bobbin")).unwrap();
        dir
    }

    #[test]
    fn test_emit_and_read() {
        let dir = setup();
        let ev = event("test-source", "command", "search", 42, serde_json::json!({"query": "hello"}));
        emit(dir.path(), &ev);

        let events = read_all(dir.path());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, "test-source");
        assert_eq!(events[0].event_type, "command");
        assert_eq!(events[0].command, "search");
        assert_eq!(events[0].duration_ms, 42);
        assert_eq!(events[0].metadata["query"], "hello");
    }

    #[test]
    fn test_multiple_events() {
        let dir = setup();
        emit(dir.path(), &event("s1", "command", "search", 10, serde_json::Value::Null));
        emit(dir.path(), &event("s2", "command", "context", 20, serde_json::Value::Null));
        emit(dir.path(), &event("s1", "hook_injection", "hook inject-context", 5, serde_json::Value::Null));

        let all = read_all(dir.path());
        assert_eq!(all.len(), 3);

        let s1 = read_by_source(dir.path(), "s1");
        assert_eq!(s1.len(), 2);

        let s2 = read_by_source(dir.path(), "s2");
        assert_eq!(s2.len(), 1);
    }

    #[test]
    fn test_clear() {
        let dir = setup();
        emit(dir.path(), &event("s", "command", "search", 10, serde_json::Value::Null));
        assert_eq!(read_all(dir.path()).len(), 1);

        clear(dir.path());
        assert_eq!(read_all(dir.path()).len(), 0);
    }

    #[test]
    fn test_read_empty() {
        let dir = setup();
        assert!(read_all(dir.path()).is_empty());
    }

    #[test]
    fn test_read_nonexistent() {
        let dir = TempDir::new().unwrap();
        assert!(read_all(dir.path()).is_empty());
    }

    #[test]
    fn test_resolve_source_cli_flag() {
        assert_eq!(resolve_source(Some("cli-val"), Some("session-val")), "cli-val");
    }

    #[test]
    fn test_resolve_source_env_var() {
        std::env::set_var("BOBBIN_METRICS_SOURCE", "env-val");
        assert_eq!(resolve_source(None, Some("session-val")), "env-val");
        std::env::remove_var("BOBBIN_METRICS_SOURCE");
    }

    #[test]
    fn test_resolve_source_session_id() {
        std::env::remove_var("BOBBIN_METRICS_SOURCE");
        assert_eq!(resolve_source(None, Some("session-123")), "session-123");
    }

    #[test]
    fn test_resolve_source_fallback() {
        std::env::remove_var("BOBBIN_METRICS_SOURCE");
        assert_eq!(resolve_source(None, None), "unknown");
    }

    #[test]
    fn test_null_metadata_not_serialized() {
        let ev = event("s", "command", "test", 0, serde_json::Value::Null);
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains("metadata"));
    }
}
