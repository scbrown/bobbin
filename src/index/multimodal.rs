//! Multimodal ingest: extract searchable text from non-source file types.
//!
//! Today this covers PDFs (runbooks, design docs) via the pure-Rust
//! `pdf-extract` crate — no Python, no native toolchain. The extracted text is
//! handed back to the normal chunking pipeline (language = "pdf") so PDFs become
//! searchable and graph-extractable alongside code and markdown.
//!
//! Image captioning (vision LLM) is intentionally out of scope here: bobbin has
//! no chat/vision provider path yet, so it is tracked as a follow-up bead. This
//! module is the seam where that second extractor will slot in.

use std::path::Path;

use anyhow::{Context, Result};

/// File extensions handled by the multimodal extractor.
///
/// Centralized so the indexer's include-pattern injection, the per-file routing
/// decision, and any future additions all agree on one list.
pub const MULTIMODAL_EXTENSIONS: &[&str] = &["pdf"];

/// Returns true if `path` has an extension the multimodal extractor handles.
/// Comparison is case-insensitive (`.PDF` counts).
pub fn is_multimodal_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            MULTIMODAL_EXTENSIONS.contains(&ext.as_str())
        }
        None => false,
    }
}

/// Extract plain text from a multimodal file for indexing.
///
/// Currently only PDFs are supported; callers gate this behind
/// [`is_multimodal_file`] and the `index.multimodal` config flag, so an
/// unsupported extension here is a programming error, not user input.
pub fn extract_text(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => extract_pdf_text(path),
        other => anyhow::bail!("multimodal extractor: unsupported extension {other:?}"),
    }
}

/// Extract text from a PDF using the pure-Rust `pdf-extract` crate.
///
/// Returns the concatenated text content. Encrypted or image-only PDFs may
/// yield little or no text; callers treat empty output the same as an empty
/// file (skipped), so this only errors on genuine parse failures.
fn extract_pdf_text(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path)
        .with_context(|| format!("failed to extract text from PDF {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_pdf_extension_case_insensitively() {
        assert!(is_multimodal_file(&PathBuf::from("docs/runbook.pdf")));
        assert!(is_multimodal_file(&PathBuf::from("docs/RUNBOOK.PDF")));
        assert!(!is_multimodal_file(&PathBuf::from("src/main.rs")));
        assert!(!is_multimodal_file(&PathBuf::from("README.md")));
        assert!(!is_multimodal_file(&PathBuf::from("no_extension")));
    }

    #[test]
    fn extract_text_rejects_unsupported_extension() {
        let err = extract_text(&PathBuf::from("photo.png")).unwrap_err();
        assert!(err.to_string().contains("unsupported extension"));
    }

    #[test]
    fn extracts_text_from_sample_pdf() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.pdf");
        let text = extract_text(&fixture).expect("extract sample PDF");
        // Whitespace/layout is extractor-dependent; assert the words survive.
        let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            normalized.contains("Hello Bobbin multimodal PDF ingest"),
            "extracted text was: {text:?}",
        );
    }
}
