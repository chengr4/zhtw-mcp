use super::*;
use crate::rules::ruleset::{IssueType, Severity};

fn make_issue(from: &str, suggestions: Vec<&str>, english: Option<&str>) -> Issue {
    let mut issue = Issue::new(
        0,
        from.len(),
        from,
        suggestions.into_iter().map(String::from).collect(),
        IssueType::CrossStrait,
        Severity::Warning,
    );
    if let Some(e) = english {
        issue.english = Some(Arc::from(e));
    }
    issue
}

#[test]
fn hard_anchor_terminates() {
    let mut issue = make_issue("軟件", vec!["軟體"], Some("software"));
    issue.anchor_match = Some(true);
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "安裝軟件更新", &cfg);
    assert_eq!(result.resolution, Resolution::HardAnchor);
    assert_eq!(result.score, 1.0);
    assert_eq!(result.resolved.as_deref(), Some("軟體"));
}

#[test]
fn soft_anchor_does_not_terminate() {
    let mut issue = make_issue("令牌", vec!["權杖", "代幣", "詞元"], Some("token"));
    issue.anchor_match = Some(true); // multiple suggestions → Soft
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "使用令牌驗證", &cfg);
    assert_ne!(result.resolution, Resolution::HardAnchor);
}

#[test]
fn collocation_resolves_deterministically() {
    let issue = make_issue("進程", vec!["行程"], Some("process"));
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "系統排程管理進程", &cfg);
    assert_eq!(result.resolution, Resolution::Collocation);
    assert_eq!(result.resolved.as_deref(), Some("行程"));
}

#[test]
fn collocation_token_domain_routing() {
    // OAuth context → 權杖
    let issue = make_issue("令牌", vec!["權杖", "代幣", "詞元"], Some("token"));
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "OAuth令牌驗證流程", &cfg);
    assert_eq!(result.resolved.as_deref(), Some("權杖"));

    // NLP context → 詞元
    let result2 = score_issue(&issue, "NLP分詞令牌化處理", &cfg);
    assert_eq!(result2.resolved.as_deref(), Some("詞元"));

    // blockchain context → 代幣
    let result3 = score_issue(&issue, "區塊鏈令牌發行", &cfg);
    assert_eq!(result3.resolved.as_deref(), Some("代幣"));
}

#[test]
fn context_clues_boost_score() {
    let mut issue = make_issue("進程", vec!["行程"], Some("process"));
    issue.context_clues = Some(Arc::from(vec![
        "排程".to_string(),
        "PID".to_string(),
        "背景".to_string(),
    ]));
    let cfg = DisambigConfig::default();
    // Two clues match ("排程", "PID"):
    let result = score_issue(&issue, "查看PID排程器的進程", &cfg);
    assert!(result.score >= cfg.decided_threshold);
}

#[test]
fn no_clues_no_anchor_suppressed() {
    let issue = make_issue("令牌", vec!["權杖", "代幣"], Some("token"));
    let cfg = DisambigConfig::default();

    // No collocations, no clues, no anchor, no profile prior for Base → score =
    // 0.0 < ambiguous_threshold → suppressed.
    let result = score_issue(&issue, "使用令牌", &cfg);
    assert_eq!(result.resolution, Resolution::Suppressed);
}

#[test]
fn weak_signal_enters_gray_zone() {
    // Strict profile has a prior for 令牌 (weight=0.3) which puts score right
    // at the ambiguous threshold boundary → gray zone.
    let issue = make_issue("令牌", vec!["權杖", "代幣"], Some("token"));
    let cfg = DisambigConfig {
        profile: Profile::Strict,
        ..Default::default()
    };
    let result = score_issue(&issue, "使用令牌", &cfg);
    assert_eq!(result.resolution, Resolution::GrayZone);
}

#[test]
fn suppressed_below_threshold() {
    let mut issue = make_issue("令牌", vec!["權杖"], Some("token"));
    issue.anchor_match = Some(false); // calibration rejected
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "令牌數量", &cfg);

    // anchor_match=false gives -0.2, no clues = 0, no prior for base. Score =
    // max(0, -0.2) = 0.0 < 0.3 → suppressed.
    assert_eq!(result.resolution, Resolution::Suppressed);
}

#[test]
fn profile_prior_contributes() {
    let issue = make_issue("內存", vec!["記憶體"], Some("memory"));
    let base_cfg = DisambigConfig::default();
    let base_result = score_issue(&issue, "內存使用量", &base_cfg);

    let strict_cfg = DisambigConfig {
        profile: Profile::Strict,
        ..Default::default()
    };
    let strict_result = score_issue(&issue, "內存使用量", &strict_cfg);

    // Strict profile has higher prior weight.
    assert!(strict_result.score >= base_result.score);
}

#[test]
fn progress_context_suppresses_jincheng_false_positive() {
    let issue = make_issue("進程", vec!["行程"], Some("process"));
    let cfg = DisambigConfig::default();
    let result = score_issue(&issue, "學習的進程需要耐心和毅力", &cfg);
    assert_eq!(result.resolution, Resolution::Suppressed);
    assert_eq!(result.score, 0.0);
}

#[test]
fn batch_disambiguate_stats() {
    let mut issues = vec![
        {
            let mut i = make_issue("軟件", vec!["軟體"], Some("software"));
            i.anchor_match = Some(true);
            i
        },
        make_issue("令牌", vec!["權杖", "代幣"], Some("token")),
        Issue::new(
            0,
            3,
            "：",
            vec!["：".to_string()],
            IssueType::Punctuation,
            Severity::Warning,
        ),
    ];

    let cfg = DisambigConfig::default();
    let stats = disambiguate_batch(&mut issues, "安裝軟件，使用令牌", &cfg);

    assert_eq!(stats.hard_anchor, 1); // 軟件
    assert_eq!(stats.not_eligible, 1); // punctuation
                                       // 令牌: no collocations, no clues, no
                                       // Base prior → suppressed
    assert_eq!(stats.suppressed, 1);
}

#[test]
fn promotion_refreshes_translationese_rewrite() {
    let mut issue = Issue::new(
        0,
        "冗長".len(),
        "冗長",
        vec!["短句".to_string(), "精簡".to_string()],
        IssueType::Translationese,
        Severity::Warning,
    );

    promote_suggestion(&mut issue, "精簡");

    assert_eq!(issue.suggestions.as_ref(), ["精簡", "短句"]);
    assert_eq!(issue.suggested_rewrite.as_deref(), Some("精簡"));

    issue.suggested_rewrite = Some("短句".to_string());
    promote_suggestion(&mut issue, "精簡");
    assert_eq!(issue.suggested_rewrite.as_deref(), Some("精簡"));
}

#[test]
fn is_tier2_eligible_filters_correctly() {
    // Plain single-suggestion lexical issue → not eligible
    let i1 = make_issue("軟件", vec!["軟體"], Some("software"));
    assert!(!is_tier2_eligible(&i1));

    // Anchor-confirmed issue → eligible
    let mut i1_anchor = make_issue("軟件", vec!["軟體"], Some("software"));
    i1_anchor.anchor_match = Some(true);
    assert!(is_tier2_eligible(&i1_anchor));

    // Punctuation → not eligible
    let i2 = Issue::new(
        0,
        1,
        "：",
        vec![],
        IssueType::Punctuation,
        Severity::Warning,
    );
    assert!(!is_tier2_eligible(&i2));

    // CrossStrait without english or clues → not eligible
    let i3 = Issue::new(
        0,
        6,
        "東西",
        vec!["物品".to_string()],
        IssueType::CrossStrait,
        Severity::Warning,
    );
    assert!(!is_tier2_eligible(&i3));

    // Negative-domain-screened ambiguous term → eligible
    let i4 = make_issue("進程", vec!["行程"], Some("process"));
    assert!(is_tier2_eligible(&i4));
}

#[test]
fn extract_context_respects_paragraph_boundary() {
    let text = "第一段落。\n\n這裡有進程排程問題。\n\n第三段落。";
    let offset = text.find("進程").unwrap();
    let ctx = extract_context_for_disambig(text, offset, "進程".len());
    assert!(ctx.contains("進程"));
    assert!(ctx.contains("排程"));
    assert!(
        !ctx.contains("第一段落"),
        "should not leak past paragraph break"
    );
    assert!(
        !ctx.contains("第三段落"),
        "should not leak past paragraph break"
    );
}

// -- Semantic chunking tests --

#[test]
fn semantic_chunk_paragraph_boundary() {
    let text = "第一段。\n\n這裡的內存需要分配。\n\n第三段。";
    let offset = text.find("內存").unwrap();
    let chunk = extract_semantic_chunk(text, offset, "內存".len());
    assert!(chunk.contains("內存"));
    assert!(!chunk.contains("第一段"), "leaked past paragraph break");
    assert!(!chunk.contains("第三段"), "leaked past paragraph break");
}

#[test]
fn semantic_chunk_heading_boundary() {
    let text = "## Section A\nSome content here.\n## Section B\n這裡有軟件問題。";
    let offset = text.find("軟件").unwrap();
    let chunk = extract_semantic_chunk(text, offset, "軟件".len());
    assert!(chunk.contains("軟件"));
    assert!(!chunk.contains("Section A"), "leaked past heading");
}

#[test]
fn semantic_chunk_list_item_boundary() {
    let text = "- 第一項：正常\n- 第二項：軟件需更新\n- 第三項：完成";
    let offset = text.find("軟件").unwrap();
    let chunk = extract_semantic_chunk(text, offset, "軟件".len());
    assert!(chunk.contains("軟件"));
    // List items are structural boundaries.
}

#[test]
fn semantic_chunk_respects_max_size() {
    // Build a very long paragraph (>500 chars).
    let prefix: String = (0..300).map(|_| '測').collect();
    let suffix: String = (0..300).map(|_| '試').collect();
    let text = format!("{prefix}軟件{suffix}");
    let offset = text.find("軟件").unwrap();
    let chunk = extract_semantic_chunk(&text, offset, "軟件".len());
    assert!(chunk.contains("軟件"), "chunk must contain the issue");
    let char_count = chunk.chars().count();
    assert!(
        char_count <= MAX_CHUNK_CHARS,
        "chunk too large: {char_count} chars (max {MAX_CHUNK_CHARS})"
    );
}

#[test]
fn semantic_chunk_utf8_safe() {
    // Ensure no panic on multibyte boundaries.
    let text = "你好世界軟件測試文字";
    let offset = text.find("軟件").unwrap();
    let chunk = extract_semantic_chunk(text, offset, "軟件".len());
    assert!(chunk.contains("軟件"));
}

#[test]
fn semantic_chunk_empty_text() {
    let chunk = extract_semantic_chunk("", 0, 0);
    assert!(chunk.is_empty());
}

#[test]
fn semantic_chunk_ordered_list() {
    let text = "1. 第一步\n2. 這裡的接口有問題\n3. 完成";
    let offset = text.find("接口").unwrap();
    let chunk = extract_semantic_chunk(text, offset, "接口".len());
    assert!(chunk.contains("接口"));
}
