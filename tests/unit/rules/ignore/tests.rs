use super::*;

fn issue(found: &str, severity: Severity) -> Issue {
    Issue::new(
        0,
        found.len(),
        found,
        vec!["x".to_string()],
        crate::rules::ruleset::IssueType::CrossStrait,
        severity,
    )
}

#[test]
fn ignored_term_drops_to_info() {
    let mut issues = vec![
        issue("軟件", Severity::Warning),
        issue("內存", Severity::Warning),
    ];
    let set: HashSet<&str> = ["軟件"].into_iter().collect();
    apply_ignore_set(&mut issues, &set);
    assert_eq!(issues[0].severity, Severity::Info);
    assert_eq!(issues[1].severity, Severity::Warning);
}

#[test]
fn empty_set_changes_nothing() {
    let mut issues = vec![issue("軟件", Severity::Error)];
    apply_ignore_set(&mut issues, &HashSet::new());
    assert_eq!(issues[0].severity, Severity::Error);
}
