//! `bobbin index` arguments. Split from `index.rs` (at its size ceiling).

use std::path::PathBuf;

use clap::Args;

#[derive(Args)]
pub struct IndexArgs {
    /// Only update changed files (now the default; kept for backwards compatibility)
    #[arg(long)]
    pub(in crate::cli) incremental: bool,

    /// Force reindex all files
    #[arg(long)]
    pub(in crate::cli) force: bool,

    /// Publish the full graph even if unchanged, without re-embedding files
    #[arg(long)]
    pub(in crate::cli) force_publish: bool,
    #[arg(long = "quipu-graph", help = "Registered quipu graph for chunks")]
    pub(in crate::cli) graph: Option<String>,

    /// Repository name for multi-repo indexing (auto-detected from source dir name)
    #[arg(long)]
    pub(in crate::cli) repo: Option<String>,

    /// Source directory to index files from (defaults to path)
    #[arg(long)]
    pub(in crate::cli) source: Option<PathBuf>,

    /// Also index beads (issues) from the configured bead store
    #[arg(long)]
    pub(in crate::cli) include_beads: bool,

    /// Do not publish this run's chunk or inferred graph to Quipu, whatever
    /// the config says. For sources whose text must stay out of the graph
    /// (e.g. a mounted document share, aegis-1v555n.1): publication is then
    /// opt-in per invocation instead of global.
    #[arg(long)]
    pub(in crate::cli) no_quipu_publish: bool,

    /// Skip auto-calibration after indexing
    #[arg(long)]
    pub(in crate::cli) skip_calibrate: bool,

    /// Directory containing .bobbin/ config (defaults to current directory)
    #[arg(default_value = ".")]
    pub(in crate::cli) path: PathBuf,
}
