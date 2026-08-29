//! In-process counters for server operations that need Prometheus visibility.

use std::sync::atomic::{AtomicU64, Ordering};

static FTS_REBUILD_TOTAL: AtomicU64 = AtomicU64::new(0);
static FTS_ERROR_KEYWORD_TOTAL: AtomicU64 = AtomicU64::new(0);
static FTS_ERROR_HYBRID_TOTAL: AtomicU64 = AtomicU64::new(0);
static HTTP_REQUEST_TOTAL: AtomicU64 = AtomicU64::new(0);
static MCP_REQUEST_TOTAL: AtomicU64 = AtomicU64::new(0);
static MCP_SESSION_HEADER_TOTAL: AtomicU64 = AtomicU64::new(0);

pub(crate) fn record_request(transport: &str, has_session: bool) {
    match transport {
        "http" => &HTTP_REQUEST_TOTAL,
        "mcp" => &MCP_REQUEST_TOTAL,
        _ => return,
    }
    .fetch_add(1, Ordering::Relaxed);
    if transport == "mcp" && has_session {
        MCP_SESSION_HEADER_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn record_fts_rebuild() {
    FTS_REBUILD_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_fts_search_error(mode: &str) {
    match mode {
        "keyword" => &FTS_ERROR_KEYWORD_TOTAL,
        "hybrid" => &FTS_ERROR_HYBRID_TOTAL,
        _ => return,
    }
    .fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn fts_rebuild_total() -> u64 {
    FTS_REBUILD_TOTAL.load(Ordering::Relaxed)
}

pub(crate) fn fts_search_errors(mode: &str) -> u64 {
    match mode {
        "keyword" => FTS_ERROR_KEYWORD_TOTAL.load(Ordering::Relaxed),
        "hybrid" => FTS_ERROR_HYBRID_TOTAL.load(Ordering::Relaxed),
        _ => 0,
    }
}

pub(crate) fn render_fts_metrics() -> String {
    let mut out = String::new();
    out.push_str("# HELP bobbin_search_errors_total Search requests that ended in an error.\n");
    out.push_str("# TYPE bobbin_search_errors_total counter\n");
    for mode in ["keyword", "hybrid"] {
        out.push_str(&format!(
            "bobbin_search_errors_total{{mode=\"{mode}\",reason=\"fts\"}} {}\n",
            fts_search_errors(mode)
        ));
    }
    out.push_str("# HELP bobbin_fts_rebuild_total Successful automatic FTS index rebuilds.\n");
    out.push_str("# TYPE bobbin_fts_rebuild_total counter\n");
    out.push_str(&format!(
        "bobbin_fts_rebuild_total {}\n",
        fts_rebuild_total()
    ));
    out.push_str(
        "# HELP bobbin_requests_total Requests observed at the Bobbin transport boundary.\n",
    );
    out.push_str("# TYPE bobbin_requests_total counter\n");
    out.push_str(&format!(
        "bobbin_requests_total{{transport=\"http\"}} {}\n",
        HTTP_REQUEST_TOTAL.load(Ordering::Relaxed)
    ));
    out.push_str(&format!(
        "bobbin_requests_total{{transport=\"mcp\"}} {}\n",
        MCP_REQUEST_TOTAL.load(Ordering::Relaxed)
    ));
    out.push_str("# HELP bobbin_mcp_session_header_requests_total MCP requests carrying a session identifier.\n");
    out.push_str("# TYPE bobbin_mcp_session_header_requests_total counter\n");
    out.push_str(&format!(
        "bobbin_mcp_session_header_requests_total {}\n",
        MCP_SESSION_HEADER_TOTAL.load(Ordering::Relaxed)
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_both_fts_counter_families_and_mode_labels() {
        let rendered = render_fts_metrics();
        assert!(rendered.contains("# TYPE bobbin_search_errors_total counter"));
        assert!(rendered.contains("bobbin_search_errors_total{mode=\"keyword\",reason=\"fts\"}"));
        assert!(rendered.contains("bobbin_search_errors_total{mode=\"hybrid\",reason=\"fts\"}"));
        assert!(rendered.contains("# TYPE bobbin_fts_rebuild_total counter"));
        assert!(rendered.contains("bobbin_requests_total{transport=\"http\"}"));
        assert!(rendered.contains("bobbin_requests_total{transport=\"mcp\"}"));
        assert!(rendered.contains("bobbin_mcp_session_header_requests_total"));
    }

    #[test]
    fn error_counter_is_mode_scoped_and_ignores_semantic() {
        let keyword_before = fts_search_errors("keyword");
        let hybrid_before = fts_search_errors("hybrid");
        record_fts_search_error("keyword");
        record_fts_search_error("semantic");
        assert_eq!(fts_search_errors("keyword"), keyword_before + 1);
        assert_eq!(fts_search_errors("hybrid"), hybrid_before);
    }
}
