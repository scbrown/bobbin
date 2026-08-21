//! Live-lane IRI scheme for code/document entities.
//!
//! `bobbin:` ≡ `aegis:` (decided 2026-08-21): vocabulary lives under the
//! aegis ontology namespace, entity IRIs under the aegis code base, in the
//! exact shape camayoc's repo ingest lane mints — repo + the relative path
//! percent-encoded as ONE segment, symbols as `name-L{line}`, sections as
//! `S{line}`. Shared here (feature-ungated) so the local entities table
//! and the knowledge-gated quipu emitters mint identical identities.

/// Vocabulary namespace (`bobbin:name`, `bobbin:Chunk`, …).
pub const ONTOLOGY_NS: &str = "http://aegis.gastown.local/ontology/";

/// Entity namespace for code/document IRIs.
pub const CODE_BASE: &str = "http://aegis.gastown.local/code/";

/// Percent-encode a path into one opaque IRI segment.
///
/// `%` first (never double-encode), then `/` — the live lane treats a
/// relative path as a single segment — then the characters not valid in
/// IRI path segments.
pub fn iri_encode(s: &str) -> String {
    s.replace('%', "%25")
        .replace('/', "%2F")
        .replace(' ', "%20")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('"', "%22")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

/// IRI for a code module / document entity (one per file).
pub fn code_module_iri(repo: &str, path: &str) -> String {
    format!("{CODE_BASE}{}/{}", iri_encode(repo), iri_encode(path))
}

/// IRI for a symbol: the live lane's `name-L{line}` idiom (the line keeps
/// same-named symbols in one file distinct).
pub fn symbol_iri(repo: &str, path: &str, name: &str, line: u32) -> String {
    format!(
        "{}/{}-L{line}",
        code_module_iri(repo, path),
        iri_encode(name)
    )
}

/// IRI for a document section: the live lane's `S{line}` idiom.
pub fn section_iri(repo: &str, path: &str, line: u32) -> String {
    format!("{}/S{line}", code_module_iri(repo, path))
}

/// IRI for a chunk: the `C{line}` sibling of `S{line}` (roadmap W2.P4).
pub fn chunk_iri(repo: &str, path: &str, start_line: u32) -> String {
    format!("{}/C{start_line}", code_module_iri(repo, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_lane_shapes() {
        assert_eq!(
            code_module_iri("quipu", "src/main.rs"),
            "http://aegis.gastown.local/code/quipu/src%2Fmain.rs"
        );
        assert_eq!(
            symbol_iri("quipu", "src/main.rs", "run", 42),
            "http://aegis.gastown.local/code/quipu/src%2Fmain.rs/run-L42"
        );
        assert_eq!(
            section_iri("q", "README.md", 7),
            "http://aegis.gastown.local/code/q/README.md/S7"
        );
        assert_eq!(
            chunk_iri("q", "README.md", 7),
            "http://aegis.gastown.local/code/q/README.md/C7"
        );
        assert_eq!(iri_encode("50% done/x"), "50%25%20done%2Fx");
    }
}
