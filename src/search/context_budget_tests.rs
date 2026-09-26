#[test]
fn test_budget_enforcement() {
    let config = ContextConfig {
        budget_lines: 10,
        depth: 0,
        max_coupled: 3,
        coupling_threshold: 0.1,
        semantic_weight: 0.7,
        content_mode: ContentMode::Full,
        search_limit: 20,
        doc_demotion: 0.5,
        rrf_k: 60.0,
        recency_half_life_days: 0.0,
        recency_weight: 0.0,
        bridge_mode: BridgeMode::Inject,
        bridge_boost_factor: 0.3,
        extra_filter: None,
        tags_config: None,
        role: None,
        file_type_rules: vec![],
        repo_affinity: None,
        repo_affinity_boost: 2.0,
        ppr_weight: 0.0,
        max_bridged_files: 3,
        max_bridged_chunks_per_file: 2,
        repo_path_prefix: None,
        ..ContextConfig::default()
    };

    let seeds = vec![
        make_seed("c1", "a.rs", 1, 6, 0.9), // 6 lines
        make_seed("c2", "b.rs", 1, 8, 0.8), // 8 lines - won't fit (6+8 > 10)
        make_seed("c3", "c.rs", 1, 3, 0.7), // 3 lines - fits (6+3 = 9 <= 10)
    ];

    let bundle = assemble_bundle("test", &config, seeds, vec![], vec![], vec![], vec![]).unwrap();

    assert!(bundle.budget.used_lines <= bundle.budget.max_lines);
    assert_eq!(bundle.budget.max_lines, 10);
}

#[test]
fn test_estimate_tokens() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("a"), 1); // 1 char rounds up to 1 token
    assert_eq!(estimate_tokens("abcd"), 1); // 4 chars => 1
    assert_eq!(estimate_tokens("abcde"), 2); // 5 chars => ceil(5/4) = 2
    assert_eq!(estimate_tokens(&"x".repeat(40)), 10); // 40 chars => 10
}

#[test]
fn test_budget_unit_roundtrip() {
    for unit in [BudgetUnit::Line, BudgetUnit::Token] {
        let s = unit.to_string();
        let parsed: BudgetUnit = s.parse().unwrap();
        assert_eq!(unit, parsed);
    }
    assert_eq!(BudgetUnit::default(), BudgetUnit::Line);
}

#[test]
fn test_token_budget_enforcement() {
    // Token mode counts a chunk's cost from its *content* (~chars/4), not its
    // line span. A chunk with a huge line span but tiny content is cheap.
    let config = ContextConfig {
        budget_lines: 20, // interpreted as 20 tokens here
        budget_unit: BudgetUnit::Token,
        depth: 0,
        bridge_mode: BridgeMode::Inject,
        ..ContextConfig::default()
    };

    // a.rs: 199-line span but only 32 chars => 8 tokens (admitted despite span)
    let mut a = make_seed("c1", "a.rs", 1, 200, 0.9);
    a.content = "x".repeat(32);
    // b.rs: 40 chars => 10 tokens; 8 + 10 = 18 <= 20, fits
    let mut b = make_seed("c2", "b.rs", 1, 5, 0.8);
    b.content = "x".repeat(40);
    // c.rs: 16 chars => 4 tokens; 18 + 4 = 22 > 20, dropped
    let mut c = make_seed("c3", "c.rs", 1, 3, 0.7);
    c.content = "x".repeat(16);

    let bundle = assemble_bundle(
        "test",
        &config,
        vec![a, b, c],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();

    assert_eq!(bundle.budget.max_lines, 20);
    assert_eq!(bundle.budget.used_lines, 18); // tokens, not lines
    assert_eq!(bundle.summary.total_chunks, 2); // c dropped on budget
}
