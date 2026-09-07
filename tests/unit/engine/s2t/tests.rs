use super::*;

fn converter() -> S2TConverter {
    S2TConverter::new()
}

#[test]
fn basic_character_conversion() {
    let c = converter();
    assert_eq!(c.convert("国"), "國");
    assert_eq!(c.convert("学"), "學");
    assert_eq!(c.convert("计算机"), "計算機");
}

#[test]
fn phrase_conversion() {
    let c = converter();

    // STPhrases handles context-dependent multi-char mappings. Note:
    // 操作系统→操作系統 (char-level only, no TWPhrases). 作業系統 is a
    // TWPhrases mapping that zhtw-mcp handles separately.
    assert_eq!(c.convert("操作系统"), "操作系統");

    // 内存→內存 (char-level only). 記憶體 is a TWPhrases mapping handled by
    // zhtw-mcp's cross_strait rules, not by s2t.
    assert_eq!(c.convert("内存"), "內存");
    // 一丝不挂→一絲不掛 (in STPhrases: phrase-level conversion).
    assert_eq!(c.convert("一丝不挂"), "一絲不掛");
}

#[test]
fn tw_variant_normalization() {
    let c = converter();
    // TWVariants: 裏→裡, 着→著
    assert_eq!(c.convert("裏"), "裡");
    assert_eq!(c.convert("着"), "著");
}

#[test]
fn mixed_text() {
    let c = converter();
    let input = "这是一个简单的测试";
    let output = c.convert(input);
    // Should be fully Traditional.
    assert!(!output.contains('这'));
    assert!(!output.contains('个'));
    assert!(!output.contains('简'));
}

#[test]
fn ascii_passthrough() {
    let c = converter();
    assert_eq!(c.convert("hello world 123"), "hello world 123");
}

#[test]
fn empty_input() {
    let c = converter();
    assert_eq!(c.convert(""), "");
}

#[test]
fn already_traditional() {
    let c = converter();
    // Traditional text should pass through unchanged.
    assert_eq!(c.convert("國學計算機"), "國學計算機");
}

#[test]
fn dict_sizes_reasonable() {
    let c = converter();
    assert!(
        c.phrase_count() > 35000,
        "expected >35K phrases, got {}",
        c.phrase_count()
    );
    assert!(
        c.char_count() > 3000,
        "expected >3K char mappings, got {}",
        c.char_count()
    );
}

#[test]
fn ambiguous_chars_are_not_safe_fallbacks() {
    let safe: std::collections::HashSet<char> = s2t_data::ST_CHARACTERS
        .iter()
        .map(|&(from, _)| from)
        .collect();
    for &ch in s2t_data::AMBIGUOUS_ST_CHARACTERS {
        assert!(
            !safe.contains(&ch),
            "ambiguous char {ch} leaked into ST_CHARACTERS"
        );
    }
}

#[test]
fn high_risk_ambiguous_chars_are_excluded() {
    // Pinned as a whole set, so a silent shrink fails rather than going
    // unnoticed.
    let mut ambiguous: Vec<char> = s2t_data::AMBIGUOUS_ST_CHARACTERS.to_vec();
    ambiguous.sort_unstable();
    let mut expected = vec![
        '丑', '伙', '佣', '克', '余', '姜', '干', '复', '后', '咸', '沈', '症', '范', '舍', '里',
    ];
    expected.sort_unstable();
    assert_eq!(ambiguous, expected);
}

#[test]
fn ambiguous_chars_resolve_by_phrase_not_by_guess() {
    let c = converter();

    // 复 has three readings (復 restore, 複 duplicate, 覆 cover). Only the
    // phrase table may choose between them.
    assert_eq!(c.convert("复习"), "複習");
    assert_eq!(c.convert("反复"), "反覆");
    assert_eq!(c.convert("恢复"), "恢復");
    // EXTRA_PHRASES additions: absent from OpenCC, hand-verified here.
    assert_eq!(c.convert("复盘"), "復盤");
    assert_eq!(c.convert("复联"), "復聯");

    // Uncovered contexts keep 复 rather than guessing. Leaving a visible
    // Simplified residue beats silently emitting 復矩陣 for 複矩陣: a wrong
    // Traditional character reads as correct and nothing downstream flags it.
    // Fix these by adding EXTRA_PHRASES entries, never by defaulting the bare
    // character.
    assert_eq!(c.convert("复矩阵"), "复矩陣");
    assert_eq!(c.convert("复购"), "复購");
}

#[test]
fn os_terminology() {
    let c = converter();

    // s2t (char-level) should NOT produce the same wrong results as s2twp. 进程
    // → 進程 (not 程序, which is what s2twp TWPhrases does)
    assert_eq!(c.convert("进程"), "進程");
    // 并行 → 並行 (char-level only, no TWPhrases override)
    assert_eq!(c.convert("并行"), "並行");
}

// -- 52.2 gate tests: identity mapping protection --

#[test]
fn identity_already_correct_tw_terms() {
    let c = converter();

    // These are correct zh-TW terms. Round-trip must produce identical output.
    assert_eq!(c.convert("演算法"), "演算法");
    assert_eq!(c.convert("執行緒"), "執行緒");
    assert_eq!(c.convert("記憶體"), "記憶體");
}

#[test]
fn identity_tw_terms_in_context() {
    let c = converter();
    // Correct TW terms embedded in longer sentences must survive.
    assert_eq!(
        c.convert("這個演算法的時間複雜度是O(n)"),
        "這個演算法的時間複雜度是O(n)"
    );
    assert_eq!(c.convert("主執行緒負責排程"), "主執行緒負責排程");
    assert_eq!(c.convert("記憶體使用量很大"), "記憶體使用量很大");
}

#[test]
fn no_double_conversion_on_tw_variants_targets() {
    let c = converter();

    // Phrase outputs with TWVariant source chars must be pre-normalized, not
    // double-converted. '裏' in phrase targets → '裡' (baked in). Standalone
    // '裏' (char-level) must still normalize to '裡'.
    assert_eq!(c.convert("裏"), "裡");

    // A phrase whose target contained '裏' should output '裡' directly.
    // '一地里' → STPhrases → '一地裡' (pre-normalized from '一地裏').
    assert_eq!(c.convert("一地里"), "一地裡");
}

#[test]
fn mixed_phrase_and_char_adjacent() {
    let c = converter();

    // Text adjacent to a phrase replacement must still be converted. '万众一心'
    // is in STPhrases; adjacent SC chars must be char-converted.
    let result = c.convert("为万众一心而奋斗");
    // '为'→'為', '万众一心'→phrase, '而'→'而', '奋斗'→'奮鬥'
    assert!(!result.contains('为'));
    assert!(!result.contains('奋'));
    assert!(result.contains("眾一心")); // phrase output with pre-normalized '衆'→'眾'
}

#[test]
fn protected_zones_do_not_suppress_adjacent_char_conversion() {
    let c = converter();

    // Char-level output OUTSIDE protected zones must still get TWVariants. '着'
    // at char level → STCharacters identity → TWVariants → '著'. That must NOT
    // be suppressed by nearby phrase zones.
    assert_eq!(c.convert("着"), "著");
    // Mix: phrase match + gap with TWVariant char.
    let input = "一丝不挂着";
    let result = c.convert(input);

    // '一丝不挂' is a phrase → '一絲不掛' (protected). '着' is in the gap →
    // TWVariants → '著'.
    assert!(result.ends_with('著'));
}
