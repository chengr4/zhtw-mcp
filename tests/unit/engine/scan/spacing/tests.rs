use super::super::Scanner;
use crate::rules::ruleset::IssueType;

fn spacing_issues(text: &str) -> Vec<(String, String)> {
    let scanner = Scanner::new(vec![], vec![]);
    let issues = scanner.scan(text).issues;
    issues
        .into_iter()
        .filter(|i| {
            i.rule_type == IssueType::Punctuation
                && i.context.as_deref().is_some_and(|c| {
                    c.contains("空格")
                        || c.contains("標點")
                        || c.contains("數字應使用")
                        || c.contains("不重複")
                })
        })
        .map(|i| {
            (
                i.context.as_deref().unwrap_or("").to_string(),
                i.suggestions.first().cloned().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn cjk_latin_missing_space() {
    let issues = spacing_issues("在LeanCloud上");
    assert!(
        issues.iter().any(|(c, _)| c.contains("中英文")),
        "should flag missing space between CJK and Latin: {issues:?}"
    );
}

#[test]
fn cjk_latin_has_space() {
    let issues = spacing_issues("在 LeanCloud 上");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("中英文")),
        "should not flag when space exists: {issues:?}"
    );
}

#[test]
fn cjk_digit_missing_space() {
    let issues = spacing_issues("花了5000元");
    assert!(
        issues.iter().any(|(c, _)| c.contains("數字")),
        "should flag missing space between CJK and digit: {issues:?}"
    );
}

#[test]
fn cjk_digit_has_space() {
    let issues = spacing_issues("花了 5000 元");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("數字")),
        "should not flag when space exists: {issues:?}"
    );
}

#[test]
fn space_before_fullwidth_punct() {
    let issues = spacing_issues("iPhone ，好開心");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag space before fullwidth comma: {issues:?}"
    );
}

#[test]
fn no_space_around_fullwidth_punct() {
    let issues = spacing_issues("iPhone，好開心");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should not flag correct punctuation: {issues:?}"
    );
}

#[test]
fn repeated_fullwidth_punct() {
    let issues = spacing_issues("太厲害了！！");
    assert!(
        issues.iter().any(|(c, _)| c.contains("不重複")),
        "should flag repeated exclamation: {issues:?}"
    );
}

#[test]
fn single_fullwidth_punct_ok() {
    let issues = spacing_issues("太厲害了！");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "should not flag single exclamation: {issues:?}"
    );
}

#[test]
fn fullwidth_digit_flagged() {
    let issues = spacing_issues("只賣１０００元");
    assert!(
        issues.iter().any(|(c, _)| c.contains("數字應使用")),
        "should flag fullwidth digits: {issues:?}"
    );
}

#[test]
fn mixed_exclamation_question_not_repeated() {
    // ！？ are different punctuation marks: not "repeated".
    let issues = spacing_issues("真的嗎！？");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "should not flag mixed ！？ as repeated: {issues:?}"
    );
}

#[test]
fn multi_space_before_fullwidth_punct() {
    // Multiple spaces before fullwidth comma should still be flagged.
    let issues = spacing_issues("iPhone  ，好開心");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag multi-space before fullwidth comma: {issues:?}"
    );
}

#[test]
fn multi_space_after_fullwidth_punct() {
    // Multiple spaces after fullwidth comma should still be flagged.
    let issues = spacing_issues("好，  開心");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag multi-space after fullwidth comma: {issues:?}"
    );
}

#[test]
fn double_ellipsis_ok() {
    // …… (exactly 2) is standard zh-TW form: should NOT be flagged.
    let issues = spacing_issues("他說……算了");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "should not flag standard double ellipsis: {issues:?}"
    );
}

#[test]
fn triple_ellipsis_flagged() {
    // ……… (3+) is non-standard: should be flagged.
    let issues = spacing_issues("他說………算了");
    assert!(
        issues.iter().any(|(c, _)| c.contains("不重複")),
        "should flag triple ellipsis: {issues:?}"
    );
}

#[test]
fn double_em_dash_ok() {
    // —— (exactly 2) is standard zh-TW form: should NOT be flagged.
    let issues = spacing_issues("他——就是那個人");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "should not flag standard double em dash: {issues:?}"
    );
}

#[test]
fn triple_em_dash_flagged() {
    // ——— (3+) is non-standard: should be flagged.
    let issues = spacing_issues("他———就是那個人");
    assert!(
        issues.iter().any(|(c, _)| c.contains("不重複")),
        "should flag triple em dash: {issues:?}"
    );
}

#[test]
fn space_after_fullwidth_punct_before_latin() {
    // Per guidelines: "全形標點與其他字元之間不加空格" applies to Latin too.
    let issues = spacing_issues("好， Test很好");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag space after fullwidth comma before Latin: {issues:?}"
    );
}

#[test]
fn no_space_after_fullwidth_punct_before_latin() {
    let issues = spacing_issues("好，Test很好");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should not flag when no space after fullwidth punct: {issues:?}"
    );
}

#[test]
fn space_after_fullwidth_punct_before_digit() {
    let issues = spacing_issues("共 3 項，其中有 2 項");

    // "，其" has no space → OK. But "， 2" would be flagged. This input has no
    // space after comma, so no flag.
    assert!(
        !issues
            .iter()
            .any(|(c, s)| c.contains("全形標點") && s.is_empty()),
        "no space after comma here: {issues:?}"
    );
    // Now with space after comma before digit:
    let issues2 = spacing_issues("共有， 2項");
    assert!(
        issues2.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag space after fullwidth comma before digit: {issues2:?}"
    );
}

// Edge-case stress tests for sliding-window rewrite

#[test]
fn space_at_text_start_before_fullwidth_punct() {
    // Leading space before fullwidth punct: prev is None, so rule 3
    // space-before-punct requires prev to be non-space content → no fire.
    let issues = spacing_issues(" ，好開心");
    assert!(
        !issues
            .iter()
            .any(|(c, s)| c.contains("全形標點") && s.is_empty()),
        "leading space before punct should not flag (no preceding content): {issues:?}"
    );
}

#[test]
fn trailing_spaces_after_fullwidth_punct() {
    // Text ends with spaces after fullwidth punct: the forward scan for
    // space-after-punct should not fire because after_ch stays ' '.
    let issues = spacing_issues("好，   ");
    assert!(
        !issues
            .iter()
            .any(|(c, s)| c.contains("全形標點") && s.is_empty()),
        "trailing spaces after punct at end of text should not flag: {issues:?}"
    );
}

#[test]
fn fullwidth_punct_then_space_then_fullwidth_punct() {
    // ， ？: space between two fullwidth puncts. Rule 3 space-after-punct only
    // fires if after_ch is CJK/alphanumeric, not another punct.
    let issues = spacing_issues("好， ？");
    assert!(
        !issues
            .iter()
            .any(|(c, s)| c.contains("全形標點") && s.is_empty()),
        "space between two fullwidth puncts should not flag: {issues:?}"
    );
}

#[test]
fn punct_run_resets_after_non_punct() {
    // ！好！: the second ！ should not be flagged as repeated because a CJK
    // char intervenes, resetting same_punct_run.
    let issues = spacing_issues("太棒！好！");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "punct separated by content should not flag as repeated: {issues:?}"
    );
}

#[test]
fn different_punct_not_repeated() {
    // ，。: different fullwidth punct chars should not trigger rule 4.
    let issues = spacing_issues("好，好。");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("不重複")),
        "different punct chars should not flag as repeated: {issues:?}"
    );
}

#[test]
fn single_char_text() {
    // Single CJK character: no next char, no prev initially.
    let issues = spacing_issues("好");
    assert!(
        issues.is_empty(),
        "single char should produce no spacing issues: {issues:?}"
    );
}

#[test]
fn only_spaces() {
    let issues = spacing_issues("   ");
    assert!(
        issues.is_empty(),
        "only spaces should produce no spacing issues: {issues:?}"
    );
}

#[test]
fn empty_text() {
    let issues = spacing_issues("");
    assert!(
        issues.is_empty(),
        "empty text should produce no spacing issues"
    );
}

#[test]
fn cjk_space_latin_space_cjk_correct() {
    // Properly spaced: CJK SPACE Latin SPACE CJK, no issues.
    let issues = spacing_issues("好 ABC 好");
    assert!(
        !issues.iter().any(|(c, _)| c.contains("中英文")),
        "properly spaced CJK-Latin-CJK should not flag: {issues:?}"
    );
}

#[test]
fn fullwidth_digit_adjacent_to_cjk() {
    // Fullwidth digit next to CJK should flag rule 5 (fullwidth→halfwidth) but
    // NOT rule 2 (CJK-digit spacing), because the fullwidth digit is not an
    // ASCII digit.
    let issues = spacing_issues("有３項");
    let has_fw_digit = issues.iter().any(|(c, _)| c.contains("數字應使用"));
    assert!(has_fw_digit, "should flag fullwidth digit: {issues:?}");
}

#[test]
fn rule3_space_before_punct_with_latin_content() {
    // Latin char before space before fullwidth punct.
    let issues = spacing_issues("test ，好");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag space before fullwidth punct after Latin: {issues:?}"
    );
}

#[test]
fn rule3_space_before_punct_with_digit_content() {
    // Digit before space before fullwidth punct.
    let issues = spacing_issues("123 ，好");
    assert!(
        issues.iter().any(|(c, _)| c.contains("全形標點")),
        "should flag space before fullwidth punct after digit: {issues:?}"
    );
}

#[test]
fn rule4_quadruple_ellipsis() {
    // 4 consecutive ellipsis marks: run=1 is OK (paired), run=2 and 3 flagged.
    let issues = spacing_issues("他說…………算了");
    let repeat_count = issues.iter().filter(|(c, _)| c.contains("不重複")).count();
    assert_eq!(
        repeat_count, 2,
        "4 ellipsis should flag 2 extras: {issues:?}"
    );
}

#[test]
fn rule1_boundary_latin_then_cjk() {
    // Latin immediately followed by CJK.
    let issues = spacing_issues("Hello世界");
    assert!(
        issues.iter().any(|(c, _)| c.contains("中英文")),
        "should flag missing space Latin→CJK: {issues:?}"
    );
}

#[test]
fn rule2_boundary_digit_then_cjk() {
    // Digit immediately followed by CJK.
    let issues = spacing_issues("42個");
    assert!(
        issues.iter().any(|(c, _)| c.contains("數字")),
        "should flag missing space digit→CJK: {issues:?}"
    );
}
