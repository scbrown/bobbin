//! In-process counters for server operations that need Prometheus visibility.

use std::sync::atomic::{AtomicU64, Ordering};

static FTS_REBUILD_TOTAL: AtomicU64 = AtomicU64::new(0);
static FTS_ERROR_KEYWORD_TOTAL: AtomicU64 = AtomicU64::new(0);
static FTS_ERROR_HYBRID_TOTAL: AtomicU64 = AtomicU64::new(0);

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
