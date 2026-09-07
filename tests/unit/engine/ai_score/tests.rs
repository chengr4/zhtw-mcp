use super::*;

#[test]
fn short_text_returns_none() {
    let result = compute_ai_score("短文", &[], &[], &[], 1.0);
    assert!(result.is_none());
}

#[test]
fn clean_text_low_score() {
    let text = "台灣的半導體產業在全球市場中佔有重要地位。".repeat(30);
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    assert!(
        report.score <= 0.3,
        "clean text should score low: {:.2}",
        report.score
    );
}

#[test]
fn ai_heavy_text_high_score() {
    // Build text loaded with AI patterns.
    let filler = "這是正常的技術段落。";
    let mut text = String::new();
    for i in 0..80 {
        match i % 8 {
            0 => text.push_str("更重要的是，這個技術非常關鍵。"),
            1 => text.push_str("值得注意的是，我們發現了新的問題。"),
            2 => text.push_str("這意味著我們需要重新評估方案。"),
            3 => text.push_str("不容忽視的影響深遠。"),
            4 => text.push_str("深刻影響了整個產業的發展。"),
            _ => text.push_str(filler),
        }
    }
    // Add some structural pattern issues.
    let structural_issues: Vec<Issue> = (0..3)
        .map(|i| {
            Issue::new(
                i,
                1,
                "",
                vec![],
                IssueType::AiStyle,
                crate::rules::ruleset::Severity::Info,
            )
            .with_structural_family(StructuralFamily::Tricolon)
        })
        .collect();
    let result = compute_ai_score(&text, &structural_issues, &[], &[], 1.0);
    let report = result.unwrap();
    assert!(
        report.score >= 0.5,
        "AI-heavy text should score high: {:.2}",
        report.score
    );
    assert!(!report.markers.is_empty());
    assert!(!report.top_signals.is_empty());
}

#[test]
fn sentence_variability_uniform_low() {
    // All sentences nearly identical length -> low sigma -> contributes to
    // score.
    let sentence = "這是一段長度相同的句子內容";
    let mut text = String::new();
    for _ in 0..60 {
        text.push_str(sentence);
        text.push('。');
    }
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    assert!(
        report.sentence_variability.is_some(),
        "should compute variability for 60 sentences"
    );
    let sigma = report.sentence_variability.unwrap();
    assert!(
        sigma < 2.0,
        "uniform sentences should have low sigma: {sigma:.1}"
    );
}

#[test]
fn sentence_variability_varied_high() {
    // Mix of short (>=4 chars) and very long sentences -> high sigma.
    let mut text = String::new();
    for i in 0..30 {
        if i % 2 == 0 {
            text.push_str("這是短句。");
        } else {
            text.push_str(&"這是一段非常非常非常非常非常冗長的句子用來增加長度變異性".repeat(3));
            text.push('。');
        }
    }
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    let sigma = report
        .sentence_variability
        .expect("should compute variability for varied sentences");
    assert!(
        sigma > 10.0,
        "varied sentences should have high sigma: {sigma:.1}"
    );
}

#[test]
fn zero_width_detection() {
    let mut text = "台灣的半導體產業在全球市場中佔有重要地位。".repeat(30);
    // Inject zero-width spaces.
    text.push('\u{200B}');
    text.push_str("更多文字");
    text.push('\u{FEFF}');
    text.push_str("結尾。");
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    assert_eq!(
        report.zero_width_count, 2,
        "should detect 2 zero-width chars"
    );
}

#[test]
fn valid_emoji_and_bidi_controls_are_not_ai_artifacts() {
    // ZWJ is essential to the single family emoji glyph. LRM/RLM are likewise
    // valid when a zh-TW sentence embeds RTL text.
    let chars: Vec<char> = "👩\u{200D}👩\u{200E}עברית\u{200F}".chars().collect();
    assert!(
        !is_suspicious_zero_width_at(&chars, 1),
        "emoji ZWJ must not be reported or deleted"
    );
    assert!(
        !is_suspicious_zero_width_at(&chars, 3) && !is_suspicious_zero_width_at(&chars, 9),
        "directional controls must not be treated as tokenizer residue"
    );

    let complex: Vec<char> = "👩\u{200D}❤️\u{200D}👩".chars().collect();
    assert!(
        !is_suspicious_zero_width_at(&complex, 1) && !is_suspicious_zero_width_at(&complex, 4),
        "variation selectors inside an emoji ZWJ sequence must be preserved"
    );
}

#[test]
fn flag_tags_need_a_terminator() {
    // A complete subdivision code: the payload is checked as well as the
    // terminator, so a two-letter stub is residue however it ends.
    let valid: Vec<char> = "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}"
        .chars()
        .collect();
    assert!(valid
        .iter()
        .enumerate()
        .filter(|(_, &ch)| ('\u{E0020}'..='\u{E007F}').contains(&ch))
        .all(|(i, _)| !is_suspicious_zero_width_at(&valid, i)));

    let malformed: Vec<char> = "🏴\u{E0067}hidden text".chars().collect();
    assert!(is_suspicious_zero_width_at(&malformed, 1));
}

// Both walks are bounded by the longest tag payload a flag can carry, so a long
// run of tag characters cannot make the per-character predicate rescan the run.
// Unbounded, 200k of them took 59 seconds on 781 KB. A payload that is not a
// subdivision code is residue however well formed the sequence looks. Bounding
// the length alone left six characters of hidden payload behind a black flag
// and a terminator.
#[test]
fn only_a_real_subdivision_code_spells_a_flag() {
    let tag = |s: &str| -> Vec<char> {
        let mut v = vec!['\u{1F3F4}'];
        v.extend(
            s.chars()
                .map(|c| char::from_u32(u32::from(c) + 0xE0000).unwrap()),
        );
        v.push('\u{E007F}');
        v
    };
    for spec in FLAG_TAG_SPECS {
        let chars = tag(spec);
        assert!(
            (1..chars.len()).all(|i| !is_suspicious_zero_width_at(&chars, i)),
            "{spec} is a flag"
        );
    }
    // Well formed, right length, not a subdivision.
    for payload in ["hidden", "abcde", "zzzzz", "gb"] {
        let chars = tag(payload);
        assert!(
            (1..chars.len()).all(|i| is_suspicious_zero_width_at(&chars, i)),
            "{payload:?} must not pass as a flag"
        );
    }
    // Unterminated, which is the shape a hidden instruction takes.
    let mut loose = vec!['\u{1F3F4}', '\u{E0067}'];
    loose.extend("hidden".chars());
    assert!(is_suspicious_zero_width_at(&loose, 1));
}

#[test]
fn digits_are_not_emoji_bases_so_residue_between_them_is_caught() {
    // Keycaps are "base FE0F 20E3" and never use a joiner, so admitting digits
    // as emoji bases only let real residue through.
    let chars: Vec<char> = "2\u{200D}024".chars().collect();
    assert!(is_suspicious_zero_width_at(&chars, 1));
}

#[test]
fn a_joiner_needs_the_right_script_and_the_right_shape() {
    // Each of these is residue that an earlier, looser rule excused.
    let cases: &[(&str, &[char])] = &[
        // Indic wants a virama; two bare letters are not the conjunct form.
        (
            "indic without virama",
            &['\u{0915}', '\u{200C}', '\u{0937}'],
        ),
        // One script per joiner: Arabic letter into Devanagari is not one.
        ("across scripts", &['\u{0627}', '\u{200C}', '\u{0915}']),
        // A virama joins into its own script, not into Latin.
        (
            "virama into latin",
            &['\u{0915}', '\u{094D}', '\u{200D}', 'A'],
        ),
        // A modifier belongs on a base, never after a joiner.
        (
            "modifier after joiner",
            &['\u{1F469}', '\u{200D}', '\u{1F3FB}'],
        ),
    ];
    for (label, chars) in cases {
        let idx = chars
            .iter()
            .position(|c| matches!(c, '\u{200C}' | '\u{200D}'))
            .unwrap();
        assert!(
            is_suspicious_zero_width_at(chars, idx),
            "{label} should be reported"
        );
    }

    // A modifier before a joiner is well formed: "👩🏻‍🚀".
    let astronaut: Vec<char> = "\u{1F469}\u{1F3FB}\u{200D}\u{1F680}".chars().collect();
    assert!(!is_suspicious_zero_width_at(&astronaut, 2));
}

#[test]
fn indic_conjuncts_join_across_a_virama() {
    // Devanagari places the joiner after the virama, which is a combining mark
    // rather than a letter, so a letters-only test rejected both.
    for (label, joiner) in [("ZWNJ", '\u{200C}'), ("ZWJ", '\u{200D}')] {
        let chars: Vec<char> = ['\u{0915}', '\u{094D}', joiner, '\u{0937}'].into();
        assert!(
            !is_suspicious_zero_width_at(&chars, 2),
            "{label} after a virama is spelling"
        );
    }
}

#[test]
fn non_emoji_pictograph_blocks_are_not_bases() {
    // Chess symbols are not part of any emoji joiner sequence, so a joiner
    // between two of them is residue.
    let chars: Vec<char> = "\u{1FA00}\u{200D}\u{1FA01}".chars().collect();
    assert!(is_suspicious_zero_width_at(&chars, 1));
}

#[test]
fn zwnj_is_orthography_in_the_scripts_that_use_it() {
    // Persian می‌رود. Same argument that exempts LRM/RLM: this is spelling.
    let persian: Vec<char> = "\u{0645}\u{06CC}\u{200C}\u{0631}\u{0648}\u{062F}"
        .chars()
        .collect();
    assert!(
        !is_suspicious_zero_width_at(&persian, 2),
        "a ZWNJ between Persian letters must not be called residue"
    );

    // Between Han characters it has no orthographic job.
    let han: Vec<char> = "中\u{200C}文".chars().collect();
    assert!(is_suspicious_zero_width_at(&han, 1));
}

#[test]
fn stray_zwj_and_mid_text_bom_remain_suspicious() {
    let chars: Vec<char> = "甲\u{200D}乙\u{FEFF}丙".chars().collect();
    assert!(is_suspicious_zero_width_at(&chars, 1));
    assert!(is_suspicious_zero_width_at(&chars, 3));
}

#[test]
fn zero_width_excluded() {
    let mut text = "台灣的半導體產業在全球市場中佔有重要地位。".repeat(30);
    let zw_offset = text.len();
    text.push('\u{200B}');
    let excluded = vec![ByteRange {
        start: zw_offset,
        end: zw_offset + 3,
    }];
    let result = compute_ai_score(&text, &[], &excluded, &[], 1.0);
    let report = result.unwrap();
    assert_eq!(
        report.zero_width_count, 0,
        "excluded zero-width should not count"
    );
}

#[test]
fn punctuation_profile_uniform_rhythm() {
    // AI-like text: commas at perfectly regular intervals.
    let clause = "這是一個測試，";
    let mut text = String::new();
    for _ in 0..80 {
        text.push_str(clause);
    }
    // Add enough periods for the profile to be computed.
    for _ in 0..15 {
        text.push_str("這是句子結尾。");
    }
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    if let Some(ref profile) = report.punctuation_profile {
        assert!(
            profile.comma.count >= 10,
            "should have enough commas: {}",
            profile.comma.count
        );
        if let Some(cv) = profile.comma.cv {
            assert!(
                cv < 0.3,
                "uniform comma spacing should have low CV: {cv:.2}"
            );
        }
    }
}

#[test]
fn punctuation_profile_varied_rhythm() {
    // Human-like text: wildly varying clause lengths.
    let mut text = String::new();
    for i in 0..40 {
        if i % 3 == 0 {
            text.push_str("短，");
        } else if i % 3 == 1 {
            text.push_str("這是一段比較長的句子用來增加變異性，");
        } else {
            text.push_str("這是一段非常非常非常非常冗長的句子，目的是讓逗號間距的變異係數升高，");
        }
    }
    for _ in 0..15 {
        text.push_str("結尾句子。");
    }
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    if let Some(ref profile) = report.punctuation_profile {
        if let Some(cv) = profile.comma.cv {
            assert!(
                cv >= 0.4,
                "varied comma spacing should have moderate-to-high CV: {cv:.2}"
            );
        }
    }
}

#[test]
fn punctuation_profile_sparse_no_cv() {
    // Text with very few commas: CV should be None.
    let text = "台灣的半導體產業在全球市場中佔有重要地位。".repeat(30);
    let result = compute_ai_score(&text, &[], &[], &[], 1.0);
    let report = result.unwrap();
    if let Some(ref profile) = report.punctuation_profile {
        assert!(
            profile.comma.cv.is_none(),
            "sparse commas should yield no CV"
        );
    }
}

#[test]
fn excluded_ranges_respected() {
    let mut text = String::new();
    for _ in 0..60 {
        text.push_str("更重要的是，這很重要。");
    }
    // Exclude entire text.
    let excluded = vec![ByteRange {
        start: 0,
        end: text.len(),
    }];
    let result = compute_ai_score(&text, &[], &excluded, &[], 1.0);
    assert!(
        result.is_none(),
        "fully excluded text should return None (below char_count threshold)"
    );
}
