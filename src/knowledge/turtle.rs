//! Turtle literal escaping shared by every knowledge producer.

/// Escape `s` for use inside a Turtle `"..."` literal.
///
/// `STRING_LITERAL_QUOTE` forbids a raw `"`, `\`, LF and CR, so those must be
/// escaped. Other control characters are escaped too, so what the parser reads
/// back is exactly the value we meant. Escaping only `\` and `"` let a
/// multi-line value reach quipu raw, and the whole snapshot was refused with
/// "Line jumps are not allowed in string literals" (aegis-86f2v7.1).
pub(crate) fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape_literal;

    #[test]
    fn escapes_line_jumps_and_controls() {
        assert_eq!(escape_literal("a\nb\r\nc"), "a\\nb\\r\\nc");
        assert_eq!(escape_literal("t\tb\u{08}f\u{0C}"), "t\\tb\\bf\\f");
        assert_eq!(escape_literal("nul\u{0}del\u{7F}"), "nul\\u0000del\\u007F");
        assert_eq!(escape_literal(r#"q"s\"#), r#"q\"s\\"#);
        assert_eq!(escape_literal("plain ünïcode"), "plain ünïcode");
    }
}
