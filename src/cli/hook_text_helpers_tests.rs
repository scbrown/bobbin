//! Tests for `hook.rs`'s pure text helpers — bash-command pattern extraction,
//! query meaningfulness, system-tag stripping, automated-message and
//! bead-command detection.
//!
//! Out of `src/cli/hook.rs` for the reason `hook_session_id_tests.rs` states:
//! that file is capped by `scripts/large-file-allowlist.txt` at the size it
//! had when its ceiling was set, and test bodies are exempt from the gate only
//! when they live in a `*tests.rs` file. Splitting a block out is how the
//! ratchet is meant to be tightened; raising the ceiling is not.
//!
//! These are all pure functions over `&str` with no shared fixture, which is
//! why they move as one block and leave nothing behind.

use super::*;

#[test]
fn test_extract_grep_pattern() {
    // Basic grep
    assert_eq!(
        extract_search_query_from_bash("grep -r \"Stmt::Import\" src/"),
        Some("Stmt::Import".to_string())
    );

    // rg with type flag
    assert_eq!(
        extract_search_query_from_bash("rg \"fn main\" --type rust"),
        Some("fn main".to_string())
    );

    // grep with -i flag
    assert_eq!(
        extract_search_query_from_bash("grep -ri \"error handling\" ."),
        Some("error handling".to_string())
    );

    // rg with single quotes
    assert_eq!(
        extract_search_query_from_bash("rg 'impl Display' src/"),
        Some("impl Display".to_string())
    );

    // git grep
    assert_eq!(
        extract_search_query_from_bash("git grep \"TODO\" -- '*.rs'"),
        Some("TODO".to_string())
    );

    // Not a grep command
    assert_eq!(
        extract_search_query_from_bash("cargo build --release"),
        None
    );

    // grep with -e flag (pattern follows -e)
    assert_eq!(
        extract_search_query_from_bash("grep -r -e \"pattern\" src/"),
        Some("pattern".to_string())
    );
}

#[test]
fn test_extract_find_pattern() {
    // find with -name
    assert_eq!(
        extract_search_query_from_bash("find . -name \"*.test.rs\""),
        Some("test.rs".to_string())
    );

    // find with -iname
    assert_eq!(
        extract_search_query_from_bash("find src/ -iname \"*.py\""),
        Some("py".to_string())
    );

    // find without -name
    assert_eq!(extract_search_query_from_bash("find . -type f"), None);
}

#[test]
fn test_clean_regex_for_search() {
    assert_eq!(clean_regex_for_search("fn\\s+main"), "fn main");
    assert_eq!(clean_regex_for_search("impl.*Display"), "impl Display");
    assert_eq!(clean_regex_for_search("^use\\b"), "use");
    assert_eq!(clean_regex_for_search("Stmt::Import"), "Stmt::Import");
}

#[test]
fn test_is_meaningful_search_query() {
    // Too short
    assert!(!is_meaningful_search_query(""));
    assert!(!is_meaningful_search_query("fn"));
    assert!(!is_meaningful_search_query("rs"));

    // Single noise words (language keywords, file extensions)
    assert!(!is_meaningful_search_query("let"));
    assert!(!is_meaningful_search_query("import"));
    assert!(!is_meaningful_search_query("toml"));
    assert!(!is_meaningful_search_query("json"));

    // Meaningful queries
    assert!(is_meaningful_search_query("PostToolUse"));
    assert!(is_meaningful_search_query("context assembler"));
    assert!(is_meaningful_search_query("fn main")); // multi-word is fine
    assert!(is_meaningful_search_query("search query"));
    assert!(is_meaningful_search_query("ContextConfig"));
}

#[test]
fn test_strip_system_tags() {
    // System reminder blocks
    assert_eq!(
        strip_system_tags("Hello <system-reminder>noise</system-reminder> world"),
        "Hello  world"
    );
    // Task notification blocks
    assert_eq!(
        strip_system_tags("Query <task-notification>task-id: abc</task-notification> here"),
        "Query  here"
    );
    // Both types together
    let input = "<system-reminder>sys</system-reminder>real content<task-notification>task</task-notification>";
    assert_eq!(strip_system_tags(input), "real content");
    // No tags
    assert_eq!(strip_system_tags("plain text"), "plain text");
}

#[test]
fn test_is_automated_message() {
    // Patrol nudges
    assert!(is_automated_message(
        "Auto-patrol: pick up aegis-abc123 (Some task). Run: bd show aegis-abc123"
    ));
    assert!(is_automated_message(
        "PATROL LOOP — you must keep working until context is below 20%."
    ));
    assert!(is_automated_message(
        "RANGER PATROL: You are a ranger. Patrol your domain."
    ));
    assert!(is_automated_message(
        "PATROL: Run gt hook, gt mail inbox, bd ready."
    ));

    // Reactor alerts
    assert!(is_automated_message(
        "[reactor] ⚠️ ESCALATION: E2ESmokeTestFailing — node-5 | Paging: aegis/crew/wu"
    ));
    assert!(is_automated_message(
        "[reactor] 🟠 P1 bead: aegis-sc86f0 Skills Framework Phase 1"
    ));
    assert!(is_automated_message(
        "[reactor] 🟠 P0 bead: aegis-thmbt2 Claude token expires"
    ));

    // Repeated work nudges
    assert!(is_automated_message("WORK: You are stryder (Bobbin Ranger). Check gt hook and gt mail inbox. Keep working until context below 25%, then /handoff."));

    // Startup/handoff messages
    assert!(is_automated_message("╔══════╗\n║  ✅ HANDOFF COMPLETE - You are the NEW session  ║\n╚══════╝\nYour predecessor handed off to you."));
    assert!(is_automated_message(
        "**STARTUP PROTOCOL**: Please:\n1. Run `gt hook` — What's hooked?"
    ));

    // Marshal/dog checks
    assert!(is_automated_message(
        "[from dog] Marshal check: You appear idle (7+ days no commits). Check bd ready."
    ));

    // Queued nudge wrappers
    assert!(is_automated_message("QUEUED NUDGE (1 message(s)):\n\n  [from dog] check status\n\nThis is a background notification. Continue current work."));

    // Agent role announcements
    assert!(is_automated_message("aegis Crew ian, checking in."));
    assert!(is_automated_message("\naegis Crew mel, checking in.\n"));

    // System reminder blocks
    assert!(is_automated_message(
        "<system-reminder>\nUserPromptSubmit hook success\n</system-reminder>"
    ));
    assert!(is_automated_message(
        "[GAS TOWN] crew ian (rig: aegis) <- self"
    ));

    // Handoff mail directives
    assert!(is_automated_message(
        "Check your hook and mail, then act on the hook if present:\n1. `gt hook`"
    ));

    // Normal messages should NOT be filtered
    assert!(!is_automated_message("Fix the bug in bobbin search"));
    assert!(!is_automated_message("How do I deploy bobbin to node-4?"));
    assert!(!is_automated_message("bd show aegis-abc123"));
    assert!(!is_automated_message(
        "Run the tests and check for failures"
    ));
    assert!(!is_automated_message("")); // Empty string

    // Whitespace-trimmed patterns should still match
    assert!(is_automated_message(
        "  \n<system-reminder>\nhook output\n</system-reminder>"
    ));
    assert!(is_automated_message(
        "\n[GAS TOWN] crew ian (rig: aegis) <- self"
    ));
}

#[test]
fn test_is_bead_command() {
    // Bead commands that should be skipped
    assert!(is_bead_command("remove bo-qq5h"));
    assert!(is_bead_command("show aegis-abc123"));
    assert!(is_bead_command("close gt-xyz"));
    assert!(is_bead_command("hook gt-h8x"));
    assert!(is_bead_command("bd show aegis-ky3wc9"));
    assert!(is_bead_command("unhook hq-abc"));
    assert!(is_bead_command("aegis-mlpgac"));

    // Should NOT be skipped (not bead commands)
    assert!(!is_bead_command("Fix the bug in bobbin search"));
    assert!(!is_bead_command("How do I deploy bobbin to node-4?"));
    assert!(!is_bead_command("Run the tests and check for failures"));
    assert!(!is_bead_command("")); // Empty string
    assert!(!is_bead_command(
        "what is the architecture of the system and how does deployment work across all rigs"
    ));
    // Too short suffix (< 3 chars)
    assert!(!is_bead_command("show x-ab"));
    // Not lowercase prefix
    assert!(!is_bead_command("show ABC-def123"));
}
