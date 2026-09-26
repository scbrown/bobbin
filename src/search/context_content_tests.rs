use super::*;

#[test]
fn test_content_mode_full() {
    let result = format_content("line1\nline2\nline3\nline4\nline5", ContentMode::Full);
    assert_eq!(
        result,
        Some("line1\nline2\nline3\nline4\nline5".to_string())
    );
}

#[test]
fn test_content_mode_preview_long() {
    let result = format_content("line1\nline2\nline3\nline4\nline5", ContentMode::Preview);
    assert_eq!(result, Some("line1\nline2\nline3...".to_string()));
}

#[test]
fn test_content_mode_preview_short() {
    let result = format_content("line1\nline2", ContentMode::Preview);
    assert_eq!(result, Some("line1\nline2".to_string()));
}

#[test]
fn test_content_mode_none() {
    let result = format_content("line1\nline2\nline3", ContentMode::None);
    assert!(result.is_none());
}
