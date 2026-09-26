//! Diagnostic assembly capture. Scores from different legs are not interchangeable.
use super::*;

#[derive(Debug, Serialize)]
pub struct AssemblyCapture {
    pub stage: &'static str,
    pub policy: serde_json::Value,
    pub candidates: Vec<CapturedCandidate>,
}

#[derive(Debug, Serialize)]
pub struct CapturedCandidate {
    pub leg: &'static str,
    pub id: String,
    pub path: String,
    pub repo: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub score: f32,
    pub score_kind: &'static str,
    pub pinned: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn snapshot(
    config: &ContextConfig,
    seeds: &[SeedResult],
    coupled: &[CoupledChunkInfo],
    bridged: &[CoupledChunkInfo],
    knowledge: &[KnowledgeChunkInfo],
    neighbors: &[NeighborChunkInfo],
    is_noise: &impl Fn(&str, &str, &Option<String>) -> bool,
) -> AssemblyCapture {
    let mut candidates = Vec::new();
    for c in seeds {
        candidates.push(CapturedCandidate {
            leg: if c.is_pinned { "pinned" } else { "direct" },
            id: c.chunk_id.clone(),
            path: c.file_path.clone(),
            repo: c.repo.clone(),
            start_line: c.start_line,
            end_line: c.end_line,
            text: c.content.clone(),
            score: c.score,
            score_kind: "adjusted_fused",
            pinned: c.is_pinned,
        });
    }
    // Apply exactly the assembly predicate, before any leg's budget can hide rows.
    // Preserve duplicates across legs: assembly resolves them in phase order.
    macro_rules! append {
        ($rows:expr, $leg:literal, $score:ident, $kind:literal) => {
            for c in $rows {
                if !is_noise(&c.file_path, &c.language, &None) {
                    candidates.push(CapturedCandidate {
                        leg: $leg,
                        id: c.chunk_id.clone(),
                        path: c.file_path.clone(),
                        repo: None,
                        start_line: c.start_line,
                        end_line: c.end_line,
                        text: c.content.clone(),
                        score: c.$score,
                        score_kind: $kind,
                        pinned: false,
                    });
                }
            }
        };
    }
    append!(coupled, "coupled", coupling_score, "coupling");
    append!(bridged, "bridged", coupling_score, "bridge");
    append!(knowledge, "knowledge", knowledge_score, "knowledge");
    append!(neighbors, "structural", neighbor_score, "neighbor");
    AssemblyCapture {
        stage: "post-adjustment-pre-packing",
        policy: serde_json::json!({
            "budget": config.budget_lines, "budget_unit": config.budget_unit,
            "max_chunk_cost": config.budget_lines / 2,
            "search_limit": config.search_limit,
            "content_mode": format!("{:?}", config.content_mode),
            "depth": config.depth, "max_coupled": config.max_coupled,
            "coupling_threshold": config.coupling_threshold,
            "recency_weight": config.recency_weight,
            "recency_half_life_days": config.recency_half_life_days,
            "ppr_weight": config.ppr_weight,
            "bridge_boost_factor": config.bridge_boost_factor,
            "repo_affinity": config.repo_affinity,
            "repo_affinity_boost": config.repo_affinity_boost,
            "semantic_weight": config.semantic_weight, "rrf_k": config.rrf_k,
            "doc_demotion": config.doc_demotion, "bridge_mode": config.bridge_mode,
            "knowledge_budget_pct": config.knowledge_budget_pct,
            "neighbor_budget_pct": config.neighbor_budget_pct,
            "bridged_budget_pct": 20,
            "feedback_applied": config.feedback_scores.is_some(),
            "feedback_boost_max": config.feedback_boost_max,
            "feedback_boost_weight": config.feedback_boost_weight,
            "phase_order": ["pinned", "direct", "coupled", "bridged", "knowledge", "structural"],
            "candidate_order": "input order within each leg; not a global ranking",
            "hook_gate": "not_run", "session_dedup": "not_run"
        }),
        candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(id: &str, path: &str, score: f32) -> SeedResult {
        SeedResult {
            chunk_id: id.into(),
            file_path: path.into(),
            language: "rust".into(),
            name: None,
            chunk_type: ChunkType::Function,
            start_line: 1,
            end_line: 4,
            content: "one\ntwo\nthree\nfour".into(),
            score,
            match_type: None,
            indexed_at: None,
            repo: None,
            tags: String::new(),
            is_pinned: false,
        }
    }

    #[test]
    fn capture_is_before_budget_after_feedback_and_filters() {
        let make = || {
            vec![
                seed("low", "src/low.rs", 0.5),
                seed("lowest", "src/lowest.rs", 0.1),
                seed("high", "src/high.rs", 0.9),
                seed("noise", "CLAUDE.md", 1.0),
            ]
        };
        let config = ContextConfig {
            capture_candidates: true,
            budget_lines: 4,
            feedback_scores: Some(HashMap::from([("src/low.rs".into(), 1.0)])),
            ..Default::default()
        };
        let captured =
            assemble_bundle("test", &config, make(), vec![], vec![], vec![], vec![]).unwrap();
        let rows = &captured.capture.as_ref().unwrap().candidates;
        assert_eq!(rows.len(), 3);
        assert_eq!(captured.summary.total_chunks, 2);
        assert!((rows.iter().find(|c| c.id == "low").unwrap().score - 0.6).abs() < 1e-6);
        let plain = assemble_bundle(
            "test",
            &ContextConfig {
                capture_candidates: false,
                ..config
            },
            make(),
            vec![],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        assert!(plain.capture.is_none());
        assert_eq!(
            serde_json::to_value(&captured).unwrap(),
            serde_json::to_value(&plain).unwrap()
        );
        assert!(serde_json::to_value(&captured)
            .unwrap()
            .get("capture")
            .is_none());
    }

    #[test]
    fn capture_keeps_all_legs_even_with_zero_budget() {
        let mut pin = seed("pin", "src/pin.rs", 1.0);
        pin.is_pinned = true;
        let coupled = |id: &str| CoupledChunkInfo {
            chunk_id: id.into(),
            file_path: format!("src/{id}.rs"),
            language: "rust".into(),
            name: None,
            chunk_type: ChunkType::Function,
            start_line: 1,
            end_line: 1,
            content: id.into(),
            coupling_score: 0.8,
            coupled_to: "src/pin.rs".into(),
        };
        let bundle = assemble_bundle(
            "test",
            &ContextConfig {
                capture_candidates: true,
                budget_lines: 0,
                ..Default::default()
            },
            vec![pin, seed("direct", "src/direct.rs", 0.7)],
            vec![coupled("coupled")],
            vec![coupled("bridged")],
            vec![KnowledgeChunkInfo {
                chunk_id: "knowledge".into(),
                file_path: "src/knowledge.rs".into(),
                language: "rust".into(),
                name: None,
                chunk_type: ChunkType::Function,
                start_line: 1,
                end_line: 1,
                content: "knowledge".into(),
                knowledge_score: 0.5,
                discovered_via: "entity".into(),
            }],
            vec![NeighborChunkInfo {
                chunk_id: "neighbor".into(),
                file_path: "src/neighbor.rs".into(),
                language: "rust".into(),
                name: None,
                chunk_type: ChunkType::Function,
                start_line: 1,
                end_line: 1,
                content: "neighbor".into(),
                neighbor_score: 0.4,
                discovered_via: "parent".into(),
            }],
        )
        .unwrap();
        let rows = &bundle.capture.as_ref().unwrap().candidates;
        assert_eq!(
            rows.iter().map(|c| c.leg).collect::<Vec<_>>(),
            [
                "pinned",
                "direct",
                "coupled",
                "bridged",
                "knowledge",
                "structural"
            ]
        );
        assert!(rows[0].pinned);
        assert_eq!(rows[2].score_kind, "coupling");
    }
}
