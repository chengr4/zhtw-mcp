use super::*;
use crate::engine::excluded::ByteRange;

fn scan(text: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    scan_spaced_acronyms(&mut Emitter::new(text, &[], &mut issues));
    issues
}

#[test]
fn catches_three_letter_acronym() {
    let issues = scan("使用 C P U 架構");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].found, "C P U");
    assert_eq!(issues[0].suggestions.as_ref(), ["CPU"]);
}

#[test]
fn catches_four_letter_acronym() {
    let issues = scan("F P G A 開發板");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].suggestions.as_ref(), ["FPGA"]);
}

#[test]
fn catches_known_two_letter() {
    let issues = scan("A I 技術");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].suggestions.as_ref(), ["AI"]);
}

#[test]
fn skips_unknown_two_letter() {
    // "X Y" is not a known acronym.
    let issues = scan("座標 X Y 軸");
    assert!(issues.is_empty());
}

#[test]
fn skips_normal_words() {
    let issues = scan("Hello World from Linux");
    assert!(issues.is_empty());
}

#[test]
fn skips_excluded() {
    let excluded = vec![ByteRange { start: 0, end: 50 }];
    let mut issues = Vec::new();
    scan_spaced_acronyms(&mut Emitter::new("C P U 架構", &excluded, &mut issues));
    assert!(issues.is_empty());
}

#[test]
fn acronym_at_end_of_string() {
    let issues = scan("使用 C P U");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].found, "C P U");
    assert_eq!(issues[0].suggestions.as_ref(), ["CPU"]);
}

#[test]
fn multiple_acronyms_in_one_text() {
    let issues = scan("C P U 和 G P U 的效能");
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].suggestions.as_ref(), ["CPU"]);
    assert_eq!(issues[1].suggestions.as_ref(), ["GPU"]);
}

#[test]
fn skips_lowercase_letters() {
    // Lowercase single letters are not ASR artifacts.
    let issues = scan("a b c d e");
    assert!(issues.is_empty());
}

#[test]
fn skips_mixed_case_letters() {
    // 'A b C' has mixed case; not a valid spaced acronym.
    let issues = scan("A b C");
    assert!(issues.is_empty());
}

#[test]
fn known_two_letter_at_end() {
    let issues = scan("使用 A I");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].suggestions.as_ref(), ["AI"]);
}

#[test]
fn five_letter_acronym() {
    let issues = scan("R I S C V 架構");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].suggestions.as_ref(), ["RISCV"]);
}

#[test]
fn does_not_fire_on_already_joined() {
    // If text already says CPU (no spaces), nothing to flag.
    let issues = scan("使用CPU架構");
    assert!(issues.is_empty());
}

#[test]
fn adjacent_to_word_boundary() {
    // 'CPU' as part of a word should not be split-detected.
    let issues = scan("myCPU is fast");
    assert!(issues.is_empty());
}
