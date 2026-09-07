use std::ffi::OsStr;

#[test]
fn no_network_off_values() {
    // Unset, empty and "0" all leave calibration enabled.
    assert!(!super::disabled_by(None));
    assert!(!super::disabled_by(Some(OsStr::new(""))));
    assert!(!super::disabled_by(Some(OsStr::new("0"))));

    // Anything else refuses. "false" reads as off to a human but is not a
    // documented off-value, and guessing wrong here would open the socket.
    for on in ["1", "true", "yes", "0no", "00"] {
        assert!(super::disabled_by(Some(OsStr::new(on))), "{on:?}");
    }
}

use super::*;

#[test]
fn tokenize_words_basic() {
    let tokens = tokenize_words("Hello, world! This is a test.");
    assert!(tokens.contains(&"Hello".to_string()));
    assert!(tokens.contains(&"world".to_string()));
    assert!(tokens.contains(&"test".to_string()));
}

#[test]
fn tokenize_words_hyphenated() {
    let tokens = tokenize_words("multi-threaded server-side");
    assert!(tokens.contains(&"multi-threaded".to_string()));
    assert!(tokens.contains(&"multi".to_string()));
    assert!(tokens.contains(&"threaded".to_string()));
}

#[test]
fn tokenize_words_possessive() {
    let tokens = tokenize_words("it's don't");
    assert!(tokens.contains(&"it's".to_string()));
    assert!(tokens.contains(&"it".to_string()));
    assert!(tokens.contains(&"s".to_string()));
}

// #3: Context extraction now counts chars, not bytes, and respects CJK
// punctuation.
#[test]
fn extract_context_respects_cjk_sentence_punctuation() {
    let text = "第一句話。這個軟件很好用。第三句話在這裡。";
    // Offset points into the second sentence (after 。).
    let second_sentence_offset = "第一句話。".len();
    let ctx = extract_issue_context(text, second_sentence_offset);
    // Should NOT include the first sentence (bounded by 。).
    assert!(!ctx.contains("第一句話"), "context leaked past 。: {ctx}");
    assert!(
        ctx.contains("軟件"),
        "context should contain the issue: {ctx}"
    );
}

#[test]
fn extract_context_counts_chars_not_bytes() {
    // 50 CJK characters = 150 bytes. With ±40 char window, context should be
    // bounded around ~40 chars each direction, not 40 bytes.
    let text: String = (0..50).map(|_| '測').collect();
    let ctx = extract_issue_context(&text, text.len() / 2);
    let char_count = ctx.chars().count();
    // Should be ~80 chars (40 back + 40 forward), not ~26 (80 bytes / 3).
    assert!(
        char_count >= 50,
        "context too short: {char_count} chars (byte-counting bug?)"
    );
}

#[test]
fn extract_context_at_start() {
    let text = "軟件品質很好。";
    let ctx = extract_issue_context(text, 0);
    assert!(!ctx.is_empty());
}

#[test]
fn extract_context_at_end() {
    let text = "測試文字";
    let ctx = extract_issue_context(text, text.len());
    assert!(!ctx.is_empty());
}

// #1: Stopword filtering prevents false matches on common words.
#[test]
fn content_anchor_words_filters_stopwords() {
    let words = content_anchor_words("if and only if");
    // "if", "and" are stopwords; "only" is also a stopword.
    assert!(
        words.is_empty(),
        "all stopwords should be filtered: {words:?}"
    );
}

#[test]
fn content_anchor_words_keeps_content() {
    let words = content_anchor_words("memory (RAM)");
    assert!(words.contains(&"memory".to_string()));
    assert!(words.contains(&"ram".to_string()));
    assert!(!words.contains(&"a".to_string())); // single char filtered
}

#[test]
fn content_anchor_words_multivariant() {
    let words = content_anchor_words("simulation/emulation");
    assert!(words.contains(&"simulation".to_string()));
    assert!(words.contains(&"emulation".to_string()));
}

// #2: Sentinel segment parsing.
#[test]
fn parse_sentinel_segments_basic() {
    let translation = "###SEG0\nThis is memory.\n###SEG1\nA friend afar.";
    let segs = parse_sentinel_segments(translation, 2);
    assert_eq!(segs.len(), 2);
    assert!(
        segs[0].contains("memory"),
        "seg0 should contain 'memory': {:?}",
        segs[0]
    );
    assert!(
        segs[1].contains("friend"),
        "seg1 should contain 'friend': {:?}",
        segs[1]
    );
}

#[test]
fn parse_sentinel_segments_case_insensitive() {
    // Google Translate may capitalize the sentinel.
    let translation = "###Seg0\nTranslated text here.";
    let segs = parse_sentinel_segments(translation, 1);
    assert!(
        !segs[0].is_empty(),
        "should handle case-insensitive markers"
    );
}

#[test]
fn parse_sentinel_segments_missing_markers() {
    // If Google strips all markers, fall back to newline splitting.
    let translation = "Line one translation.\nLine two translation.";
    let segs = parse_sentinel_segments(translation, 2);
    assert_eq!(segs.len(), 2);
    assert!(!segs[0].is_empty());
    assert!(!segs[1].is_empty());
}

#[test]
fn calibrate_empty_text() {
    let mut issues = vec![];
    let r = calibrate_issues("", &mut issues);
    assert!(!r.api_ok);
    assert_eq!(r.matched, 0);
}

#[test]
fn calibrate_empty_issues() {
    let mut issues = vec![];
    let r = calibrate_issues("Some text here", &mut issues);
    assert!(!r.api_ok);
    assert_eq!(r.matched, 0);
}

#[test]
fn calibrate_no_english_field() {
    let mut issues = vec![Issue::new(
        0,
        6,
        "軟件",
        vec!["軟體".to_string()],
        crate::rules::ruleset::IssueType::CrossStrait,
        crate::rules::ruleset::Severity::Warning,
    )];
    // english is None by default
    assert!(issues[0].english.is_none());
    let r = calibrate_issues("這個軟件很好", &mut issues);
    assert_eq!(r.no_english, 1);
    assert!(issues[0].anchor_match.is_none());
}

// #1: Anchor with only stopwords should produce None, not false positive.
#[test]
fn calibrate_stopword_only_anchor_yields_none() {
    // "if and only if": all stopwords.  Should not produce a false match.
    let _issues = [Issue::new(
        0,
        6,
        "當且僅當",
        vec!["若且唯若".to_string()],
        crate::rules::ruleset::IssueType::CrossStrait,
        crate::rules::ruleset::Severity::Warning,
    )
    .with_english("if and only if")];

    // We can't call the real API in unit tests, but we can verify the
    // content_anchor_words logic that gates the match.
    let anchors = content_anchor_words("if and only if");
    assert!(
        anchors.is_empty(),
        "stopword-only anchor should produce no content words"
    );
}
