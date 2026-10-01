//! What the search and context logs put in the journal at each level
//! (aegis-uy7boa): user text only at DEBUG, a fingerprint at INFO.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing::Level;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Everything `f` logs through a fmt subscriber at `level`, as text.
fn logged(level: Level, f: impl FnOnce()) -> String {
    let cap = Capture::default();
    let writer = cap.clone();
    let sub = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(sub, f);
    let out = cap.0.lock().unwrap().clone();
    String::from_utf8(out).unwrap()
}

const TEXT: &str = "out of memory|oom-kill|Killed process uy7boa-marker";

#[test]
fn search_logs_the_query_text_only_at_debug() {
    let emit = || super::search::log_search(TEXT, Some("aegis"), None, "hybrid", 3, 42);
    let sha = super::query_sha(TEXT);

    let info = logged(Level::INFO, emit);
    assert!(
        info.contains(" search ") || info.contains(": search"),
        "{info}"
    );
    assert!(info.contains(&format!("query_sha={sha}")), "{info}");
    assert!(
        info.contains("results=3") && info.contains("duration_ms=42"),
        "{info}"
    );
    assert!(
        !info.contains("uy7boa-marker") && !info.contains("oom-kill"),
        "{info}"
    );

    let debug = logged(Level::DEBUG, emit);
    assert!(
        debug.contains("uy7boa-marker"),
        "the text is there at DEBUG: {debug}"
    );
    assert_eq!(
        debug.matches(&format!("query_sha={sha}")).count(),
        2,
        "{debug}"
    );
}

#[test]
fn context_logs_the_prompt_only_at_debug() {
    let emit = || super::context::log_context(TEXT, None, Some("worker"), 2, (10, 60), 7);
    let info = logged(Level::INFO, emit);
    assert!(
        info.contains("files=2") && info.contains("budget_max=60"),
        "{info}"
    );
    assert!(
        info.contains(&format!("query_len={}", TEXT.chars().count())),
        "{info}"
    );
    assert!(!info.contains("uy7boa-marker"), "{info}");
    assert!(logged(Level::DEBUG, emit).contains("uy7boa-marker"));
}

#[test]
fn errors_still_reach_the_journal_at_info() {
    // d11cle's regression: a genuine bobbin ERROR must stay visible.
    let info = logged(Level::INFO, || tracing::error!("index write failed"));
    assert!(
        info.contains("ERROR") && info.contains("index write failed"),
        "{info}"
    );
}

#[test]
fn query_sha_is_short_stable_and_distinguishes_texts() {
    assert_eq!(super::query_sha("a"), super::query_sha("a"));
    assert_ne!(super::query_sha("a"), super::query_sha("b"));
    assert_eq!(super::query_sha("a").len(), 12);
}
