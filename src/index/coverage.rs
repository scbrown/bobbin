//! Test↔source coverage mapping derived from temporal coupling.
//!
//! Bobbin already records which files change together (`FileCoupling`, see
//! [`crate::index::git`]). A test and the source it exercises almost always
//! co-change — fix a bug in `auth.rs` and you touch `test_auth.rs` in the same
//! commit. This module turns that signal into an explicit test↔source map:
//! given a file, return the coupled files on the *other* side of the test/source
//! divide.
//!
//! Test detection reuses the path-based [`classify_file`] heuristic (filename +
//! directory conventions), so no index-time schema change is needed — coverage
//! is derived purely at query time from the stored coupling table.

use crate::types::{classify_file, FileCategory, FileCoupling};

/// Which direction a coverage query resolved to, based on the target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageDirection {
    /// Target is a source file; links are the test files that cover it.
    TestsForSource,
    /// Target is a test file; links are the source files it covers.
    SourcesForTest,
}

impl CoverageDirection {
    /// Human label for the link kind (used in CLI/MCP output).
    pub fn link_kind(self) -> &'static str {
        match self {
            CoverageDirection::TestsForSource => "test",
            CoverageDirection::SourcesForTest => "source",
        }
    }
}

/// A single coverage link: a file coupled to the target on the opposite side of
/// the test/source divide, with its coupling strength.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageLink {
    pub path: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Derive coverage links for `target` from its `couplings`.
///
/// If `target` is a test file, returns the coupled **source** files it covers;
/// otherwise returns the coupled **test** files that cover it. The input
/// couplings are expected to already reference `target` as either `file_a` or
/// `file_b` (i.e. the rows returned by `get_coupling(target, _)`). Couplings on
/// the same side as the target (test↔test, source↔source) are filtered out.
///
/// Results preserve the input order (callers pass score-sorted couplings).
pub fn derive_coverage(
    target: &str,
    couplings: Vec<FileCoupling>,
) -> (CoverageDirection, Vec<CoverageLink>) {
    let target_is_test = matches!(classify_file(target), FileCategory::Test);
    let direction = if target_is_test {
        CoverageDirection::SourcesForTest
    } else {
        CoverageDirection::TestsForSource
    };
    // When the target is source we want the test side, and vice versa.
    let want_test = !target_is_test;

    let links = couplings
        .into_iter()
        .filter_map(|c| {
            let other = if c.file_a == target {
                c.file_b
            } else {
                c.file_a
            };
            let other_is_test = matches!(classify_file(&other), FileCategory::Test);
            if other_is_test == want_test {
                Some(CoverageLink {
                    path: other,
                    score: c.score,
                    co_changes: c.co_changes,
                })
            } else {
                None
            }
        })
        .collect();

    (direction, links)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coupling(a: &str, b: &str, score: f32, n: u32) -> FileCoupling {
        FileCoupling {
            file_a: a.to_string(),
            file_b: b.to_string(),
            score,
            co_changes: n,
            last_co_change: 0,
        }
    }

    #[test]
    fn source_target_returns_only_test_links() {
        let target = "src/auth.rs";
        let couplings = vec![
            coupling("src/auth.rs", "tests/test_auth.rs", 0.9, 12),
            coupling("src/auth.rs", "src/session.rs", 0.5, 6), // source↔source: excluded
            coupling("src/db.rs", "src/auth.rs", 0.4, 4),      // source↔source: excluded
        ];
        let (dir, links) = derive_coverage(target, couplings);
        assert_eq!(dir, CoverageDirection::TestsForSource);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].path, "tests/test_auth.rs");
        assert_eq!(links[0].co_changes, 12);
    }

    #[test]
    fn test_target_returns_only_source_links() {
        let target = "tests/test_auth.rs";
        let couplings = vec![
            coupling("tests/test_auth.rs", "src/auth.rs", 0.9, 12),
            coupling("tests/test_auth.rs", "tests/helpers.rs", 0.7, 8), // test↔test: excluded
        ];
        let (dir, links) = derive_coverage(target, couplings);
        assert_eq!(dir, CoverageDirection::SourcesForTest);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].path, "src/auth.rs");
    }

    #[test]
    fn handles_target_on_either_side_of_pair() {
        // Coupling rows store the pair canonically; target may be file_a or file_b.
        let target = "src/parser.rs";
        let couplings = vec![
            coupling("src/parser.rs", "parser_test.go", 0.8, 5), // target is file_a
            coupling("parser_spec.rb", "src/parser.rs", 0.6, 3), // target is file_b
        ];
        let (_dir, links) = derive_coverage(target, couplings);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["parser_test.go", "parser_spec.rb"]);
    }

    #[test]
    fn empty_couplings_yield_no_links() {
        let (dir, links) = derive_coverage("src/main.rs", vec![]);
        assert_eq!(dir, CoverageDirection::TestsForSource);
        assert!(links.is_empty());
    }
}
