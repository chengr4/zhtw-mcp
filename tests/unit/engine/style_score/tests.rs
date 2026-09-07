use super::*;
use std::sync::Arc;

fn issue(rule_type: IssueType, severity: Severity, line: usize) -> Issue {
    Issue {
        offset: 0,
        length: 0,
        line,
        col: 0,
        found: "x".into(),
        suggestions: Arc::from(Vec::<String>::new()),
        suggested_rewrite: None,
        rule_type,
        severity,
        context: None,
        english: None,
        context_clues: None,
        anchor_match: None,
        glossary_banned: false,
        phase_family: None,
        structural_family: None,
        tier2_outcome: Default::default(),
        llm_judged: false,
        spelling_rule_idx: None,
        table_cell: None,
        editorial_confidence: None,
    }
}

#[test]
fn empty_inputs_zero_regional_density() {
    let card = StyleScorecard::build(None, None, &[], 0);
    assert!(card.style_scores.ai.is_none());
    assert!(card.style_scores.translationese.is_none());
    assert_eq!(card.style_scores.regional_density, Some(0.0));
    assert!(card.top_issues_per_axis.ai.is_empty());
}

#[test]
fn three_axes_orthogonal_never_collapsed() {
    let issues = vec![
        issue(IssueType::AiStyle, Severity::Info, 1),
        issue(IssueType::Translationese, Severity::Warning, 2),
        issue(IssueType::CrossStrait, Severity::Warning, 3),
        issue(IssueType::CrossStrait, Severity::Info, 4),
    ];
    let card = StyleScorecard::build(None, None, &issues, 1000);
    // 2 cross_strait per 1000 chars = 2.0 raw → capped 1.0.
    assert_eq!(card.style_scores.regional_density, Some(1.0));
    assert_eq!(card.top_issues_per_axis.ai.len(), 1);
    assert_eq!(card.top_issues_per_axis.translationese.len(), 1);
    assert_eq!(card.top_issues_per_axis.regional_density.len(), 2);
}

#[test]
fn top_issues_capped_at_5_per_axis() {
    let issues: Vec<Issue> = (0..10)
        .map(|i| issue(IssueType::AiStyle, Severity::Info, i))
        .collect();
    let card = StyleScorecard::build(None, None, &issues, 1000);
    assert_eq!(card.top_issues_per_axis.ai.len(), 5);
}

#[test]
fn document_level_scores_survive_without_issue_entries() {
    let ai = AiSignatureReport {
        score: 0.7,
        markers: Vec::new(),
        top_signals: Vec::new(),
        sentence_variability: None,
        zero_width_count: 0,
        punctuation_profile: None,
    };
    let card = StyleScorecard::build(Some(&ai), None, &[], 1000);
    assert_eq!(card.style_scores.ai, Some(0.7));
}

#[test]
fn top_issues_sorted_by_severity_then_line() {
    let issues = vec![
        issue(IssueType::AiStyle, Severity::Info, 100),
        issue(IssueType::AiStyle, Severity::Error, 200),
        issue(IssueType::AiStyle, Severity::Warning, 300),
    ];
    let card = StyleScorecard::build(None, None, &issues, 1000);
    let top = &card.top_issues_per_axis.ai;
    assert_eq!(top[0].severity, Severity::Error);
    assert_eq!(top[1].severity, Severity::Warning);
    assert_eq!(top[2].severity, Severity::Info);
}
