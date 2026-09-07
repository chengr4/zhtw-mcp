use super::*;

fn sugs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn only_a_lone_empty_suggestion_means_delete() {
    assert!(is_delete_suggestion(&sugs(&[""])));
    // An empty list means "no suggestion", not "delete".
    assert!(!is_delete_suggestion(&[]));
    assert!(!is_delete_suggestion(&sugs(&["軟體"])));
    // Two entries, one empty, is an alternatives list.
    assert!(!is_delete_suggestion(&sugs(&["", "軟體"])));
}

#[test]
fn compact_suggestion_renders_the_delete_sentinel() {
    let issue = Issue::new(
        0,
        3,
        "\u{200B}",
        sugs(&[""]),
        IssueType::AiStyle,
        Severity::Info,
    );
    assert_eq!(issue.compact_suggestion(), DELETE_SUGGESTION);
}
