use super::*;

/// The text of a single-content resource result.
fn resource_text(result: &ReadResourceResult) -> &str {
    match &result.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => text,
        other => panic!("expected text contents, got {other:?}"),
    }
}

#[test]
fn list_returns_two_resources() {
    let result = list_resources();
    assert_eq!(result.resources.len(), 2);
    assert_eq!(result.resources[0].uri, STYLE_GUIDE_URI);
    assert_eq!(result.resources[1].uri, AMBIGUOUS_DICT_URI);
}

#[test]
fn read_style_guide_returns_markdown() {
    let result = read_resource(STYLE_GUIDE_URI, &[], &OnceLock::new()).unwrap();
    assert_eq!(result.contents.len(), 1);
    assert!(resource_text(&result).contains("Punctuation"));
    assert!(resource_text(&result).contains("Character Variants"));
}

#[test]
fn read_ambiguous_dict_filters_by_english() {
    use crate::rules::ruleset::RuleType;

    let rules = vec![
        SpellingRule {
            context: Some("程序 in TW = procedure".into()),
            english: Some("program".into()),
            ..SpellingRule::new("程序", vec!["程式".into()], RuleType::Confusable)
        },
        // No english field, so this one stays out of the ambiguous dict.
        SpellingRule::new("軟件", vec!["軟體".into()], RuleType::CrossStrait),
    ];

    let result = read_resource(AMBIGUOUS_DICT_URI, &rules, &OnceLock::new()).unwrap();
    let text = resource_text(&result);
    let entries: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["from"], "程序");
}

#[test]
fn read_unknown_uri_returns_none() {
    assert!(read_resource("zh-tw://unknown", &[], &OnceLock::new()).is_none());
}

#[test]
fn ambiguous_dict_is_built_once_per_cache() {
    use crate::rules::ruleset::RuleType;

    let rules = vec![SpellingRule {
        context: None,
        english: Some("program".into()),
        ..SpellingRule::new("程序", vec!["程式".into()], RuleType::CrossStrait)
    }];
    let cache = OnceLock::new();
    let first = read_resource(AMBIGUOUS_DICT_URI, &rules, &cache).unwrap();

    // A second read with an empty ruleset must still serve the cached payload:
    // the ruleset cannot change under a live server, so the cache is what
    // decides the content after the first read.
    let second = read_resource(AMBIGUOUS_DICT_URI, &[], &cache).unwrap();
    assert_eq!(resource_text(&first), resource_text(&second));
    assert!(resource_text(&second).contains("程序"));
}
