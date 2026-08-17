/// Query intent classification for adjusting search parameters.
///
/// Classifies prompts into intents based on keyword signals, then returns
/// parameter adjustments for context assembly. ZFC compliant: pure keyword
/// matching, no ML or adaptive behavior.
use serde::{Deserialize, Serialize};

/// Detected intent of a user prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    /// Bug fix: error messages, stack traces, "fix", "broken"
    BugFix,
    /// Architecture: "how does", "explain", "architecture", "design"
    Architecture,
    /// Implementation: "add", "implement", "create", "build"
    Implementation,
    /// Configuration: "config", "deploy", "setup", "install"
    Configuration,
    /// Navigation: "where is", "find", "locate", "which file"
    Navigation,
    /// Operational: tool execution, git commands, test runs — no code context needed
    Operational,
    /// General: no strong signal detected
    General,
}

/// Parameter adjustments based on detected intent.
#[derive(Debug, Clone)]
pub struct IntentAdjustments {
    /// Multiplier for doc_demotion (< 1.0 means docs are less demoted = more visible)
    pub doc_demotion_factor: f32,
    /// Multiplier for semantic_weight (> 1.0 means more semantic, < 1.0 more keyword)
    pub semantic_weight_factor: f32,
    /// Multiplier for recency_weight (> 1.0 means prefer recent, < 1.0 means less recency bias)
    pub recency_weight_factor: f32,
    /// Additive boost to gate threshold (0.0 means no change, positive raises the bar)
    pub gate_boost: f32,
    /// Override for coupling_threshold (None = use config default).
    /// Higher = stricter coupling (only strongly coupled files).
    pub coupling_threshold: Option<f32>,
}

impl Default for IntentAdjustments {
    fn default() -> Self {
        Self {
            doc_demotion_factor: 1.0,
            semantic_weight_factor: 1.0,
            recency_weight_factor: 1.0,
            gate_boost: 0.0,
            coupling_threshold: None,
        }
    }
}

/// Apply an intent's `doc_demotion_factor` to a configured `doc_demotion`.
///
/// **`doc_demotion` is a score MULTIPLIER, not an amount of demotion**: 1.0
/// leaves doc scores untouched, 0.0 suppresses them entirely. So a *lower*
/// `doc_demotion` is *stronger* demotion, and the factor cannot be applied by
/// multiplying it — that moves the result the wrong way. The factor scales the
/// demotion EFFECT, which is `1.0 - doc_demotion`; invert, scale, invert back.
///
/// Worked, with `base = 0.5` and BugFix's `factor = 1.5`:
///
/// ```text
///   effect  = (1.0 - 0.5) * 1.5 = 0.75
///   result  =  1.0 - 0.75       = 0.25   -> lower than 0.5, so MORE demotion
///   (naive) =        0.5  * 1.5 = 0.75   -> higher than 0.5, so LESS demotion
/// ```
///
/// This exists as one function because it was written twice and the two copies
/// disagreed: the remote hook path used effect space and the local path
/// raw-multiplied, so the same intent strengthened demotion on one path and
/// weakened it on the other (bobbin-lpp). Both paths now call this, which is
/// what makes the divergence structurally impossible rather than merely fixed.
///
/// `floor` differs by caller and is deliberately a parameter: the local path
/// keeps a 0.01 floor so a doc score is never multiplied to exactly zero, the
/// remote path allows full suppression at 0.0.
pub fn apply_doc_demotion_factor(base: f32, factor: f32, floor: f32) -> f32 {
    let effect = (1.0 - base) * factor;
    (1.0 - effect).clamp(floor, 1.0)
}

/// Classify a prompt's intent based on keyword signals.
pub fn classify_intent(prompt: &str) -> QueryIntent {
    let lower = prompt.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // Score each intent by counting matching signals
    let mut scores = [
        (QueryIntent::BugFix, 0i32),
        (QueryIntent::Architecture, 0),
        (QueryIntent::Implementation, 0),
        (QueryIntent::Configuration, 0),
        (QueryIntent::Navigation, 0),
        (QueryIntent::Operational, 0),
    ];

    // Helper: check if any word starts with keyword (handles "failing" matching "fail")
    let has_word = |kw: &str| -> bool {
        words.iter().any(|w| w.starts_with(kw))
    };

    // Bug fix signals
    let bugfix_stems = ["fix", "bug", "broke", "error", "crash", "fail", "wrong", "issue", "debug", "traceback", "panic", "exception", "stack"];
    let bugfix_phrases = ["doesn't work", "does not work", "not working", "is broken", "stopped working"];
    for kw in &bugfix_stems {
        if has_word(kw) { scores[0].1 += 1; }
    }
    for phrase in &bugfix_phrases {
        if lower.contains(phrase) { scores[0].1 += 2; }
    }
    if lower.contains("error[") || lower.contains("error:") || lower.contains("exception") {
        scores[0].1 += 2;
    }

    // Architecture signals
    let arch_stems = ["architect", "design", "explain", "overview", "understand", "structur", "diagram", "pattern"];
    let arch_phrases = ["how does", "how do", "how is", "what is the", "walk me through"];
    for kw in &arch_stems {
        if has_word(kw) { scores[1].1 += 1; }
    }
    for phrase in &arch_phrases {
        if lower.contains(phrase) { scores[1].1 += 2; }
    }

    // Implementation signals (implement/create/build are strong signals worth 2)
    let impl_strong = ["implement", "creat", "add"];
    let impl_stems = ["build", "write", "make", "feature", "extend"];
    let impl_phrases = ["add a", "create a", "build a", "implement a", "write a", "add support",
                        "add the", "add new", "add an", "build the", "build new",
                        "create the", "create new", "write the", "write new"];
    for kw in &impl_strong {
        if has_word(kw) { scores[2].1 += 2; }
    }
    for kw in &impl_stems {
        if has_word(kw) { scores[2].1 += 1; }
    }
    for phrase in &impl_phrases {
        if lower.contains(phrase) { scores[2].1 += 2; }
    }

    // Configuration signals
    let config_stems = ["config", "deploy", "setup", "install", "env", "environment", "dockerfile", "yaml", "toml", "nginx", "traefik", "systemd", "systemctl"];
    let config_phrases = ["set up", "how to configure", "how to deploy", "how to install",
                          "deploy to", "deploy the", "deploy this", "deploy it"];
    for kw in &config_stems {
        if has_word(kw) { scores[3].1 += 1; }
    }
    for phrase in &config_phrases {
        if lower.contains(phrase) { scores[3].1 += 2; }
    }

    // Navigation signals
    let nav_stems = ["where", "find", "locate", "which", "file", "path", "defin", "declarat"];
    let nav_phrases = ["where is", "where are", "which file", "find the", "locate the", "defined in",
                       "look at", "read the", "open the", "show me the file", "what file",
                       "search for", "grep for", "look up"];
    for kw in &nav_stems {
        if has_word(kw) { scores[4].1 += 1; }
    }
    for phrase in &nav_phrases {
        if lower.contains(phrase) { scores[4].1 += 2; }
    }

    // Operational signals: tool execution, git/cargo/test commands, status checks,
    // agent workflow queries (beads, hooks, mail, handoff), infrastructure monitoring
    let op_stems = ["commit", "push", "pull", "merge", "rebase", "stash", "checkout", "check", "status", "close",
                     "remove", "delete", "rename", "hook", "sling", "nudge"];
    let op_phrases = [
        "run the test", "run test", "cargo test", "cargo build", "cargo check",
        "go test", "npm test", "npm run", "make test", "make build",
        "git push", "git pull", "git commit", "git merge", "git rebase",
        "git stash", "git checkout", "git status", "git diff", "git log",
        "bd close", "bd ready", "bd list", "bd show", "bd update",
        "gt hook", "gt mail", "gt handoff", "gt sling", "gt nudge",
        "check status", "check the status", "check if tests pass",
        "push the code", "push this", "commit this", "commit the",
        "land this", "ship it", "merge this",
        // Agent workflow phrases — these are about process, not code
        "what's next", "what is next", "next task", "next bead",
        "ready beads", "ready queue", "what's on my hook", "check my hook",
        "check mail", "check inbox", "read mail", "read inbox",
        "checking in", "session start", "hand off", "handoff",
        "pick up work", "pick next", "what should i work on",
        // Bead assignment/status queries
        "assigned to me", "my beads", "my issues", "my tasks",
        "beads assigned", "open beads", "in progress beads",
        "close this bead", "close the bead", "update the bead",
        "close your beads", "close beads", "check the status",
        "check status", "check your", "check on",
        // Infrastructure monitoring — runtime queries about services, not code
        "disk usage", "disk space", "memory usage", "cpu usage", "cpu load",
        "service status", "container status", "is it up", "is it down",
        "is it running", "restart the", "restart service",
        "how much disk", "how much memory", "how much space",
        "free space", "uptime", "health check",
        "alert firing", "alert status", "prometheus", "grafana",
        "backup status", "cert expir",
        // Review/diff — operational, not code context
        "review the pr", "review this pr", "review the diff", "show the diff",
        "what changed", "what did i change", "show changes",
        "git show", "git blame", "git shortlog",
        // Session lifecycle
        "cycle session", "new session", "fresh session", "context low",
        "context is low", "running low on context",
    ];
    // Monitoring queries often compete with "what is the" (Architecture) — boost them
    let op_strong_phrases = [
        "disk usage", "disk space", "memory usage", "cpu usage", "cpu load",
        "how much disk", "how much memory", "how much space",
        "service status", "container status", "backup status",
        "alert firing", "alert status",
        "is it up", "is it down", "is it running",
    ];
    for phrase in &op_strong_phrases {
        if lower.contains(phrase) { scores[5].1 += 1; } // Extra point on top of the +2 above
    }
    // Short bead/gt management commands: "remove bo-qq5h", "hook xyz", "show aegis-abc"
    let mgmt_verbs = ["remove", "delete", "hook", "unhook", "sling", "show", "claim"];
    if words.len() <= 4 {
        for verb in &mgmt_verbs {
            if has_word(verb) { scores[5].1 += 2; }
        }
    }
    // Strong signal: prompt IS a command (very short, starts with tool name)
    let cmd_prefixes = ["git ", "cargo ", "go ", "npm ", "make ", "bd ", "gt ", "docker "];
    for phrase in &op_phrases {
        if lower.contains(phrase) { scores[5].1 += 2; }
    }
    for kw in &op_stems {
        if has_word(kw) { scores[5].1 += 1; }
    }
    // If the entire prompt looks like a shell command, strong operational signal
    for prefix in &cmd_prefixes {
        if lower.starts_with(prefix) && words.len() <= 6 { scores[5].1 += 3; }
    }

    // Return highest scoring intent (minimum threshold of 2 to avoid false positives)
    let best = scores.iter().max_by_key(|(_, s)| *s).unwrap();
    if best.1 >= 2 {
        best.0
    } else {
        QueryIntent::General
    }
}

/// Get parameter adjustments for a detected intent.
pub fn intent_adjustments(intent: QueryIntent) -> IntentAdjustments {
    match intent {
        QueryIntent::BugFix => IntentAdjustments {
            doc_demotion_factor: 1.5,    // Demote docs more (focus on code)
            semantic_weight_factor: 0.8,  // Slightly more keyword (error messages are literal)
            recency_weight_factor: 1.5,   // Prefer recent code (bugs are in recent changes)
            gate_boost: 0.0,
            coupling_threshold: None,     // Use config default (bugs hide in related code)
        },
        QueryIntent::Architecture => IntentAdjustments {
            doc_demotion_factor: 0.3,    // Docs are very relevant for architecture questions
            semantic_weight_factor: 1.2,  // More semantic (conceptual queries)
            recency_weight_factor: 0.5,   // Recency less important for architecture
            gate_boost: 0.0,
            coupling_threshold: Some(0.10), // Looser: design patterns spread across files
        },
        QueryIntent::Implementation => IntentAdjustments {
            doc_demotion_factor: 1.2,    // Slightly demote docs (code patterns matter more)
            semantic_weight_factor: 1.0,  // Balanced
            recency_weight_factor: 1.0,   // Balanced
            gate_boost: 0.0,
            coupling_threshold: None,     // Use config default
        },
        QueryIntent::Configuration => IntentAdjustments {
            doc_demotion_factor: 0.5,    // Config files and docs both relevant
            semantic_weight_factor: 0.7,  // More keyword (config terms are literal)
            recency_weight_factor: 0.8,   // Slightly less recency bias
            gate_boost: 0.0,
            coupling_threshold: Some(0.20), // Tighter: config files are specific
        },
        QueryIntent::Navigation => IntentAdjustments {
            doc_demotion_factor: 1.0,    // Balanced
            semantic_weight_factor: 0.5,  // More keyword (looking for exact names)
            recency_weight_factor: 0.3,   // Recency irrelevant for navigation
            gate_boost: 0.0,
            coupling_threshold: Some(0.25), // Tight: precision over recall
        },
        QueryIntent::Operational => IntentAdjustments {
            doc_demotion_factor: 2.0,    // Strongly demote docs
            semantic_weight_factor: 0.5,  // Keyword-heavy (command names are literal)
            recency_weight_factor: 0.5,   // Recency irrelevant
            gate_boost: 0.10,            // Raise gate from 0.50 → 0.60 (blocks low-signal noise without gating legitimate queries)
            coupling_threshold: Some(0.30), // Very tight: operational queries rarely need coupling
        },
        QueryIntent::General => IntentAdjustments {
            gate_boost: 0.08,            // Raise gate (0.50 → 0.58) to filter marginal noise
            coupling_threshold: Some(0.20), // Tighter than default (0.15) — General queries produce loose coupling noise
            ..IntentAdjustments::default()
        },
    }
}

#[cfg(test)]
#[path = "intent_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "eval_prompt_intent_tests.rs"]
mod eval_prompt_intent_tests;
