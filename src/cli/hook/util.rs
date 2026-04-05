use std::path::Path;

/// Detect the git repo name from a directory by walking up to find `.git`.
/// Returns the directory name containing `.git` (e.g. "aegis" for /home/user/gt/aegis/crew/ian/).
pub(super) fn detect_repo_name(dir: &Path) -> Option<String> {
    let mut current = dir;
    loop {
        if current.join(".git").exists() {
            return current.file_name()?.to_str().map(|s| s.to_string());
        }
        current = current.parent()?;
    }
}

/// Detect the server-side repo name from a working directory.
///
/// In Gas Town, workspaces follow the pattern `/<town>/<rig>/crew/<name>/`
/// or `/<town>/<rig>/polecats/<name>/`. The rig name (e.g., "bobbin", "aegis")
/// is the repo name on the server, while `detect_repo_name` would return "strider"
/// or "ian" (the git root directory). This function walks up looking for known
/// Gas Town directory patterns.
///
/// Falls back to `detect_repo_name` if no Gas Town structure is found.
pub(super) fn detect_server_repo_name(dir: &Path) -> Option<String> {
    // Walk up looking for a parent named "crew" or "polecats" — the grandparent is the rig/repo
    let mut current = dir.to_path_buf();
    for _ in 0..10 {
        if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
            if name == "crew" || name == "polecats" {
                // Parent of crew/polecats is the rig name (= server repo name)
                return current.parent()?.file_name()?.to_str().map(|s| s.to_string());
            }
        }
        if !current.pop() {
            break;
        }
    }
    // Fallback: use git root dir name
    detect_repo_name(dir)
}

/// Strip XML tag blocks from prompt text that pollute semantic search.
/// System boilerplate, tool schemas, previous injections, and tool call output
/// all add noise to embedding queries without providing useful search signal.
pub(super) fn strip_system_tags(text: &str) -> String {
    let result = text.to_string();
    // System boilerplate (hook output, nudge metadata, task reminders)
    let result = strip_xml_block(&result, "system-reminder");
    let result = strip_xml_block(&result, "task-notification");
    // Tool name lists and schemas (large JSON blobs)
    let result = strip_xml_block(&result, "available-deferred-tools");
    let result = strip_xml_block(&result, "functions");
    // Previous bobbin injection output re-submitted in prompts
    let result = strip_xml_block(&result, "bobbin-context");
    // Tool call/result output from Claude (XML tool use blocks)
    let result = strip_xml_block(&result, "function_calls");
    let result = strip_xml_block(&result, "function_results");
    let result = strip_xml_block(&result, "antml:function_calls");
    let result = strip_xml_block(&result, "antml:invoke");
    // Example blocks from system prompts
    let result = strip_xml_block(&result, "example");
    let result = strip_xml_block(&result, "example_agent_descriptions");
    // Claude's internal reasoning blocks (re-submitted in context)
    let result = strip_xml_block(&result, "antml:thinking");
    // System prompt metadata blocks
    let result = strip_xml_block(&result, "fast_mode_info");
    let result = strip_xml_block(&result, "types");
    result
}

/// Strip all occurrences of `<tag>...</tag>` from text.
pub(super) fn strip_xml_block(text: &str, tag: &str) -> String {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find(&open) {
        result.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find(&close) {
            remaining = &remaining[start + end + close.len()..];
        } else {
            remaining = "";
            break;
        }
    }
    result.push_str(remaining);
    result
}

/// Detect short prompts that are bead/issue commands (e.g., "remove bo-qq5h",
/// "show aegis-abc", "close gt-xyz"). These are operational commands that don't
/// benefit from search context injection. Bead IDs match: prefix-alphanumeric.
pub(super) fn is_bead_command(prompt: &str) -> bool {
    if prompt.len() > 60 {
        return false;
    }
    let words: Vec<&str> = prompt.split_whitespace().collect();
    if words.len() > 5 {
        return false;
    }
    words.iter().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if let Some(dash_pos) = w.find('-') {
            let prefix = &w[..dash_pos];
            let suffix = &w[dash_pos + 1..];
            !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_lowercase())
                && !suffix.is_empty()
                && suffix.len() >= 3
                && suffix.chars().all(|c| c.is_ascii_alphanumeric())
        } else {
            false
        }
    })
}

/// Detect automated messages that don't benefit from semantic search.
/// Auto-patrol nudges, reactor alerts, and similar machine-generated messages
/// produce noise injections (matching docs about "escalation", "patrol", etc.)
/// rather than useful context for the agent's actual work.
pub(super) fn is_automated_message(prompt: &str) -> bool {
    // Trim leading whitespace — prompts may start with \n from nudge/hook wrappers
    let prompt = prompt.trim_start();
    // Check first 500 chars for efficiency (patterns appear early in messages)
    let check = if prompt.len() > 500 { &prompt[..500] } else { prompt };

    // Auto-patrol nudge patterns (from crew-patrol.sh / gt nudge)
    if check.contains("Auto-patrol: pick up") || check.contains("PATROL LOOP") {
        return true;
    }
    if check.contains("RANGER PATROL:") || check.contains("PATROL:") {
        return true;
    }

    // Reactor alert patterns
    if check.contains("[reactor]") && (check.contains("ESCALATION:") || check.contains("P1 bead:") || check.contains("P0 bead:")) {
        return true;
    }

    // Repeated automated work nudges (pattern: same message duplicated many times)
    if check.contains("WORK: You are") && check.contains("Keep working until context") {
        return true;
    }

    // Startup/handoff messages — these contain system boilerplate, not domain queries
    if check.contains("HANDOFF COMPLETE") && check.contains("You are the NEW session") {
        return true;
    }
    if check.contains("STARTUP PROTOCOL") && check.contains("gt hook") {
        return true;
    }

    // Marshal/dog automated checks
    if check.contains("Marshal check:") && check.contains("You appear idle") {
        return true;
    }

    // Queued nudge wrappers (system envelope, not user intent)
    if check.contains("QUEUED NUDGE") && check.contains("background notification") {
        return true;
    }

    // Session start hook output (system boilerplate injected at conversation start)
    if check.contains("SessionStart:startup hook") || check.contains("[GAS TOWN]") && check.contains("session:") {
        return true;
    }

    // Reactor alert nudges (always have "[reactor] P" followed by priority + "bead:")
    if check.contains("[reactor] P") && check.contains("bead:") {
        return true;
    }

    // Agent role announcements ("Crew ian, checking in.", "aegis Crew mel, checking in.")
    if check.contains("checking in") && check.contains("Crew ") {
        return true;
    }

    // System reminder blocks (hook output injected into prompts)
    if check.starts_with("<system-reminder>") || check.starts_with("[GAS TOWN]") {
        return true;
    }

    // Handoff mail content — "Check your hook and mail" directives
    if check.contains("Check your hook") && check.contains("mail") && check.contains("then act") {
        return true;
    }

    // Handoff continuation — "[GAS TOWN] crew" + "handoff" patterns
    if check.contains("[GAS TOWN]") && check.contains("handoff") {
        return true;
    }

    // Tool loaded / tool result acknowledgments (no domain content)
    let trimmed = check.trim();
    if trimmed == "Tool loaded." || trimmed == "Acknowledged."
        || trimmed == "Continue." || trimmed == "OK" || trimmed == "ok"
        || trimmed == "Go ahead." || trimmed == "Proceed."
        || trimmed.starts_with("Tool loaded")
        || trimmed.starts_with("Human: Tool loaded")
    {
        return true;
    }

    // Crew role assignment / WORK directives (automated dispatching)
    if check.contains("Your differentiated work:") && check.contains("Keep working until") {
        return true;
    }

    // Overseer work assignment nudges (repeated automated directive)
    if check.contains("WORK: You are") {
        return true;
    }

    // "IMPORTANT: After completing" task continuation reminders
    if check.contains("IMPORTANT: After completing your current task") {
        return true;
    }

    // Repeated WORK directives with partial match (without "Keep working" suffix)
    if check.contains("WORK: You are") && check.contains("differentiated work") {
        return true;
    }

    // System reminder about task tools (injected by harness, not user)
    if check.contains("task tools haven't been used recently") {
        return true;
    }

    // Molecule/convoy status checks (orchestration, not domain work)
    if check.contains("gt mol status") && check.contains("gt hook") {
        return true;
    }

    // Very short prompts that are just confirmations (< 15 chars, no technical terms)
    if trimmed.len() < 15
        && !trimmed.contains('_')
        && !trimmed.contains('.')
        && !trimmed.contains("::")
        && trimmed.split_whitespace().count() <= 3
    {
        let lower = trimmed.to_lowercase();
        let confirmation_words = [
            "yes", "no", "ok", "sure", "thanks", "done", "good", "fine",
            "right", "correct", "agreed", "continue", "proceed", "next",
            "go", "yep", "nope", "ack", "roger", "noted",
        ];
        if confirmation_words.iter().any(|w| lower == *w || lower.starts_with(w)) {
            return true;
        }
    }

    false
}

/// Extract brief primer text (title + first section only).
pub(super) fn extract_brief(primer: &str) -> String {
    let mut result = String::new();
    let mut heading_count = 0;
    for line in primer.lines() {
        if line.starts_with("## ") {
            heading_count += 1;
            if heading_count > 1 {
                break;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result.trim_end().to_string()
}

/// Extract a search query from a grep/rg/find bash command.
/// Returns None if the command doesn't look like a search command.
pub(super) fn extract_search_query_from_bash(command: &str) -> Option<String> {
    let cmd = command.trim();

    // Match: grep [-flags] "pattern" or grep [-flags] pattern
    // Also matches: rg, git grep
    // Strategy: find the command, skip flags (start with -), take the next arg as pattern
    let search_cmds = ["grep", "rg"];
    for search_cmd in &search_cmds {
        // Find the command (could be prefixed with env vars, pipes, etc.)
        // Look for the command as a word boundary
        if let Some(pos) = cmd.find(search_cmd) {
            // Make sure it's a command start (beginning, after pipe, after &&, after ;, after space)
            if pos > 0 {
                let before = cmd[..pos].chars().last().unwrap_or(' ');
                if !before.is_whitespace() && before != '|' && before != ';' && before != '&' {
                    continue;
                }
            }
            // Extract everything after the command name
            let after_cmd = &cmd[pos + search_cmd.len()..];
            if let Some(pattern) = extract_pattern_from_args(after_cmd) {
                return Some(pattern);
            }
        }
    }

    // Match: find . -name "pattern" — extract the name pattern
    if let Some(pos) = cmd.find("find") {
        if pos == 0 || cmd[..pos].chars().last().map_or(true, |c| c.is_whitespace() || c == '|' || c == ';' || c == '&') {
            let after_cmd = &cmd[pos + 4..];
            if let Some(pattern) = extract_find_pattern(after_cmd) {
                return Some(pattern);
            }
        }
    }

    None
}

/// Extract pattern from grep/rg argument list.
/// Skips flags (starting with -), takes the first non-flag argument.
pub(super) fn extract_pattern_from_args(args: &str) -> Option<String> {
    let args = args.trim();
    // Use a simple state machine to handle quoted strings
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in args.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if !in_single_quote => escape_next = true,
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    // Skip flags and flag arguments, handle -e/--regexp specially (pattern flag)
    let mut i = 0;
    let mut explicit_pattern: Option<String> = None;
    // Flags that take a value argument (next token is NOT the pattern)
    let flags_with_value = [
        "-f", "--file", "-A", "-B", "-C", "--context",
        "--color", "--colours", "-m", "--max-count", "--include", "--exclude",
        "--type", "-t", "--type-add", "--glob", "-g", "--max-depth",
        "--threads", "-j", "--after-context", "--before-context",
    ];
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "--" {
            // Everything after -- is positional
            i += 1;
            break;
        }
        if tok == "-e" || tok == "--regexp" {
            // -e pattern — the next arg IS the pattern
            if i + 1 < tokens.len() {
                explicit_pattern = Some(tokens[i + 1].clone());
            }
            i += 2;
        } else if tok.starts_with('-') {
            // Check if this flag takes a value
            if flags_with_value.iter().any(|f| tok == f) {
                i += 2; // skip flag and its value
            } else if tok.starts_with("--") && tok.contains('=') {
                i += 1; // --flag=value
            } else {
                i += 1; // simple flag like -r, -i, -n
            }
        } else {
            break; // first positional = pattern
        }
    }

    // If -e was used, prefer that pattern
    if let Some(p) = explicit_pattern {
        let cleaned = clean_regex_for_search(&p);
        if !cleaned.is_empty() && p.len() >= 2 && p.len() <= 200 {
            return Some(cleaned);
        }
    }

    if i < tokens.len() {
        let pattern = &tokens[i];
        // Skip very short patterns (likely noise) and very long ones (likely paths)
        if pattern.len() >= 2 && pattern.len() <= 200 {
            // Clean up regex-specific syntax for semantic search
            let cleaned = clean_regex_for_search(pattern);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }

    None
}

/// Extract search intent from find command arguments.
/// Looks for -name/-iname/-path patterns.
pub(super) fn extract_find_pattern(args: &str) -> Option<String> {
    let args = args.trim();
    let parts: Vec<&str> = args.split_whitespace().collect();

    for i in 0..parts.len().saturating_sub(1) {
        if parts[i] == "-name" || parts[i] == "-iname" || parts[i] == "-path" || parts[i] == "-ipath" {
            let pattern = parts[i + 1].trim_matches('"').trim_matches('\'');
            // Strip glob wildcards for semantic search
            let cleaned = pattern
                .replace("*.", "")
                .replace(".*", "")
                .replace('*', " ")
                .trim()
                .to_string();
            if cleaned.len() >= 2 {
                return Some(cleaned);
            }
        }
    }

    None
}

/// Clean regex pattern for use as a semantic search query.
/// Strips regex metacharacters and converts to readable text.
pub(super) fn clean_regex_for_search(pattern: &str) -> String {
    pattern
        .replace("\\s+", " ")
        .replace("\\s*", " ")
        .replace("\\b", "")
        .replace("\\w+", "")
        .replace("\\d+", "")
        .replace(".*", " ")
        .replace(".+", " ")
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\{", "{")
        .replace("\\}", "}")
        .replace("\\[", "[")
        .replace("\\]", "]")
        .replace(['(', ')', '[', ']', '{', '}', '^', '$', '|', '?', '+'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if a cleaned search query is meaningful enough for semantic search.
/// Rejects too-short queries, pure language keywords, and file extensions.
pub(super) fn is_meaningful_search_query(query: &str) -> bool {
    let q = query.trim();
    // Too short — likely noise
    if q.len() < 3 {
        return false;
    }
    // Single token that's a common language keyword or file extension — too generic
    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.len() == 1 {
        let lower = tokens[0].to_lowercase();
        let noise_words = [
            "fn", "let", "var", "const", "use", "import", "from", "return",
            "if", "else", "for", "while", "match", "type", "struct", "enum",
            "class", "def", "func", "pub", "mod", "crate", "self", "super",
            "rs", "go", "py", "ts", "js", "tsx", "jsx", "md", "toml", "yaml",
            "yml", "json", "html", "css", "sh", "bash", "txt",
        ];
        if noise_words.contains(&lower.as_str()) {
            return false;
        }
    }
    true
}

/// Check if a file path points to source code (where symbol refs are useful).
pub(super) fn is_source_code_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext,
        "rs" | "go" | "py" | "ts" | "tsx" | "js" | "jsx" | "java" | "c" | "cpp"
            | "h" | "hpp" | "cs" | "rb" | "swift" | "kt" | "scala" | "zig" | "lua"
            | "ex" | "exs" | "erl" | "hs" | "ml" | "mli" | "fs" | "fsi"
    )
}
