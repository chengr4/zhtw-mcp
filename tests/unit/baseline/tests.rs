use super::*;
use crate::rules::ruleset::{IssueType, Severity};

fn make_issue(found: &str) -> Issue {
    let mut issue = Issue::new(
        0,
        found.len(),
        found,
        vec!["fix".to_string()],
        IssueType::CrossStrait,
        Severity::Warning,
    );
    issue.line = 1;
    issue.col = 1;
    issue
}

#[test]
fn fingerprint_is_stable() {
    let issue = make_issue("軟件");
    let fp1 = fingerprint("test.md", &issue);
    let fp2 = fingerprint("test.md", &issue);
    assert_eq!(fp1, fp2);
}

#[test]
fn fingerprint_differs_by_file() {
    let issue = make_issue("軟件");
    let fp1 = fingerprint("a.md", &issue);
    let fp2 = fingerprint("b.md", &issue);
    assert_ne!(fp1, fp2);
}

#[test]
fn baseline_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("baseline.json");

    let mut bl = Baseline::default();
    let issue = make_issue("軟件");
    bl.insert("test.md", &issue);
    bl.save(&path).unwrap();

    let loaded = Baseline::load(&path).unwrap();
    assert!(loaded.contains("test.md", &issue));
    assert!(!loaded.contains("other.md", &issue));
}

#[test]
fn baseline_load_missing_file() {
    let bl = Baseline::load(Path::new("/nonexistent/baseline.json")).unwrap();
    assert_eq!(bl.len(), 0);
}
