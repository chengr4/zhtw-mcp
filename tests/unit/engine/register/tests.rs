use super::*;

#[test]
fn plain_prose_is_casual() {
    assert_eq!(
        detect_register("我們今天要處理這個問題。"),
        Register::Casual
    );
}

#[test]
fn empty_text_is_casual() {
    assert_eq!(detect_register(""), Register::Casual);
}

#[test]
fn a_salutation_settles_the_register() {
    assert_eq!(detect_register("敬啟者：茲有一事相商。"), Register::Formal);
}

#[test]
fn a_sign_off_past_the_head_still_settles_it() {
    // The whole point of reading the document rather than its first hundred
    // characters: the strongest evidence is at the bottom.
    let body = "說明事項。".repeat(60);
    let text = format!("{body}\n此致\n敬禮");
    assert!(text.chars().count() > FORMAL_HEAD_CHARS);
    assert_eq!(detect_register(&text), Register::Formal);
}

#[test]
fn an_anchor_inside_a_word_is_not_an_anchor() {
    // 台端 sits inside 平台端, 前台端 and 後台端, which is ordinary technical
    // prose about the platform or front-end side of a system.
    for text in [
        "這個錯誤發生在平台端。",
        "前台端與後台端都要修。",
        "資料在後台端就已經遺失了。",
    ] {
        assert_eq!(detect_register(text), Register::Casual, "{text}");
    }
}

#[test]
fn a_connective_is_not_a_sign_off() {
    // 此致 sits inside 因此致使, one of the commonest connectives there is.
    assert_eq!(detect_register("因此致使資料遺失。"), Register::Casual);
    assert_eq!(detect_register("因此致命的錯誤發生了。"), Register::Casual);
}

#[test]
fn a_sign_off_that_runs_into_the_next_word_is_not_one() {
    // Nothing in front of 此致 here, so the left-hand test passes it; what
    // rejects it is that 致 runs on into 命. Sentence-initial is the case the
    // connective test above could not reach.
    for text in [
        "此致命的錯誤發生了。",
        "此致使資料遺失。",
        "此致力於改善效能。",
    ] {
        assert_eq!(detect_register(text), Register::Casual, "{text}");
    }

    // The real sign-off still counts, on its own line and inline.
    assert_eq!(detect_register("說明如上。\n此致\n敬禮"), Register::Formal);
    assert_eq!(detect_register("說明如上。此致 敬禮"), Register::Formal);
}

#[test]
fn a_discount_is_not_a_request() {
    // 惠請 sits inside 優惠請洽, which is advertising copy.
    assert_eq!(detect_register("優惠請洽門市人員。"), Register::Casual);
}

#[test]
fn an_anchor_after_punctuation_or_a_line_break_counts() {
    assert_eq!(detect_register("說明如上。\n此致\n敬禮"), Register::Formal);
    assert_eq!(detect_register("報告完畢，惠請查照。"), Register::Formal);
}

#[test]
fn an_anchor_opening_the_document_counts() {
    // Nothing in front of it at all is the boundary case that matters most,
    // because it is what a salutation actually looks like.
    assert_eq!(detect_register("謹此陳報。"), Register::Formal);
}

#[test]
fn contract_nouns_count_only_in_the_head() {
    assert_eq!(detect_register("本合約之當事人如下。"), Register::Formal);

    let padding = "這是一段說明文字。".repeat(20);
    let text = format!("{padding}本合約之當事人如下。");
    assert!(text.chars().count() > FORMAL_HEAD_CHARS);
    assert_eq!(detect_register(&text), Register::Casual);
}

#[test]
fn an_anaphoric_contract_reference_is_not_self_reference() {
    // 該合約 is how an article refers to a contract it has just named, not how
    // a contract refers to itself.
    for text in [
        "該合約價值十億美元，我們予以處理。",
        "此合約的爭議點在於付款條件。",
        "這篇文章討論合約與契約的差異。",
    ] {
        assert_eq!(detect_register(text), Register::Casual, "{text}");
    }
}

#[test]
fn a_place_name_ending_in_the_determiner_is_not_a_contract() {
    // 本合約 sits inside 日本合約, which is a contract with Japan rather than a
    // contract naming itself.
    assert_eq!(
        detect_register("日本合約的談判仍在進行。"),
        Register::Casual
    );
}

#[test]
fn the_head_boundary_lands_on_a_character() {
    // A multi-byte head cut must not slice through a CJK character. Every
    // prefix length from nothing to past the window has to be safe.
    for n in 0..(FORMAL_HEAD_CHARS + 20) {
        let text = "說".repeat(n);
        assert_eq!(detect_register(&text), Register::Casual, "n={n}");
    }
}

#[test]
fn an_anchor_after_a_digit_or_a_latin_letter_is_not_an_anchor() {
    for text in ["2024台端的說明如下。", "版本3此致 敬禮"] {
        assert_eq!(detect_register(text), Register::Casual, "{text}");
    }
}

#[test]
fn an_anchor_split_across_a_latin_run_is_not_an_anchor() {
    assert_eq!(detect_register("平台 端點的設定"), Register::Casual);
}
