/// Query preprocessing for improved search relevance.
///
/// Conversational prompts like "Can you help me fix the authentication bug?"
/// embed well semantically but produce noisy keyword (BM25) matches. This
/// module extracts focused search terms while preserving the semantic query.

/// Stopwords to remove from keyword queries. Kept minimal to avoid
/// accidentally removing meaningful technical terms.
const STOPWORDS: &[&str] = &[
    // Articles and determiners
    "a", "an", "the", "this", "that", "these", "those",
    // Pronouns
    "i", "me", "my", "we", "our", "you", "your", "it", "its",
    // Prepositions
    "in", "on", "at", "to", "for", "of", "with", "from", "by", "as",
    // Conjunctions
    "and", "or", "but", "so", "if", "when", "while",
    // Common verbs (non-technical)
    "is", "are", "was", "were", "be", "been", "being",
    "do", "does", "did", "have", "has", "had",
    "can", "could", "would", "should", "will", "shall", "may", "might",
    // Conversational fillers
    "please", "help", "need", "want", "like", "just", "also",
    "about", "some", "any", "all", "each", "every",
    "here", "there", "where", "what", "which", "who", "how", "why",
    // Agent/chat context
    "check", "look", "see", "show", "tell", "explain", "make", "let",
    "know", "think", "try", "use", "using", "used",
];

/// Common conversational prefixes that add no search value.
const STRIP_PREFIXES: &[&str] = &[
    "can you help me",
    "can you",
    "could you",
    "please help me",
    "please help",
    "i need to",
    "i want to",
    "help me",
    "how do i",
    "how to",
    "what is",
    "what are",
    "where is",
    "where are",
    "show me",
    "tell me about",
    "tell me",
    "let me know",
    "i'm looking for",
    "looking for",
];

/// Preprocess a query for keyword (FTS/BM25) search.
///
/// Strips conversational prefixes, removes stopwords, and preserves
/// quoted phrases and code-like identifiers (snake_case, camelCase, paths).
pub fn preprocess_for_keywords(query: &str) -> String {
    let query = query.trim();
    if query.is_empty() {
        return String::new();
    }

    // Strip conversational prefixes (case-insensitive)
    let lower = query.to_lowercase();
    let mut stripped = query;
    for prefix in STRIP_PREFIXES {
        if lower.starts_with(prefix) {
            stripped = &query[prefix.len()..];
            break;
        }
    }
    let stripped = stripped.trim_start_matches(|c: char| c == ' ' || c == ',' || c == ':');

    // Extract quoted phrases (preserve them exactly)
    let mut preserved = Vec::new();
    let mut remaining = String::new();
    let mut in_quote = false;
    let mut quote_buf = String::new();
    for ch in stripped.chars() {
        if ch == '"' || ch == '\'' || ch == '`' {
            if in_quote {
                if !quote_buf.is_empty() {
                    preserved.push(quote_buf.clone());
                }
                quote_buf.clear();
                in_quote = false;
            } else {
                in_quote = true;
            }
        } else if in_quote {
            quote_buf.push(ch);
        } else {
            remaining.push(ch);
        }
    }

    // Tokenize remaining text and filter stopwords
    let words: Vec<&str> = remaining
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            let clean = lower.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.');
            if clean.is_empty() {
                return false;
            }
            // Keep code identifiers (contain _, ., ::, or are UPPER_CASE)
            if clean.contains('_') || clean.contains('.') || clean.contains("::") {
                return true;
            }
            if clean.chars().all(|c| c.is_uppercase() || c == '_') && clean.len() > 1 {
                return true;
            }
            // Filter stopwords
            !STOPWORDS.contains(&clean)
        })
        .collect();

    // Combine preserved quoted phrases with filtered words
    let mut result_parts: Vec<String> = preserved;
    result_parts.extend(words.iter().map(|w| w.to_string()));

    let result = result_parts.join(" ");
    if result.is_empty() {
        // If preprocessing removed everything, fall back to original
        stripped.to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_technical_query() {
        assert_eq!(
            preprocess_for_keywords("authentication middleware token refresh"),
            "authentication middleware token refresh"
        );
    }

    #[test]
    fn test_strip_conversational_prefix() {
        assert_eq!(
            preprocess_for_keywords("can you help me fix the auth bug"),
            "fix auth bug"
        );
    }

    #[test]
    fn test_preserve_code_identifiers() {
        assert_eq!(
            preprocess_for_keywords("where is the verify_token function"),
            "verify_token function"
        );
    }

    #[test]
    fn test_preserve_paths() {
        assert_eq!(
            preprocess_for_keywords("show me src/auth.rs"),
            "src/auth.rs"
        );
    }

    #[test]
    fn test_preserve_quoted_phrases() {
        let result = preprocess_for_keywords("search for \"connection pool\" in the code");
        assert!(result.contains("connection pool"));
    }

    #[test]
    fn test_empty_after_filter_falls_back() {
        // "how do i" stripped, remaining is "do it" which is all stopwords
        // Should fall back to original stripped text
        let result = preprocess_for_keywords("how to do it");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_empty_query() {
        assert_eq!(preprocess_for_keywords(""), "");
        assert_eq!(preprocess_for_keywords("  "), "");
    }

    #[test]
    fn test_mixed_query() {
        let result = preprocess_for_keywords("I need to fix the HybridSearch scoring in search/hybrid.rs");
        assert!(result.contains("HybridSearch"));
        assert!(result.contains("scoring"));
        assert!(result.contains("search/hybrid.rs"));
        assert!(!result.contains("need"));
    }
}
