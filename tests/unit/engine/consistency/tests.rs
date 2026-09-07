use super::*;
use std::sync::Arc;

fn cross_strait(offset: usize, found: &str, suggestion: &str, english: &str) -> Issue {
    let mut issue = Issue::new(
        offset,
        found.len(),
        found,
        vec![suggestion.into()],
        IssueType::CrossStrait,
        Severity::Warning,
    );
    issue.english = Some(Arc::from(english));
    issue
}

#[test]
fn empty_when_no_mixed_usage() {
    let text = "我們只用線程實作。";
    let issues = vec![cross_strait(3, "線程", "執行緒", "thread")];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert!(report.is_empty(), "no canonical 執行緒 in text → no group");
}

#[test]
fn canonical_inside_calque_does_not_count_as_mixed_usage() {
    for text in ["厄瓜多爾", "厄瓜多爾和厄瓜多爾"] {
        let issues: Vec<Issue> = text
            .match_indices("厄瓜多爾")
            .map(|(offset, found)| cross_strait(offset, found, "厄瓜多", "Ecuador"))
            .collect();
        let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
        assert!(
            report.is_empty(),
            "only one regional form is present: {text}"
        );
    }
}

#[test]
fn repeated_calques_with_unsorted_overlapping_spans_stay_unmixed() {
    let text = "厄瓜多爾 ".repeat(1000);
    let mut issues: Vec<Issue> = text
        .match_indices("厄瓜多爾")
        .map(|(offset, found)| cross_strait(offset, found, "厄瓜多", "Ecuador"))
        .collect();
    issues.push(cross_strait(0, "厄瓜多爾 厄瓜多爾", "厄瓜多", "Ecuador"));
    issues.reverse();
    let report = compute_consistency_report(&text, &issues, &ProjectGlossary::default());
    assert!(report.is_empty());
}

#[test]
fn canonical_outside_calque_counts_before_or_after_it() {
    for text in ["厄瓜多和厄瓜多爾", "厄瓜多爾和厄瓜多"] {
        let offset = text.find("厄瓜多爾").unwrap();
        let issues = vec![cross_strait(offset, "厄瓜多爾", "厄瓜多", "Ecuador")];
        let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
        assert_eq!(report.groups.len(), 1, "both regional forms occur: {text}");
        assert_eq!(report.groups[0].preferred, "厄瓜多");
    }
}

#[test]
fn canonical_overlapping_calque_edge_does_not_count() {
    let text = "甲乙丙";
    let issues = vec![cross_strait(0, "甲乙", "乙丙", "example")];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert!(
        report.is_empty(),
        "a partial overlap is not independent usage"
    );
}

#[test]
fn independent_canonical_can_overlap_an_earlier_rejected_match() {
    let text = "哈哈哈";
    let issues = vec![cross_strait(0, "哈", "哈哈", "example")];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert_eq!(
        report.groups.len(),
        1,
        "the final two characters are independent"
    );
}

#[test]
fn glossary_substring_does_not_displace_independent_default() {
    let text = "大實話和真心話";
    let mut issue = cross_strait(0, "大實話", "真心話", "blunt truth");
    issue.suggestions = vec!["真心話".into(), "實話".into()].into();
    let glossary = ProjectGlossary {
        preferred: vec!["實話".into()],
        ..ProjectGlossary::default()
    };
    let report = compute_consistency_report(text, &[issue], &glossary);
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].preferred, "真心話");
}

#[test]
fn fires_when_both_forms_present() {
    let text = "我們的線程很慢。執行緒設計需要重構。";
    let issues = vec![cross_strait(9, "線程", "執行緒", "thread")];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert_eq!(report.groups.len(), 1);
    let group = &report.groups[0];
    assert_eq!(group.term_group, "thread");
    assert_eq!(group.preferred, "執行緒");
    assert_eq!(group.occurrences.len(), 1);
    assert_eq!(group.occurrences[0].found, "線程");
}

#[test]
fn groups_multiple_calques_for_same_english() {
    // Two occurrences of the same calque 線程, both anchored to
    // english="thread", so they group into one entry.
    let text = "我們的線程很慢，線程數量太多。執行緒重構。";
    let issues = vec![
        cross_strait(9, "線程", "執行緒", "thread"),
        cross_strait(24, "線程", "執行緒", "thread"),
    ];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].occurrences.len(), 2);
}

#[test]
fn ignores_info_severity_issues_tm_suppressed() {
    let text = "線程 ... 執行緒";
    let mut issue = cross_strait(0, "線程", "執行緒", "thread");
    issue.severity = Severity::Info;
    let report = compute_consistency_report(text, &[issue], &ProjectGlossary::default());
    assert!(report.is_empty(), "Info severity (TM-suppressed) skipped");
}

#[test]
fn ignores_issues_without_english_anchor() {
    let text = "X ... Y";
    let mut issue = Issue::new(
        0,
        1,
        "X",
        vec!["Y".into()],
        IssueType::CrossStrait,
        Severity::Warning,
    );
    issue.english = None;
    let report = compute_consistency_report(text, &[issue], &ProjectGlossary::default());
    assert!(report.is_empty());
}

#[test]
fn separates_groups_by_english_anchor() {
    let text = "線程 執行緒 用戶 使用者";
    let issues = vec![
        cross_strait(0, "線程", "執行緒", "thread"),
        cross_strait(7, "用戶", "使用者", "user"),
    ];
    let report = compute_consistency_report(text, &issues, &ProjectGlossary::default());
    assert_eq!(report.groups.len(), 2);
    let groups: Vec<&str> = report
        .groups
        .iter()
        .map(|g| g.term_group.as_str())
        .collect();
    assert!(groups.contains(&"thread"));
    assert!(groups.contains(&"user"));
}

#[test]
fn prefers_glossary_preferred_form_over_default_suggestion() {
    // The rule lists two acceptable TW forms; the glossary picks one as the
    // project-canonical. When both regional variants appear in the document AND
    // the glossary's choice is among the rule's suggestions (matches_group),
    // the consistency report surfaces the glossary's choice instead of the
    // rule's first suggestion.
    let text = "我們的線程很慢。緒程設計需要重構。";
    let mut issue = Issue::new(
        9,
        6,
        "線程",
        vec!["執行緒".into(), "緒程".into()],
        IssueType::CrossStrait,
        Severity::Warning,
    );
    issue.english = Some(Arc::from("thread"));
    let glossary = ProjectGlossary {
        preferred: vec!["緒程".into()],
        ..ProjectGlossary::default()
    };
    let report = compute_consistency_report(text, &[issue], &glossary);
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.groups[0].preferred, "緒程");
}

#[test]
fn glossary_preferred_outside_suggestions_falls_back_to_rule_suggestion() {
    let text = "我們的線程很慢。緒程設計需要重構。執行緒也要重構。";
    let issues = vec![cross_strait(9, "線程", "執行緒", "thread")];
    let glossary = ProjectGlossary {
        preferred: vec!["緒程".into()],
        ..ProjectGlossary::default()
    };
    let report = compute_consistency_report(text, &issues, &glossary);
    assert_eq!(report.groups.len(), 1);
    assert_eq!(
        report.groups[0].preferred, "執行緒",
        "preferred terms outside rule suggestions must not hijack the group"
    );
}

#[test]
fn edit_distance_neighbor_does_not_hijack_group() {
    // Regression guard for short zh terms: sharing one edge character with the
    // calque is not enough to join the same concept group.
    let text = "我們的線程很慢。執行緒設計需要重構。線性代數也出現。";
    let issues = vec![cross_strait(9, "線程", "執行緒", "thread")];
    let glossary = ProjectGlossary {
        preferred: vec!["線性".into()],
        ..ProjectGlossary::default()
    };
    let report = compute_consistency_report(text, &issues, &glossary);
    assert_eq!(report.groups.len(), 1);
    assert_eq!(
        report.groups[0].preferred, "執行緒",
        "must fall back to rule suggestion, not pick unrelated 線性"
    );
}

#[test]
fn glossary_preference_does_not_leak_across_groups() {
    let text = "線程與使用者都出現在文件裡。執行緒也出現。";
    let issues = vec![
        cross_strait(0, "線程", "執行緒", "thread"),
        cross_strait(3, "用戶", "使用者", "user"),
    ];
    let glossary = ProjectGlossary {
        preferred: vec!["使用者".into()],
        ..ProjectGlossary::default()
    };
    let report = compute_consistency_report(text, &issues, &glossary);
    let thread_group = report
        .groups
        .iter()
        .find(|group| group.term_group == "thread")
        .expect("thread group should exist");
    assert_eq!(thread_group.preferred, "執行緒");
}
