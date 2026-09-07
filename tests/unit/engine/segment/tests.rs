use super::*;

fn test_segmenter() -> Segmenter {
    Segmenter::new(
        [
            "蘋果",
            "香蕉",
            "橘子",
            "台灣",
            "軟體",
            "程式",
            "程式語言",
            "人工智慧",
            "機器學習",
        ]
        .iter()
        .map(|s| s.to_string()),
    )
}

#[test]
fn basic_segmentation() {
    let seg = test_segmenter();
    let tokens = seg.segment("蘋果和香蕉");
    assert_eq!(tokens.len(), 3); // 蘋果, 和, 香蕉
    assert_eq!(tokens[0].text, "蘋果");
    assert!(tokens[0].in_dict);
    assert_eq!(tokens[1].text, "和");
    assert!(!tokens[1].in_dict);
    assert_eq!(tokens[2].text, "香蕉");
    assert!(tokens[2].in_dict);
}

#[test]
fn longest_match_wins() {
    let seg = test_segmenter();

    // "程式語言" should match as one token, not "程式" + "語言". MMSEG Rule 2:
    // ["程式語言"(4)] (1 word, avg=4) beats
    //               ["程式"(2), "語"(1), "言"(1)] (3 words, avg≈1.3) on avg word length.
    let tokens = seg.segment("程式語言");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].text, "程式語言");
    assert!(tokens[0].in_dict);
}

#[test]
fn single_char_fallback() {
    let seg = test_segmenter();
    let tokens = seg.segment("你好");
    // Neither char in dict, each is a separate token.
    assert_eq!(tokens.len(), 2);
    assert!(!tokens[0].in_dict);
    assert!(!tokens[1].in_dict);
}

#[test]
fn mixed_content() {
    let seg = test_segmenter();
    let tokens = seg.segment("台灣的蘋果很好吃");
    let dict_tokens: Vec<&str> = tokens
        .iter()
        .filter(|t| t.in_dict)
        .map(|t| t.text.as_str())
        .collect();
    assert!(dict_tokens.contains(&"台灣"));
    assert!(dict_tokens.contains(&"蘋果"));
}

#[test]
fn byte_offsets_correct() {
    let seg = test_segmenter();
    let text = "蘋果和香蕉";
    let tokens = seg.segment(text);
    for token in &tokens {
        assert_eq!(
            &text[token.offset..token.offset + token.text.len()],
            token.text
        );
    }
}

#[test]
fn empty_input() {
    let seg = test_segmenter();
    assert!(seg.segment("").is_empty());
}

#[test]
fn ascii_passes_through() {
    let seg = test_segmenter();
    let tokens = seg.segment("hello world");
    // Each ASCII char is a separate token (no ASCII words in dict).
    assert_eq!(tokens.len(), 11);
}

#[test]
fn word_count_basic() {
    let seg = test_segmenter();
    assert_eq!(seg.word_count("蘋果和香蕉"), 2);
}

#[test]
fn has_context_clue_found() {
    let seg = test_segmenter();
    assert!(seg.has_context_clue("台灣的蘋果", &["蘋果", "橘子"]));
}

#[test]
fn has_context_clue_not_found() {
    let seg = test_segmenter();
    assert!(!seg.has_context_clue("你好世界", &["蘋果", "橘子"]));
}

#[test]
fn from_rules_builds_dict() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    let rules = vec![SpellingRule::new(
        "軟件",
        vec!["軟體".into()],
        RuleType::CrossStrait,
    )];
    let seg = Segmenter::from_rules(&rules);
    // Dict should contain "軟件", "軟體", and all stop words.
    assert!(seg.trie.contains("軟件"));
    assert!(seg.trie.contains("軟體"));
    assert!(seg.trie.contains("的"));
}

#[test]
fn stop_words_help_segmentation() {
    let seg = Segmenter::new(
        STOP_WORDS
            .iter()
            .map(|s| s.to_string())
            .chain(["蘋果", "好吃"].iter().map(|s| s.to_string())),
    );
    let tokens = seg.segment("蘋果很好吃");
    let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, vec!["蘋果", "很", "好吃"]);
}

#[test]
fn numbers_as_stop_words() {
    let seg = Segmenter::new(STOP_WORDS.iter().map(|s| s.to_string()));
    let tokens = seg.segment("三個人");
    assert!(tokens[0].in_dict); // 三
    assert!(tokens[1].in_dict); // 個
    assert!(tokens[2].in_dict); // 人
}

#[test]
fn count_context_clues_multiple() {
    let seg = test_segmenter();
    // "蘋果" and "香蕉" both present as dict tokens, "橘子" absent.
    assert_eq!(
        seg.count_context_clues("蘋果和香蕉", &["蘋果", "橘子", "香蕉"]),
        2
    );
}

#[test]
fn count_context_clues_none() {
    let seg = test_segmenter();
    assert_eq!(seg.count_context_clues("你好世界", &["蘋果", "橘子"]), 0);
}

// MMSEG-specific tests

/// MMSEG Rule 3 (min variance) resolves ambiguity in "研究生命科學".
/// FMM greedy-left takes "研究生"(3) first; MMSEG finds the more-balanced
/// chunk ["研究"(2), "生命"(2), "科學"(2)] scores higher on Rule 1 (total=6
/// vs 5 for FMM's best chunk) and emits "研究" as first token.
#[test]
fn mmseg_chunk_scoring_beats_fmm() {
    let seg = Segmenter::new(
        ["研究生", "研究", "生命", "科學", "命"]
            .iter()
            .map(|s| s.to_string()),
    );
    // FMM would emit "研究生" first; MMSEG should emit "研究".
    let tokens = seg.segment("研究生命科學");
    assert_eq!(
        tokens[0].text, "研究",
        "MMSEG Rule 1 prefers chunk with higher total: 研究+生命+科學=6 > 研究生+命+科=5"
    );
}

/// MMSEG Rule 2 (max avg / min words) prefers the chunk with fewer, longer
/// words when total chars are equal.
#[test]
fn mmseg_rule2_min_words() {
    // "ABCD" where "AB"(2) and "ABCD"(4) are both in dict, nothing follows.
    // Chunk ["ABCD"(4)] has 1 word, avg=4. Chunk ["AB"(2), "C"(1), "D"(1)] has
    // 3 words, avg≈1.3. Equal total chars (4 each), Rule 2 picks "ABCD" (fewer
    // words).
    let seg = Segmenter::new(["AB", "ABCD", "C", "D"].iter().map(|s| s.to_string()));
    let tokens = seg.segment("ABCD");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].text, "ABCD");
}

/// MMSEG Rule 3 (min variance) resolves ties after Rules 1 and 2.
#[test]
fn mmseg_rule3_min_variance() {
    // "ABCDE" (5 chars). Dict has "AB"(2), "ABC"(3), "CD"(2), "DE"(2), "E"(1).
    // Chunk from "ABC": ["ABC"(3), "DE"(2)] = total 5, 2 words, avg=2.5
    //                    var: ((3-2.5)²+(2-2.5)²)/2 = (0.25+0.25)/2 = 0.25
    // Chunk from "AB": ["AB"(2), "CD"(2), "E"(1)] = total 5, 3 words Rule 2
    // (min words): "ABC"-first chunk wins (2 < 3 words).
    let seg = Segmenter::new(["AB", "ABC", "CD", "DE", "E"].iter().map(|s| s.to_string()));
    let tokens = seg.segment("ABCDE");
    assert_eq!(tokens[0].text, "ABC");
}

/// Clue absorption: MMSEG improves recall for cases where Rule 1 already
/// disambiguates in favour of the segmentation that exposes the clue word.
/// "研究生命科學": "研究" (the clue) surfaces as a standalone token.
#[test]
fn mmseg_clue_surfaces_when_rule1_wins() {
    let seg = Segmenter::new(
        ["研究生", "研究", "生命", "科學"]
            .iter()
            .map(|s| s.to_string()),
    );

    // Rule 1 (total chars): ["研究"(2),"生命"(2),"科學"(2)] = 6
    //                   vs  ["研究生"(3),"命"(1-OOV),"科"(1-OOV)] = 5
    // → "研究" chunk wins, so "研究" appears as a token.
    assert!(seg.has_context_clue("研究生命科學", &["研究"]));
}

/// Single-char OOV fallback: in_dict=false for fallback tokens means a
/// single-char clue that happens to equal a fallback char is NOT matched.
#[test]
fn single_char_oov_not_matched_as_clue() {
    // Clue "人" (single char), but "人" is NOT in this segmenter's dict.
    let seg = Segmenter::new(["蘋果"].iter().map(|s| s.to_string()));
    // "人" will be an OOV fallback with in_dict=false.
    assert!(!seg.has_context_clue("蘋果很好吃人人愛", &["人"]));
}

/// Stop words in from_rules() get freq=10, rule terms get freq=1.
#[test]
fn freq_weights_assigned_correctly() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    let rules = vec![SpellingRule::new(
        "軟件",
        vec!["軟體".into()],
        RuleType::CrossStrait,
    )];
    let seg = Segmenter::from_rules(&rules);
    // Stop word "的" must have freq=10.
    assert_eq!(seg.trie.get_freq("的"), Some(10));
    // Rule term "軟件" must have freq=1.
    assert_eq!(seg.trie.get_freq("軟件"), Some(1));
}

/// MMSEG deterministic tiebreaker: leftmost-longest resolves final ties.
#[test]
fn mmseg_tiebreaker_leftmost_longest() {
    // "ABAB": dict has "AB"(2) and "A"(1-OOV), "B"(1-OOV). Two possible 2-word
    // chunks starting at pos 0:
    //   ["AB"(2), "AB"(2)] total=4, avg=2, var=0
    //   ["A"(1), "B"(1), "AB"(2)]: but this is 3-word chunk; total=4, avg=4/3
    // Rule 2: ["AB","AB"] (2 words) wins over 3-word chunk → "AB" as first
    // token.
    let seg = Segmenter::new(["AB"].iter().map(|s| s.to_string()));
    let tokens = seg.segment("ABAB");
    assert_eq!(tokens[0].text, "AB");
    assert_eq!(tokens[1].text, "AB");
}

// Clue absorption (17.1b) tests

/// MMSEG Rule 1 prefers "下拉菜單"(4) as one token over "下拉"(2)+"菜單"(2)
/// because 4-char single token wins on total chars in the chunk.  The clue
/// "下拉" is absorbed into the longer token and never surfaces standalone.
/// The substring check recovers it.
#[test]
fn clue_absorption_substring_match() {
    let seg = Segmenter::new(
        ["下拉", "菜單", "下拉菜單", "操作"]
            .iter()
            .map(|s| s.to_string()),
    );
    // Without substring matching, this would return false.
    assert!(seg.has_context_clue("下拉菜單的操作", &["下拉"]));
}

/// count_context_clues also handles absorption.
#[test]
fn clue_absorption_count() {
    let seg = Segmenter::new(
        ["下拉", "菜單", "下拉菜單", "操作"]
            .iter()
            .map(|s| s.to_string()),
    );
    assert_eq!(
        seg.count_context_clues("下拉菜單的操作", &["下拉", "操作"]),
        2
    );
}

/// Clue as suffix of a longer token.
#[test]
fn clue_absorption_suffix() {
    let seg = Segmenter::new(["人工智慧", "智慧", "應用"].iter().map(|s| s.to_string()));
    assert!(seg.has_context_clue("人工智慧的應用", &["智慧"]));
}

/// Clue that is not a substring of any token should still return false.
#[test]
fn clue_absorption_no_false_positive() {
    let seg = Segmenter::new(["下拉菜單", "操作"].iter().map(|s| s.to_string()));
    assert!(!seg.has_context_clue("下拉菜單的操作", &["選單"]));
}

/// Clue as infix of a longer token (neither prefix nor suffix).
#[test]
fn clue_absorption_infix() {
    let seg = Segmenter::new(["人工智慧型", "智慧", "應用"].iter().map(|s| s.to_string()));
    assert!(seg.has_context_clue("人工智慧型的應用", &["智慧"]));
}

/// token_contains_clue unit tests.
#[test]
fn token_contains_clue_basic() {
    assert!(token_contains_clue("下拉菜單", "下拉"));
    assert!(token_contains_clue("下拉菜單", "菜單"));
    assert!(token_contains_clue("人工智慧", "智慧"));
    assert!(token_contains_clue("人工智慧", "人工"));
    // Equal strings: not a substring (caller handles exact match).
    assert!(!token_contains_clue("下拉", "下拉"));
    // Empty clue.
    assert!(!token_contains_clue("下拉菜單", ""));
    // Clue longer than token.
    assert!(!token_contains_clue("下拉", "下拉菜單"));
}

// General vocabulary supplement tests

/// General vocab is included in from_rules() dict.
#[test]
fn general_vocab_in_from_rules_dict() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    let rules = vec![SpellingRule::new(
        "軟件",
        vec!["軟體".into()],
        RuleType::CrossStrait,
    )];
    let seg = Segmenter::from_rules(&rules);
    // General vocab words should be present.
    assert!(seg.trie.contains("提供"));
    assert!(seg.trie.contains("目前"));
    assert!(seg.trie.contains("重要"));
    assert!(seg.trie.contains("例如"));
    // General vocab has freq=5 (between rule=1 and stop=10).
    assert_eq!(seg.trie.get_freq("提供"), Some(5));
}

/// Natural prose context clue recall: general vocab provides multi-char
/// tokens that prevent the segmenter from falling back to single chars,
/// keeping surrounding dict tokens intact for clue matching.
#[test]
fn general_vocab_improves_clue_recall() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    let rules = vec![SpellingRule {
        context_clues: Some(vec!["分析".into(), "處理".into()]),
        ..SpellingRule::new("數據", vec!["資料".into()], RuleType::CrossStrait)
    }];
    let seg = Segmenter::from_rules(&rules);

    // "提供" and "處理" are general vocab; "數據" and "分析"/"處理" are rule
    // terms. Without general vocab, "目前提供的數據處理" would degrade on
    // "提供" (single-char fallback).
    assert!(seg.has_context_clue("目前提供的數據處理方式", &["處理"]));
    assert!(seg.has_context_clue("進行數據分析的過程", &["分析"]));
}

/// General vocab words segment as multi-char tokens, not single-char
/// fallback.
#[test]
fn general_vocab_segments_as_multichar() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    let rules = vec![SpellingRule::new(
        "軟件",
        vec!["軟體".into()],
        RuleType::CrossStrait,
    )];
    let seg = Segmenter::from_rules(&rules);
    let tokens = seg.segment("目前提供的重要功能");
    let dict_words: Vec<&str> = tokens
        .iter()
        .filter(|t| t.in_dict && t.text.chars().count() > 1)
        .map(|t| t.text.as_str())
        .collect();
    assert!(dict_words.contains(&"目前"));
    assert!(dict_words.contains(&"提供"));
    assert!(dict_words.contains(&"重要"));
    assert!(dict_words.contains(&"功能"));
}

/// General vocab does not override rule term freq (rule=1 stays 1).
#[test]
fn general_vocab_does_not_override_rule_freq() {
    use crate::rules::ruleset::{RuleType, SpellingRule};
    // "設計" is both a general vocab word AND could be a rule term.
    let rules = vec![SpellingRule::new(
        "設計",
        vec!["設計".into()],
        RuleType::CrossStrait,
    )];
    let seg = Segmenter::from_rules(&rules);

    // Rule term "設計" inserted first with freq=1; general vocab uses
    // or_insert(5) which does NOT overwrite the existing freq=1.
    assert_eq!(seg.trie.get_freq("設計"), Some(1));
}

#[test]
fn word_straddles_boundary_detects_cross_word_match() {
    // "累積" + "分佈" are distinct words. An AC match for "積分" starting at
    // the 積 in 累積分佈 straddles a word boundary.
    let seg = Segmenter::new(
        ["累積", "分佈", "排程", "序列", "引導"]
            .iter()
            .map(|s| s.to_string()),
    );

    let text = "累積分佈函數";
    // "積分" would start at byte offset of 積 (=3 in UTF-8 for 累).
    let boundary = "累".len(); // left edge of would-be "積分" match
    assert!(
        seg.word_straddles_boundary(text, boundary),
        "累積 should straddle the boundary at 積"
    );

    let text2 = "排程序列";
    let boundary2 = "排".len(); // left edge of would-be "程序" match
    assert!(
        seg.word_straddles_boundary(text2, boundary2),
        "排程 should straddle the boundary at 程"
    );

    let text3 = "引導出平滑的";
    let boundary3 = "引".len(); // left edge of would-be "導出" match
    assert!(
        seg.word_straddles_boundary(text3, boundary3),
        "引導 should straddle the boundary at 導"
    );
}

#[test]
fn word_straddles_boundary_allows_real_words() {
    // When "積分" stands alone (e.g. "會員積分兌換"), no straddling.
    let seg = Segmenter::new(["會員", "兌換"].iter().map(|s| s.to_string()));
    let text = "會員積分兌換";
    let boundary = "會員".len(); // left edge of "積分"
    assert!(
        !seg.word_straddles_boundary(text, boundary),
        "no dict word should straddle between 會員 and 積分"
    );
    let boundary_right = "會員積分".len(); // right edge of "積分"
    assert!(
        !seg.word_straddles_boundary(text, boundary_right),
        "no dict word should straddle between 積分 and 兌換"
    );
}

#[test]
fn word_straddles_boundary_stops_before_non_cjk_suffix() {
    let seg = Segmenter::new(["程式A"].iter().map(|s| s.to_string()));
    let text = "我寫程式A";
    let boundary = text.find('式').unwrap() + '式'.len_utf8();

    assert!(
        !seg.word_straddles_boundary(text, boundary),
        "mixed-script dictionary entries should not count after probing crosses into ASCII"
    );
}

#[test]
fn boundary_bitmap_ignores_non_cjk_internal_boundary() {
    let seg = Segmenter::new(["程式A"].iter().map(|s| s.to_string()));
    let text = "我寫程式A";
    let boundary = text.find('式').unwrap() + '式'.len_utf8();
    let bitmap = seg.build_boundary_bitmap(text);

    assert!(
        !bitmap.start_straddles(boundary),
        "bitmap precompute should match direct probing for mixed-script words"
    );
}

#[test]
fn end_boundary_limit_still_considers_words_starting_at_match_start() {
    // The end-boundary limiter should ignore dictionary words that start
    // strictly inside the match, but it must still catch a longer word that
    // begins exactly at the match start and extends past the match.
    let seg = Segmenter::new(["項目管理"].iter().map(|s| s.to_string()));
    let text = "項目管理流程";
    let start = 0;
    let end = "項目".len();

    assert!(
        seg.word_straddles_boundary_with_limit(text, end, Some(start)),
        "項目管理 should straddle the end boundary of 項目"
    );
}

#[test]
fn end_boundary_limit_preserves_true_distance_from_boundary() {
    let seg = Segmenter::new(["操作", "操作系統"].iter().map(|s| s.to_string()));
    let text = "操作系統提供服務";
    let start = 0;
    let end = "操作系統".len();

    assert!(
        !seg.word_straddles_boundary_with_limit(text, end, Some(start)),
        "the exact match should not be mistaken for a longer crossing word when inner starts are skipped"
    );
}
