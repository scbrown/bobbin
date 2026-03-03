//! Advanced query parser for bobbin search.
//!
//! Parses user queries into structured components: free-text terms, inline field
//! filters (repo:X, lang:Y), quoted phrases, and negation prefixes.
//!
//! Design: never error on bad syntax — treat unparseable input as literal search
//! terms. Modeled after GitHub/Sourcegraph query conventions.

use std::fmt;

/// A parsed search query with structured components extracted.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedQuery {
    /// Free-text search terms (everything that isn't a filter or phrase)
    pub terms: Vec<String>,
    /// Exact phrase matches (from quoted strings)
    pub phrases: Vec<String>,
    /// Inline field filters
    pub filters: Vec<Filter>,
    /// The reconstructed free-text query (terms + phrases joined)
    pub text_query: String,
}

/// An inline field filter extracted from the query.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// The filter field
    pub field: FilterField,
    /// The filter value(s) — comma-separated values are split
    pub values: Vec<String>,
    /// Whether this filter is negated (prefixed with -)
    pub negated: bool,
}

/// Known filter fields that can appear inline in queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterField {
    /// Filter by repository name
    Repo,
    /// Filter by programming language
    Lang,
    /// Filter by chunk type (function, struct, section, etc.)
    Type,
    /// Filter by file path (substring match)
    File,
    /// Filter by file path (alias for File)
    Path,
    /// Filter by named repo group
    Group,
    /// Filter by tag
    Tag,
}

impl FilterField {
    /// Parse a field name string into a FilterField.
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "repo" | "repository" => Some(Self::Repo),
            "lang" | "language" => Some(Self::Lang),
            "type" | "kind" => Some(Self::Type),
            "file" | "filename" => Some(Self::File),
            "path" | "filepath" => Some(Self::Path),
            "group" => Some(Self::Group),
            "tag" | "label" => Some(Self::Tag),
            _ => None,
        }
    }

    /// The canonical name for this field (used in SQL generation).
    pub fn canonical_name(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Lang => "lang",
            Self::Type => "type",
            Self::File => "file",
            Self::Path => "path",
            Self::Group => "group",
            Self::Tag => "tag",
        }
    }

    /// The LanceDB column name this field maps to.
    pub fn column_name(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Lang => "language",
            Self::Type => "chunk_type",
            Self::File | Self::Path => "file_path",
            Self::Group => "repo", // groups resolve to repo IN (...) — handled separately
            Self::Tag => "tags",
        }
    }
}

impl fmt::Display for FilterField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.canonical_name())
    }
}

/// Parse a query string into structured components.
///
/// Extracts inline filters (repo:X, lang:Y), quoted phrases, and free-text terms.
/// Never errors — unparseable input is treated as literal search text.
///
/// # Examples
///
/// ```
/// use bobbin::search::query::parse;
///
/// let q = parse("repo:aegis lang:rust PostToolUse");
/// assert_eq!(q.terms, vec!["PostToolUse"]);
/// assert_eq!(q.filters.len(), 2);
/// assert_eq!(q.filters[0].field, bobbin::search::query::FilterField::Repo);
/// ```
pub fn parse(input: &str) -> ParsedQuery {
    let mut terms: Vec<String> = Vec::new();
    let mut phrases: Vec<String> = Vec::new();
    let mut filters: Vec<Filter> = Vec::new();

    let input = input.trim();
    if input.is_empty() {
        return ParsedQuery {
            terms,
            phrases,
            filters,
            text_query: String::new(),
        };
    }

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        // Quoted phrase: "exact match"
        if chars[i] == '"' {
            if let Some((phrase, end)) = parse_quoted(&chars, i) {
                if !phrase.is_empty() {
                    phrases.push(phrase);
                }
                i = end;
                continue;
            }
            // Unmatched quote — fall through to treat as literal
        }

        // Check for negation prefix followed by a filter
        let (negated, filter_start) = if chars[i] == '-' && i + 1 < len && !chars[i + 1].is_whitespace() {
            (true, i + 1)
        } else {
            (false, i)
        };

        // Try to parse as field:value filter
        if let Some((filter, end)) = parse_filter(&chars, filter_start, negated) {
            filters.push(filter);
            i = end;
            continue;
        }

        // Regular word/token
        let (word, end) = parse_word(&chars, i);
        if !word.is_empty() {
            // Skip boolean operators as standalone words — they're structural, not search terms
            let upper = word.to_uppercase();
            if upper != "AND" && upper != "OR" && upper != "NOT" {
                terms.push(word);
            }
        }
        i = end;
    }

    // Build the text query from terms and phrases
    let mut text_parts: Vec<String> = Vec::new();
    for t in &terms {
        text_parts.push(t.clone());
    }
    for p in &phrases {
        // Keep phrases quoted in the text query for FTS
        text_parts.push(format!("\"{}\"", p));
    }
    let text_query = text_parts.join(" ");

    ParsedQuery {
        terms,
        phrases,
        filters,
        text_query,
    }
}

/// Convert parsed filters into SQL WHERE clause fragments for LanceDB.
///
/// Returns a list of SQL conditions that should be ANDed together.
/// Group filters are excluded (handled separately via group resolution).
///
/// # Examples
///
/// - `repo:aegis` → `repo = 'aegis'`
/// - `lang:rust,go` → `language IN ('rust', 'go')`
/// - `-repo:test` → `repo != 'test'`
/// - `file:*.rs` → `file_path LIKE '%.rs'`
/// - `path:src/cli` → `file_path LIKE '%src/cli%'`
pub fn filters_to_sql(filters: &[Filter]) -> Vec<String> {
    let mut clauses = Vec::new();

    for filter in filters {
        // Group filters are handled separately via group resolution
        if filter.field == FilterField::Group {
            continue;
        }

        let col = filter.field.column_name();
        let sql = if filter.values.len() == 1 {
            let val = &filter.values[0];
            let escaped = val.replace('\'', "''");

            // Check for glob wildcards in value
            if val.contains('*') || val.contains('?') {
                // Convert glob to SQL LIKE pattern
                let like_pattern = escaped.replace('*', "%").replace('?', "_");
                if filter.negated {
                    format!("{} NOT LIKE '{}'", col, like_pattern)
                } else {
                    format!("{} LIKE '{}'", col, like_pattern)
                }
            } else if filter.field == FilterField::File || filter.field == FilterField::Path {
                // Path/file filters do substring matching by default
                if filter.negated {
                    format!("{} NOT LIKE '%{}%'", col, escaped)
                } else {
                    format!("{} LIKE '%{}%'", col, escaped)
                }
            } else if filter.negated {
                format!("{} != '{}'", col, escaped)
            } else {
                format!("{} = '{}'", col, escaped)
            }
        } else {
            // Multiple values: IN/NOT IN
            let vals: Vec<String> = filter
                .values
                .iter()
                .map(|v| format!("'{}'", v.replace('\'', "''")))
                .collect();
            if filter.negated {
                format!("{} NOT IN ({})", col, vals.join(", "))
            } else {
                format!("{} IN ({})", col, vals.join(", "))
            }
        };

        clauses.push(sql);
    }

    clauses
}

/// Extract group filter names from parsed filters.
/// Returns group names for resolution against configured groups.
pub fn extract_group_filters(filters: &[Filter]) -> Vec<String> {
    filters
        .iter()
        .filter(|f| f.field == FilterField::Group && !f.negated)
        .flat_map(|f| f.values.clone())
        .collect()
}

/// Parse a quoted string starting at position `start` (which should be `"`).
/// Returns the content between quotes and the position after the closing quote.
fn parse_quoted(chars: &[char], start: usize) -> Option<(String, usize)> {
    if start >= chars.len() || chars[start] != '"' {
        return None;
    }
    let mut i = start + 1;
    let mut content = String::new();
    while i < chars.len() {
        if chars[i] == '"' {
            return Some((content, i + 1));
        }
        if chars[i] == '\\' && i + 1 < chars.len() {
            // Escaped character
            content.push(chars[i + 1]);
            i += 2;
        } else {
            content.push(chars[i]);
            i += 1;
        }
    }
    // No closing quote found — return content anyway (graceful degradation)
    Some((content, i))
}

/// Try to parse a field:value filter starting at position `start`.
/// Returns the Filter and position after the value, or None if not a filter.
fn parse_filter(chars: &[char], start: usize, negated: bool) -> Option<(Filter, usize)> {
    // Look for field_name: pattern
    let mut i = start;
    let mut field_name = String::new();

    // Read the field name (letters, digits, underscores)
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
        field_name.push(chars[i]);
        i += 1;
    }

    // Must be followed by ':'
    if i >= chars.len() || chars[i] != ':' || field_name.is_empty() {
        return None;
    }

    // Must be a known filter field
    let field = FilterField::from_str(&field_name)?;

    // Skip the ':'
    i += 1;

    // Parse the value — could be quoted or bare
    if i >= chars.len() || chars[i].is_whitespace() {
        // field: with no value — not a filter
        return None;
    }

    let (value, end) = if chars[i] == '"' {
        // Quoted value: field:"value with spaces"
        match parse_quoted(chars, i) {
            Some((v, end)) => (v, end),
            None => return None,
        }
    } else {
        // Bare value: field:value (terminated by whitespace)
        parse_word(chars, i)
    };

    if value.is_empty() {
        return None;
    }

    // Split comma-separated values: lang:rust,go → ["rust", "go"]
    let values: Vec<String> = value
        .split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();

    if values.is_empty() {
        return None;
    }

    let actual_start = if negated { start - 1 } else { start };
    let _ = actual_start; // We only need `end` for the caller

    Some((
        Filter {
            field,
            values,
            negated,
        },
        end,
    ))
}

/// Parse a bare word (non-whitespace token) starting at position `start`.
/// Returns the word and the position after it.
fn parse_word(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let mut word = String::new();
    while i < chars.len() && !chars[i].is_whitespace() {
        word.push(chars[i]);
        i += 1;
    }
    (word, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let q = parse("context assembler");
        assert_eq!(q.terms, vec!["context", "assembler"]);
        assert!(q.filters.is_empty());
        assert!(q.phrases.is_empty());
        assert_eq!(q.text_query, "context assembler");
    }

    #[test]
    fn test_quoted_phrase() {
        let q = parse("\"error handling\" in rust");
        assert_eq!(q.terms, vec!["in", "rust"]);
        assert_eq!(q.phrases, vec!["error handling"]);
        assert_eq!(q.text_query, "in rust \"error handling\"");
    }

    #[test]
    fn test_single_filter() {
        let q = parse("repo:aegis PostToolUse");
        assert_eq!(q.terms, vec!["PostToolUse"]);
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].field, FilterField::Repo);
        assert_eq!(q.filters[0].values, vec!["aegis"]);
        assert!(!q.filters[0].negated);
        assert_eq!(q.text_query, "PostToolUse");
    }

    #[test]
    fn test_multiple_filters() {
        let q = parse("repo:aegis lang:rust type:function search");
        assert_eq!(q.terms, vec!["search"]);
        assert_eq!(q.filters.len(), 3);
        assert_eq!(q.filters[0].field, FilterField::Repo);
        assert_eq!(q.filters[1].field, FilterField::Lang);
        assert_eq!(q.filters[2].field, FilterField::Type);
    }

    #[test]
    fn test_negated_filter() {
        let q = parse("-repo:aegis search query");
        assert_eq!(q.terms, vec!["search", "query"]);
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].field, FilterField::Repo);
        assert_eq!(q.filters[0].values, vec!["aegis"]);
        assert!(q.filters[0].negated);
    }

    #[test]
    fn test_multi_value_filter() {
        let q = parse("lang:rust,go search");
        assert_eq!(q.terms, vec!["search"]);
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].values, vec!["rust", "go"]);
    }

    #[test]
    fn test_quoted_filter_value() {
        let q = parse("file:\"src/cli/hook.rs\" search");
        assert_eq!(q.terms, vec!["search"]);
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].field, FilterField::File);
        assert_eq!(q.filters[0].values, vec!["src/cli/hook.rs"]);
    }

    #[test]
    fn test_unknown_field_is_literal() {
        let q = parse("http:server search");
        assert_eq!(q.terms, vec!["http:server", "search"]);
        assert!(q.filters.is_empty());
    }

    #[test]
    fn test_boolean_operators_stripped() {
        let q = parse("context AND assembler");
        assert_eq!(q.terms, vec!["context", "assembler"]);
        assert!(q.filters.is_empty());

        let q2 = parse("redis OR memcached");
        assert_eq!(q2.terms, vec!["redis", "memcached"]);
    }

    #[test]
    fn test_empty_query() {
        let q = parse("");
        assert!(q.terms.is_empty());
        assert!(q.filters.is_empty());
        assert!(q.phrases.is_empty());
        assert_eq!(q.text_query, "");
    }

    #[test]
    fn test_whitespace_only() {
        let q = parse("   ");
        assert!(q.terms.is_empty());
    }

    #[test]
    fn test_filter_aliases() {
        let q1 = parse("language:rust");
        assert_eq!(q1.filters[0].field, FilterField::Lang);

        let q2 = parse("repository:aegis");
        assert_eq!(q2.filters[0].field, FilterField::Repo);

        let q3 = parse("filepath:src/main.rs");
        assert_eq!(q3.filters[0].field, FilterField::Path);

        let q4 = parse("label:reviewed");
        assert_eq!(q4.filters[0].field, FilterField::Tag);
    }

    #[test]
    fn test_filter_no_value_is_literal() {
        // "repo:" with nothing after it is treated as literal text
        let q = parse("repo: search");
        // "repo:" becomes a word because the value parse finds whitespace
        assert!(q.filters.is_empty());
    }

    #[test]
    fn test_complex_query() {
        let q = parse("repo:aegis lang:rust -type:section \"error handling\" PostToolUse");
        assert_eq!(q.terms, vec!["PostToolUse"]);
        assert_eq!(q.phrases, vec!["error handling"]);
        assert_eq!(q.filters.len(), 3);
        assert_eq!(q.filters[0].field, FilterField::Repo);
        assert!(!q.filters[0].negated);
        assert_eq!(q.filters[1].field, FilterField::Lang);
        assert_eq!(q.filters[2].field, FilterField::Type);
        assert!(q.filters[2].negated);
    }

    #[test]
    fn test_unmatched_quote_graceful() {
        // Unmatched quote should not crash — treat rest as phrase
        let q = parse("search \"unfinished");
        assert_eq!(q.terms, vec!["search"]);
        assert_eq!(q.phrases, vec!["unfinished"]);
    }

    #[test]
    fn test_path_filter_with_glob() {
        let q = parse("file:*.rs search");
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters[0].field, FilterField::File);
        assert_eq!(q.filters[0].values, vec!["*.rs"]);
    }

    #[test]
    fn test_column_names() {
        assert_eq!(FilterField::Repo.column_name(), "repo");
        assert_eq!(FilterField::Lang.column_name(), "language");
        assert_eq!(FilterField::Type.column_name(), "chunk_type");
        assert_eq!(FilterField::File.column_name(), "file_path");
        assert_eq!(FilterField::Path.column_name(), "file_path");
    }

    #[test]
    fn test_case_insensitive_field_names() {
        let q1 = parse("Repo:aegis");
        assert_eq!(q1.filters.len(), 1);
        assert_eq!(q1.filters[0].field, FilterField::Repo);

        let q2 = parse("LANG:rust");
        assert_eq!(q2.filters.len(), 1);
        assert_eq!(q2.filters[0].field, FilterField::Lang);
    }

    // -- filters_to_sql tests --

    #[test]
    fn test_sql_simple_eq() {
        let q = parse("repo:aegis");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["repo = 'aegis'"]);
    }

    #[test]
    fn test_sql_negated() {
        let q = parse("-repo:aegis");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["repo != 'aegis'"]);
    }

    #[test]
    fn test_sql_multi_value_in() {
        let q = parse("lang:rust,go");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["language IN ('rust', 'go')"]);
    }

    #[test]
    fn test_sql_negated_multi_value() {
        let q = parse("-lang:rust,go");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["language NOT IN ('rust', 'go')"]);
    }

    #[test]
    fn test_sql_glob_wildcard() {
        let q = parse("file:*.rs");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["file_path LIKE '%.rs'"]);
    }

    #[test]
    fn test_sql_path_substring() {
        let q = parse("path:src/cli");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["file_path LIKE '%src/cli%'"]);
    }

    #[test]
    fn test_sql_group_excluded() {
        let q = parse("group:infra search");
        let sql = filters_to_sql(&q.filters);
        // Group filters are handled separately — not in SQL
        assert!(sql.is_empty());
    }

    #[test]
    fn test_sql_multiple_filters() {
        let q = parse("repo:aegis lang:rust -type:section");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql.len(), 3);
        assert_eq!(sql[0], "repo = 'aegis'");
        assert_eq!(sql[1], "language = 'rust'");
        assert_eq!(sql[2], "chunk_type != 'section'");
    }

    #[test]
    fn test_sql_escapes_quotes() {
        let q = parse("repo:o'brien");
        let sql = filters_to_sql(&q.filters);
        assert_eq!(sql, vec!["repo = 'o''brien'"]);
    }

    #[test]
    fn test_extract_group_filters() {
        let q = parse("group:infra group:apps search");
        let groups = extract_group_filters(&q.filters);
        assert_eq!(groups, vec!["infra", "apps"]);
    }

    #[test]
    fn test_extract_group_filters_negated_excluded() {
        let q = parse("-group:infra search");
        let groups = extract_group_filters(&q.filters);
        assert!(groups.is_empty());
    }
}
