use colored::Colorize;
use std::time::Duration;

#[derive(Default)]
pub(super) struct ProfileStats {
    pub file_read_ms: u128,
    pub parse_ms: u128,
    pub context_ms: u128,
    pub embed_ms: u128,
    pub embed_tokenize_ms: u128,
    pub embed_inference_ms: u128,
    pub embed_pooling_ms: u128,
    pub delete_ms: u128,
    pub insert_ms: u128,
    pub git_coupling_ms: u128,
    pub git_commits_ms: u128,
    pub deps_ms: u128,
    pub graph_push_ms: u128,
    pub compact_ms: u128,
    pub total_chunks_embedded: usize,
    pub total_batches: usize,
}

impl ProfileStats {
    pub fn print(&self, elapsed: Duration) {
        let total_ms = elapsed.as_millis();
        let accounted = self.file_read_ms
            + self.parse_ms
            + self.context_ms
            + self.embed_ms
            + self.delete_ms
            + self.insert_ms
            + self.git_coupling_ms
            + self.deps_ms
            + self.graph_push_ms
            + self.git_commits_ms
            + self.compact_ms;
        println!("\n{}", "Profile:".bold());
        println!("  file I/O:       {:>7}ms", self.file_read_ms);
        println!("  parse:          {:>7}ms", self.parse_ms);
        println!("  context:        {:>7}ms", self.context_ms);
        println!(
            "  embed:          {:>7}ms  ({} chunks in {} batches)",
            self.embed_ms, self.total_chunks_embedded, self.total_batches
        );
        println!("    tokenize:     {:>7}ms", self.embed_tokenize_ms);
        println!("    inference:    {:>7}ms", self.embed_inference_ms);
        println!("    pooling:      {:>7}ms", self.embed_pooling_ms);
        println!("  lance delete:   {:>7}ms", self.delete_ms);
        println!("  lance insert:   {:>7}ms", self.insert_ms);
        println!("  git coupling:   {:>7}ms", self.git_coupling_ms);
        println!("  git commits:    {:>7}ms", self.git_commits_ms);
        println!("  deps:           {:>7}ms", self.deps_ms);
        println!("  graph push:     {:>7}ms", self.graph_push_ms);
        println!("  compact:        {:>7}ms", self.compact_ms);
        println!(
            "  other/overhead: {:>7}ms",
            total_ms.saturating_sub(accounted)
        );
        println!("  TOTAL:          {:>7}ms", total_ms);
        if self.total_chunks_embedded > 0 && self.embed_ms > 0 {
            let rate = self.total_chunks_embedded as f64 / (self.embed_ms as f64 / 1000.0);
            println!("  embed throughput: {:.1} chunks/s", rate);
        }
    }
}
