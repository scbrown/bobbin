//! Incremental catch-up after activating a portable index pack.

use anyhow::Result;
use std::path::PathBuf;

use super::{index, OutputConfig};

/// Run the normal hash- and watermark-based incremental index path after a
/// portable pack has been activated.
pub(super) async fn run(
    home: PathBuf,
    source: PathBuf,
    repo: String,
    output: OutputConfig,
) -> Result<()> {
    index::run(
        index::IndexArgs {
            incremental: true,
            force: false,
            repo: Some(repo),
            source: Some(source),
            include_beads: false,
            skip_calibrate: true,
            path: home,
        },
        output,
    )
    .await
}
