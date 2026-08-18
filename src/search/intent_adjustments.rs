//! Per-intent retrieval adjustments — the tuning table behind
//! [`super::QueryIntent`].
//!
//! Split from `intent` under the file-size ratchet. The seam is the natural
//! one: `intent` decides WHAT a query is, this decides what that classification
//! costs a search. The numbers here are tuned independently of the classifier
//! and are read far more often than they are changed.

use super::{IntentAdjustments, QueryIntent};

/// Get parameter adjustments for a detected intent.
pub fn intent_adjustments(intent: QueryIntent) -> IntentAdjustments {
    match intent {
        QueryIntent::BugFix => IntentAdjustments {
            doc_demotion_factor: 1.5,    // Demote docs more (focus on code)
            semantic_weight_factor: 0.8, // Slightly more keyword (error messages are literal)
            recency_weight_factor: 1.5,  // Prefer recent code (bugs are in recent changes)
            gate_boost: 0.0,
            coupling_threshold: None, // Use config default (bugs hide in related code)
        },
        QueryIntent::Architecture => IntentAdjustments {
            doc_demotion_factor: 0.3, // Docs are very relevant for architecture questions
            semantic_weight_factor: 1.2, // More semantic (conceptual queries)
            recency_weight_factor: 0.5, // Recency less important for architecture
            gate_boost: 0.0,
            coupling_threshold: Some(0.10), // Looser: design patterns spread across files
        },
        QueryIntent::Implementation => IntentAdjustments {
            doc_demotion_factor: 1.2, // Slightly demote docs (code patterns matter more)
            semantic_weight_factor: 1.0, // Balanced
            recency_weight_factor: 1.0, // Balanced
            gate_boost: 0.0,
            coupling_threshold: None, // Use config default
        },
        QueryIntent::Configuration => IntentAdjustments {
            doc_demotion_factor: 0.5,    // Config files and docs both relevant
            semantic_weight_factor: 0.7, // More keyword (config terms are literal)
            recency_weight_factor: 0.8,  // Slightly less recency bias
            gate_boost: 0.0,
            coupling_threshold: Some(0.20), // Tighter: config files are specific
        },
        QueryIntent::Navigation => IntentAdjustments {
            doc_demotion_factor: 1.0,    // Balanced
            semantic_weight_factor: 0.5, // More keyword (looking for exact names)
            recency_weight_factor: 0.3,  // Recency irrelevant for navigation
            gate_boost: 0.0,
            coupling_threshold: Some(0.25), // Tight: precision over recall
        },
        QueryIntent::Operational => IntentAdjustments {
            doc_demotion_factor: 2.0,       // Strongly demote docs
            semantic_weight_factor: 0.5,    // Keyword-heavy (command names are literal)
            recency_weight_factor: 0.5,     // Recency irrelevant
            gate_boost: 0.10, // Raise gate from 0.50 → 0.60 (blocks low-signal noise without gating legitimate queries)
            coupling_threshold: Some(0.30), // Very tight: operational queries rarely need coupling
        },
        QueryIntent::General => IntentAdjustments {
            gate_boost: 0.08,               // Raise gate (0.50 → 0.58) to filter marginal noise
            coupling_threshold: Some(0.20), // Tighter than default (0.15) — General queries produce loose coupling noise
            ..IntentAdjustments::default()
        },
    }
}
