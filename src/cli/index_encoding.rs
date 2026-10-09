use std::path::Path;

use crate::config::Config;
use anyhow::{Context, Result};

/// Read a file's text content for indexing.
///
/// For multimodal-enabled file types (currently PDFs) the text is extracted via
/// [`crate::index::multimodal`]; other files use UTF-8 with a Latin-1 fallback. The
/// multimodal branch only activates when `index.multimodal` is set.
pub(super) fn read_indexable_content(path: &Path, config: &Config) -> Result<String> {
    if config.index.multimodal && crate::index::multimodal::is_multimodal_file(path) {
        crate::index::multimodal::extract_text(path)
    } else if config.index.documents && crate::index::documents::is_document_file(path) {
        crate::index::documents::extract_text(path)
    } else {
        let bytes =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        // Preserve valid UTF-8 exactly. Legacy source archives often contain
        // ISO-8859-1 text: map every byte to its Latin-1 code point rather than
        // dropping the whole file or inserting replacement characters. This is
        // an explicit fallback, not encoding detection; I/O errors still fail.
        Ok(String::from_utf8(bytes)
            .unwrap_or_else(|error| error.into_bytes().into_iter().map(char::from).collect()))
    }
}
