use super::*;
use crate::rules::ruleset::{Issue, IssueType, Severity};

/// Positions for a batch of ascending offsets, which is the only shape
/// this index is ever asked for: the scanner hands it a document's issues
/// at once. Written against the batch call rather than a per-offset one
/// so the column arithmetic is tested where it actually runs, including
/// the cursor that carries a partial count between issues on one line.
fn positions(text: &str, offsets: &[usize], encoding: ColumnEncoding) -> Vec<(usize, usize)> {
    let mut issues: Vec<Issue> = offsets
        .iter()
        .map(|&offset| {
            Issue::new(
                offset,
                0,
                "",
                Vec::new(),
                IssueType::Grammar,
                Severity::Warning,
            )
        })
        .collect();
    LineIndex::new(text).fill_line_col_sorted(&mut issues, encoding);
    issues.iter().map(|issue| (issue.line, issue.col)).collect()
}

#[test]
fn single_line_ascii() {
    assert_eq!(
        positions("hello world", &[0, 5, 11], ColumnEncoding::Utf16),
        [(1, 1), (1, 6), (1, 12)]
    );
}

#[test]
fn multi_line_ascii() {
    // 'a', the newline ending line 1, 'd', and 'g'.
    assert_eq!(
        positions("abc\ndef\nghi", &[0, 3, 4, 8], ColumnEncoding::Utf16),
        [(1, 1), (1, 4), (2, 1), (3, 1)]
    );
}

#[test]
fn cjk_columns_utf16() {
    // CJK chars are in the BMP: one UTF-16 code unit each, three UTF-8 bytes
    // each, so the columns advance by one where the offsets advance by three.
    assert_eq!(
        positions("你好世界", &[0, 3, 6, 9], ColumnEncoding::Utf16),
        [(1, 1), (1, 2), (1, 3), (1, 4)]
    );
}

#[test]
fn emoji_utf16_surrogate_pair() {
    // U+1F600 is outside the BMP: four UTF-8 bytes, two UTF-16 code units, so
    // the char after it sits at column 4 rather than 3.
    assert_eq!(
        positions("a😀b", &[0, 1, 5], ColumnEncoding::Utf16),
        [(1, 1), (1, 2), (1, 4)]
    );
}

#[test]
fn emoji_utf32() {
    // The same offsets counted in scalar values, where the emoji is one.
    assert_eq!(
        positions("a😀b", &[0, 1, 5], ColumnEncoding::Utf32),
        [(1, 1), (1, 2), (1, 3)]
    );
}

#[test]
fn mixed_ascii_cjk_multiline() {
    // Line 1 is Hello 你好 with 你 at byte 6 and 好 at byte 9; line 2 starts at
    // byte 13, after the newline at 12.
    assert_eq!(
        positions("Hello 你好\nWorld 世界", &[6, 9, 13], ColumnEncoding::Utf16),
        [(1, 7), (1, 8), (2, 1)]
    );
}

#[test]
fn offset_at_end() {
    assert_eq!(positions("abc", &[3], ColumnEncoding::Utf16), [(1, 4)]);
}

#[test]
fn empty_text() {
    assert_eq!(positions("", &[0], ColumnEncoding::Utf16), [(1, 1)]);
}

#[test]
fn a_line_start_resets_the_running_column() {
    // The cursor carries a partial column count from one issue to the next, and
    // only while both sit on the same line. A batch that crosses a line
    // boundary and then reports again on the new line is what catches a reset
    // that did not happen.
    assert_eq!(
        positions("你好\n世界", &[0, 3, 7, 10], ColumnEncoding::Utf16),
        [(1, 1), (1, 2), (2, 1), (2, 2)]
    );
}
