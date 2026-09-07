use super::*;

#[test]
fn ignore_next_line_markdown() {
    let text = "第一行\n<!-- zhtw:ignore-next-line -->\n這行被忽略\n第四行\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    // The suppressed line is "這行被忽略\n"
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("這行被忽略"));
}

#[test]
fn ignore_next_line_at_end() {
    // Suppress marker on last line with no following line.
    let text = "第一行\n<!-- zhtw:ignore-next-line -->";
    let ranges = build_suppression_ranges(text, true);
    assert!(ranges.is_empty()); // No next line to suppress.
}

#[test]
fn ignore_block_markdown() {
    let text =
        "開始\n<!-- zhtw:ignore-block -->\n被忽略1\n被忽略2\n<!-- zhtw:end-ignore -->\n結束\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略1"));
    assert!(suppressed.contains("被忽略2"));
    assert!(!suppressed.contains("結束"));
}

#[test]
fn ignore_block_unclosed() {
    let text = "開始\n<!-- zhtw:ignore-block -->\n被忽略到結尾";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].end, text.len());
}

#[test]
fn inline_code_suppression() {
    let text = "正常行\n被忽略 // zhtw:ignore\n正常行\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略"));
}

#[test]
fn no_suppression_markers() {
    let text = "這是正常文字\n沒有任何忽略標記\n";
    let ranges = build_suppression_ranges(text, true);
    assert!(ranges.is_empty());
}

#[test]
fn multiple_suppressions() {
    let text = "行1\n<!-- zhtw:ignore-next-line -->\n忽略2\n行3 // zhtw:ignore\n行4\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 2);
}

#[test]
fn empty_text() {
    let ranges = build_suppression_ranges("", true);
    assert!(ranges.is_empty());
}

// disable alias tests

#[test]
fn disable_next_line_alias() {
    let text = "第一行\n<!-- zhtw:disable-next-line -->\n這行被忽略\n第四行\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("這行被忽略"));
}

#[test]
fn disable_block_alias() {
    let text =
        "開始\n<!-- zhtw:disable-block -->\n被忽略1\n被忽略2\n<!-- zhtw:end-disable -->\n結束\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略1"));
    assert!(suppressed.contains("被忽略2"));
    assert!(!suppressed.contains("結束"));
}

#[test]
fn end_disable_closes_ignore_block() {
    // end-disable should close ignore-block and vice-versa.
    let text = "開始\n<!-- zhtw:ignore-block -->\n被忽略1\n<!-- zhtw:end-disable -->\n結束\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略1"));
    assert!(!suppressed.contains("結束"));
}

#[test]
fn inline_code_disable_alias() {
    // // zhtw:disable should work identically to // zhtw:ignore for inline
    // code.
    let text = "正常行\n被忽略 // zhtw:disable\n正常行\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略"));
}

// hash-comment markers (YAML, TOML, Python, shell)

#[test]
fn hash_comment_line_suppression() {
    let text = "name: 正常\ntitle: 被忽略  # zhtw:ignore\nname: 正常\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略"));
    // Suppression starts at the line start, not at the comment.
    assert!(suppressed.starts_with("title:"));
}

#[test]
fn hash_comment_next_line_suppression() {
    let text = "# zhtw:disable-next-line\ntitle: 被忽略\ntitle: 正常\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略"));
    assert!(!suppressed.contains("正常"));
}

#[test]
fn hash_comment_block_fences_a_list() {
    // The fixture-array case: fence off a region of a YAML file.
    let text = "\
terms:
# zhtw:disable-block
  - 被忽略1
  - 被忽略2
# zhtw:enable
  - 結束
";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略1"));
    assert!(suppressed.contains("被忽略2"));
    assert!(!suppressed.contains("結束"));
}

#[test]
fn marker_needs_a_comment_opener() {
    // Prose that merely mentions a pragma is not a pragma. Both the bare form
    // and one quoted inside running text stay inert.
    let text = "說明 zhtw:ignore 的用法\n這行提到 zhtw:disable-block 但不是標記\n";
    assert!(build_suppression_ranges(text, true).is_empty());
}

#[test]
fn hash_marker_needs_its_own_token() {
    // "value# zhtw:ignore" is data in YAML and shell, not a comment.
    let text = "url: http://example.com/a#zhtw:ignore\nkey: 值# zhtw:ignore\n";
    assert!(build_suppression_ranges(text, true).is_empty());
}

#[test]
fn slash_marker_needs_its_own_token() {
    // A URL scheme separator and a doubled path slash are not comment openers,
    // however much they look like one to ends_with.
    for text in [
        "參考 https://zhtw:ignore 的說明\n",
        "路徑 docs//zhtw:disable-block 之下\n",
        "壞網址 https:///zhtw:ignore 也不算\n",
    ] {
        assert!(
            build_suppression_ranges(text, true).is_empty(),
            "must not be a pragma: {text}"
        );
    }

    // A comment opener after code keeps working, with or without a space in
    // front of it.
    for text in ["x();// zhtw:ignore\n", "x(); // zhtw:ignore\n"] {
        assert_eq!(
            build_suppression_ranges(text, true).len(),
            1,
            "must stay a pragma: {text}"
        );
    }
}

#[test]
fn cjk_suffix_leaves_the_keyword_unknown() {
    // 範例 ends no word, so this is not "ignore-block" plus a comment.
    let text = "# zhtw:ignore-block範例\n被檢查\n";
    assert!(build_suppression_ranges(text, true).is_empty());
}

#[test]
fn markdown_headings_are_not_pragmas() {
    // With hash comments off, a heading documenting a pragma is prose.
    let text = "# zhtw:ignore-block\n被檢查\n# zhtw:end-ignore\n";
    assert!(build_suppression_ranges(text, false).is_empty());
    // The same text in a hash-comment format does suppress.
    assert_eq!(build_suppression_ranges(text, true).len(), 1);
}

#[test]
fn unknown_keyword_suppresses_nothing() {
    // Neither suffix may degrade into a bare "ignore".
    for text in [
        "被忽略 # zhtw:ignore-everything\n",
        "被忽略 # zhtw:ignore_rule\n",
    ] {
        assert!(
            build_suppression_ranges(text, true).is_empty(),
            "unknown keyword must not suppress: {text}"
        );
    }
}

#[test]
fn crlf_line_endings() {
    let text = "行1\r\n被忽略 # zhtw:ignore\r\n行3\r\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略"));
    assert!(!suppressed.contains("行3"));
}

#[test]
fn keyword_order_is_safe() {
    // First match wins, so no keyword may be a prefix of a later one.
    for (i, (earlier, _)) in KEYWORDS.iter().enumerate() {
        for (later, _) in &KEYWORDS[i + 1..] {
            assert!(
                !later.starts_with(earlier),
                "{earlier} shadows {later}; move {later} earlier in KEYWORDS"
            );
        }
    }
}

#[test]
fn end_ignore_closes_disable_block() {
    // Reciprocal of end_disable_closes_ignore_block: end-ignore should close a
    // block opened by disable-block.
    let text = "開始\n<!-- zhtw:disable-block -->\n被忽略1\n<!-- zhtw:end-ignore -->\n結束\n";
    let ranges = build_suppression_ranges(text, true);
    assert_eq!(ranges.len(), 1);
    let suppressed = &text[ranges[0].start..ranges[0].end];
    assert!(suppressed.contains("被忽略1"));
    assert!(!suppressed.contains("結束"));
}
