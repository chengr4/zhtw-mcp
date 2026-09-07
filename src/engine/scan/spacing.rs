// CJK spacing rules from Chinese Copywriting Guidelines.
//
// 1. Space between CJK and half-width Latin characters
// 2. Space between CJK and digits (except °, %)
// 3. No space adjacent to full-width punctuation
// 4. No repeated full-width punctuation marks
// 5. Full-width digits → half-width

use super::emit::Emitter;
use std::iter::Peekable;
use std::str::CharIndices;

use crate::engine::excluded::{is_excluded, ByteRange};
use crate::rules::ruleset::{Issue, IssueType, Severity};

use super::{is_cjk_ideograph, punct_issue_sev};

/// Create a zero-length insertion issue (missing space) at the given boundary.
fn missing_space_issue(boundary: usize, context: &str) -> Issue {
    punct_issue_sev(boundary, "", " ", context, Severity::Info)
}

/// Create an issue for unwanted spaces that should be removed.
fn unwanted_space_issue(offset: usize, space_len: usize, context: &str) -> Issue {
    Issue::new(
        offset,
        space_len,
        " ".repeat(space_len),
        vec!["".into()],
        IssueType::Punctuation,
        Severity::Info,
    )
    .with_context(context)
}

/// True if ch is a full-width CJK punctuation mark (，。！？；：、「」
/// 『』（）【】《》〈〉——…… etc.).
fn is_fullwidth_punct(ch: char) -> bool {
    matches!(ch,
        '\u{3001}'..='\u{3003}' | // 、。〃
        '\u{3008}'..='\u{3011}' | // 〈〉《》「」『』【】
        '\u{3014}'..='\u{301B}' | // 〔〕〖〗〘〙〚〛
        '\u{FF01}' | // ！
        '\u{FF08}' | // （
        '\u{FF09}' | // ）
        '\u{FF0C}' | // ，
        '\u{FF0E}' | // ．
        '\u{FF1A}' | // ：
        '\u{FF1B}' | // ；
        '\u{FF1F}' | // ？
        '\u{2014}' | // —
        '\u{2026}'   // …
    )
}

/// True if ch is a full-width digit (０-９).
fn is_fullwidth_digit(ch: char) -> bool {
    matches!(ch, '\u{FF10}'..='\u{FF19}')
}

/// Convert a full-width digit to its half-width equivalent.
fn fullwidth_to_halfwidth_digit(ch: char) -> char {
    debug_assert!(is_fullwidth_digit(ch));
    // Safe: fullwidth digits U+FF10..U+FF19 map to U+0030..U+0039.
    char::from_u32(ch as u32 - 0xFF10 + '0' as u32).expect("U+FF10..U+FF19 maps into ASCII 0-9")
}

impl super::Scanner {
    /// Scan for CJK spacing violations (Chinese Copywriting Guidelines).
    ///
    /// Detects:
    /// - Missing space between CJK and Latin characters (rule 1)
    /// - Missing space between CJK and digits (rule 2, except °/%)
    /// - Unwanted space adjacent to full-width punctuation (rule 3)
    /// - Repeated full-width punctuation marks (rule 4)
    /// - Full-width digits that should be half-width (rule 5)
    pub(crate) fn scan_spacing(&self, em: &mut Emitter<'_>) {
        let text = em.text;
        let excluded = em.excluded;
        let issues = &mut *em.issues;

        if text.is_empty() {
            return;
        }

        // Sliding window: prev/curr/next chars with byte offsets. Avoids
        // materializing the full Vec<(usize, char)>.
        let mut iter = text.char_indices().peekable();
        let mut prev: Option<(usize, char)> = None;
        // Track consecutive identical punct run length for rule 4.
        let mut same_punct_run: usize = 0;

        while let Some((offset, ch)) = iter.next() {
            let ch_len = ch.len_utf8();
            let excluded_ch = is_excluded(offset, offset + ch_len, excluded);

            // Update punct run tracking (independent of exclusion).
            if !excluded_ch && is_fullwidth_punct(ch) {
                if prev.is_some_and(|(_, pc)| pc == ch) {
                    same_punct_run += 1;
                } else {
                    same_punct_run = 0;
                }
            } else {
                same_punct_run = 0;
            }

            if !excluded_ch {
                check_char(
                    &mut iter,
                    offset,
                    ch,
                    prev,
                    same_punct_run,
                    excluded,
                    issues,
                );
            }

            // Single update point for prev, which is why the per-character
            // checks live in a function rather than a continue statement.
            prev = Some((offset, ch));
        }
    }
}

/// Apply rules 1 to 5 at one character. Rules 4 and 5 are mutually exclusive
/// with the adjacency rules, so each match returns.
fn check_char(
    iter: &mut Peekable<CharIndices<'_>>,
    offset: usize,
    ch: char,
    prev: Option<(usize, char)>,
    same_punct_run: usize,
    excluded: &[ByteRange],
    issues: &mut Vec<Issue>,
) {
    // Rule 5: full-width digits should be half-width.
    if is_fullwidth_digit(ch) {
        let hw = fullwidth_to_halfwidth_digit(ch);
        issues.push(punct_issue_sev(
            offset,
            &ch.to_string(),
            &hw.to_string(),
            "數字應使用半形字元",
            Severity::Warning,
        ));
        return;
    }

    // Rule 4: repeated full-width punctuation. Paired punct (…… and ——) is
    // allowed at exactly two, so it only trips from the third onward.
    if is_fullwidth_punct(ch) && same_punct_run > 0 {
        let limit = if is_paired_punct(ch) { 2 } else { 1 };
        if same_punct_run >= limit {
            issues.push(punct_issue_sev(
                offset,
                &ch.to_string(),
                "",
                "不重複使用標點符號",
                Severity::Warning,
            ));
        }
        return;
    }

    // Rules 1 to 3 all compare against the following character.
    let Some(&(next_offset, next_ch)) = iter.peek() else {
        return;
    };
    if is_excluded(next_offset, next_offset + next_ch.len_utf8(), excluded) {
        return;
    }

    // Rule 1: CJK immediately adjacent to Latin.
    if (is_cjk_ideograph(ch) && next_ch.is_ascii_alphabetic())
        || (ch.is_ascii_alphabetic() && is_cjk_ideograph(next_ch))
    {
        issues.push(missing_space_issue(
            offset + ch.len_utf8(),
            "中英文之間需要增加空格",
        ));
    }

    // Rule 2: CJK immediately adjacent to a digit.
    if (is_cjk_ideograph(ch) && next_ch.is_ascii_digit())
        || (ch.is_ascii_digit() && is_cjk_ideograph(next_ch))
    {
        issues.push(missing_space_issue(
            offset + ch.len_utf8(),
            "中文與數字之間需要增加空格",
        ));
    }

    // Rule 3: no space on either side of full-width punctuation.
    check_space_before_punct(iter, offset, ch, prev, issues);
    check_space_after_punct(iter, next_offset, ch, next_ch, issues);
}

/// Rule 3, leading half: a space run between content and full-width punct.
fn check_space_before_punct(
    iter: &Peekable<CharIndices<'_>>,
    offset: usize,
    ch: char,
    prev: Option<(usize, char)>,
    issues: &mut Vec<Issue>,
) {
    if ch != ' ' {
        return;
    }
    let Some((_, content_ch)) = prev.filter(|&(_, pc)| pc != ' ') else {
        return;
    };
    if !is_cjk_ideograph(content_ch) && !content_ch.is_ascii_alphanumeric() {
        return;
    }
    let Some((punct_offset, punct_ch)) = next_non_space(iter.clone()) else {
        return;
    };
    if !is_fullwidth_punct(punct_ch) {
        return;
    }
    issues.push(unwanted_space_issue(
        offset,
        punct_offset - offset,
        "全形標點與其他字元之間不加空格",
    ));
}

/// Rule 3, trailing half: a space run between full-width punct and content.
fn check_space_after_punct(
    iter: &Peekable<CharIndices<'_>>,
    next_offset: usize,
    ch: char,
    next_ch: char,
    issues: &mut Vec<Issue>,
) {
    if !is_fullwidth_punct(ch) || next_ch != ' ' {
        return;
    }
    // Skip the space already peeked, then find what the run leads to.
    let mut fwd = iter.clone();
    fwd.next();
    let Some((content_offset, content_ch)) = next_non_space(fwd) else {
        return;
    };
    if !is_cjk_ideograph(content_ch) && !content_ch.is_ascii_alphanumeric() {
        return;
    }
    issues.push(unwanted_space_issue(
        next_offset,
        content_offset - next_offset,
        "全形標點與其他字元之間不加空格",
    ));
}

/// First non-space character at or after `fwd`'s current position.
fn next_non_space(fwd: Peekable<CharIndices<'_>>) -> Option<(usize, char)> {
    fwd.into_iter().find(|&(_, ch)| ch != ' ')
}

/// True for punctuation that is legitimately used in pairs (…… and ——).
fn is_paired_punct(ch: char) -> bool {
    ch == '\u{2026}' || ch == '\u{2014}'
}

#[cfg(test)]
#[path = "../../../tests/unit/engine/scan/spacing/tests.rs"]
mod tests;
