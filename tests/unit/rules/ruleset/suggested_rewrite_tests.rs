use super::*;

#[test]
fn style_rewrite_hint_requires_one_determined_replacement() {
    assert_eq!(
        Issue::derive_suggested_rewrite(IssueType::AiStyle, &["廣泛使用".into()]),
        Some("廣泛使用".into())
    );
    assert_eq!(
        Issue::derive_suggested_rewrite(IssueType::AiStyle, &["藉由".into(), "經由".into()]),
        None,
        "a rewrite assistant must not be handed an arbitrary first alternative"
    );
    assert_eq!(
        Issue::derive_suggested_rewrite(IssueType::AiStyle, &["".into()]),
        None,
        "a deletion is an instruction, not replacement anchor text"
    );
}
