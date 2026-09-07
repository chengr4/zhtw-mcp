use super::*;

#[test]
fn range_content_treats_a_straddling_char_as_excluded() {
    // A ByteRange that cuts a character in half is not something the builders
    // produce, but the type does not forbid it, and rounding the readable chunk
    // outwards would both read excluded text and step the walk backwards onto
    // bytes it had already skipped.
    let text = "\u{201c}\u{4e2d}\u{201d}";
    let excluded = [ByteRange { start: 4, end: 5 }];
    let mut next = 0;
    assert!(!range_content(text, 3, 6, &excluded, &mut next).cjk);
}

#[test]
fn range_content_reads_prose_beside_an_excluded_range() {
    let text = "\u{201c}ab\u{4e2d}\u{201d}";
    let excluded = [ByteRange { start: 3, end: 5 }];
    let mut next = 0;
    assert!(range_content(text, 3, 8, &excluded, &mut next).cjk);
}

#[test]
fn range_content_handles_an_empty_range_list() {
    let text = "\u{201c}\u{4e2d}\u{201d}";
    let mut next = 0;
    assert!(range_content(text, 3, 6, &[], &mut next).cjk);
    let mut next = 0;
    assert!(!range_content(text, 3, 3, &[], &mut next).cjk);
}
