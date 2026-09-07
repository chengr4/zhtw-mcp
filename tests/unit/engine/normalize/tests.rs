use super::*;

/// A range mapped forward has to cover every normalized byte that carries
/// any part of the original range, or an exclusion leaks.
fn covers(input: &str, start: usize, end: usize) -> (usize, usize) {
    let norm = normalize_nfc(input);
    map_range_forward(&norm.offset_map, start, end).expect("range maps to something")
}

#[test]
fn forward_mapping_is_identity_when_already_nfc() {
    let input = "Hello 你好世界";
    let norm = normalize_nfc(input);
    assert!(norm.offset_map.is_empty());
    assert_eq!(map_range_forward(&norm.offset_map, 2, 8), Some((2, 8)));
}

#[test]
fn forward_mapping_rejects_a_range_starting_at_or_past_the_end() {
    // The map's last entry is the sentinel for the end-of-string position. A
    // range opening there names no byte, and mapping it produced an end one
    // past the normalized text, which a caller would slice with.
    let input = "cafe\u{301}";
    let norm = normalize_nfc(input);
    assert!(norm.text.len() < input.len(), "fixture must compose");
    assert_eq!(
        map_range_forward(&norm.offset_map, input.len(), input.len() + 1),
        None
    );
    assert_eq!(
        map_range_forward(&norm.offset_map, input.len() + 5, input.len() + 9),
        None
    );

    // An end past the text is held to it rather than running off the map.
    let (start, end) = map_range_forward(&norm.offset_map, 0, input.len() + 4)
        .expect("a range that starts inside the text maps to something");
    assert!(
        end <= norm.text.len(),
        "mapped end left the normalized text"
    );
    assert_eq!(&norm.text[start..end], norm.text.as_ref());
}

#[test]
fn forward_mapping_rejects_an_empty_or_inverted_range() {
    let norm = normalize_nfc("cafe\u{301}");
    assert_eq!(map_range_forward(&norm.offset_map, 3, 3), None);
    assert_eq!(map_range_forward(&norm.offset_map, 5, 2), None);
}

#[test]
fn forward_mapping_covers_the_bytes_after_a_composition() {
    // "cafe" plus a combining acute: five chars in, four out, so every offset
    // after the mark moves by two bytes.
    let input = "cafe\u{301} 用";
    let tail = input.find('用').expect("fixture contains the character");
    let (start, end) = covers(input, tail, input.len());
    let norm = normalize_nfc(input);
    assert_eq!(&norm.text[start..end], "用");
}

#[test]
fn forward_mapping_keeps_a_range_that_opens_on_a_combining_mark() {
    // The mark composes into the base before it, so no normalized byte records
    // the mark's own offset. Rounding the start inward would drop the composed
    // character out of the range entirely.
    let input = "e\u{301}";
    let norm = normalize_nfc(input);
    let (start, end) = map_range_forward(&norm.offset_map, 1, input.len())
        .expect("a range over the mark maps to something");
    assert_eq!(&norm.text[start..end], "\u{e9}");
}

#[test]
fn forward_mapping_of_every_prefix_covers_what_it_should() {
    // Exhaustive over one string that composes, reorders and expands, so a
    // boundary rule that works only on the fixtures above fails here.
    let input = "a\u{301}b e\u{344}\u{316}c 用字";
    let norm = normalize_nfc(input);
    assert!(
        !norm.offset_map.is_empty(),
        "fixture must not be NFC already"
    );
    for start in 0..input.len() {
        for end in start + 1..=input.len() {
            let Some((s, e)) = map_range_forward(&norm.offset_map, start, end) else {
                continue;
            };
            assert!(
                s < e && e <= norm.text.len(),
                "{start}..{end} mapped to {s}..{e}"
            );

            // Every normalized byte whose recorded origin is inside the range
            // has to be inside the mapped range.
            for (i, &origin) in norm.offset_map.iter().enumerate().take(norm.text.len()) {
                if (start..end).contains(&origin) {
                    assert!(
                        (s..e).contains(&i),
                        "byte {i} (origin {origin}) escaped {start}..{end} -> {s}..{e}"
                    );
                }
            }
        }
    }
}

#[test]
fn already_nfc_identity() {
    let input = "Hello 你好世界";
    let norm = normalize_nfc(input);
    assert_eq!(&*norm.text, input);
    // Fast path: empty offset_map means identity mapping.
    assert!(norm.offset_map.is_empty());
    for i in 0..=input.len() {
        assert_eq!(map_offset(&norm.offset_map, i), i);
    }
}

#[test]
fn nfc_composed_vs_decomposed() {
    // U+0065 U+0301 (e + combining acute) -> U+00E9 (é precomposed).
    let decomposed = "e\u{0301}";
    let norm = normalize_nfc(decomposed);
    assert_eq!(norm.text, "\u{00E9}"); // NFC form: é
    assert_eq!(norm.text.len(), 2); // é is 2 UTF-8 bytes
                                    // The normalized é maps back to byte 0 (the
                                    // 'e' position).
    assert_eq!(map_offset(&norm.offset_map, 0), 0);
    // End sentinel maps to original end.
    assert_eq!(
        map_offset(&norm.offset_map, norm.text.len()),
        decomposed.len()
    );
}

#[test]
fn nfc_with_surrounding_text() {
    // "ae\u{0301}b" -> "aéb" after NFC.
    let input = "ae\u{0301}b";
    let norm = normalize_nfc(input);
    assert_eq!(norm.text, "a\u{00E9}b");
    // 'a' at norm byte 0 maps to orig byte 0.
    assert_eq!(map_offset(&norm.offset_map, 0), 0);
    // 'é' at norm byte 1 maps to orig byte 1 (the 'e').
    assert_eq!(map_offset(&norm.offset_map, 1), 1);

    // 'b' at norm byte 3 maps to orig byte 4 (after e + combining = 3 bytes).
    assert_eq!(map_offset(&norm.offset_map, 3), 4);
}

#[test]
fn cjk_text_unchanged() {
    let input = "繁體中文測試";
    let norm = normalize_nfc(input);
    assert_eq!(norm.text, input);
}

#[test]
fn mixed_content() {
    // Mix of ASCII, CJK, and precomposed chars - all already NFC.
    let input = "Hello 你好 café";
    let norm = normalize_nfc(input);
    assert_eq!(norm.text, input);
}

#[test]
fn map_offset_out_of_bounds() {
    // For NFC fast path, map_offset returns the input offset unchanged.
    let input = "abc";
    let norm = normalize_nfc(input);
    assert_eq!(map_offset(&norm.offset_map, 100), 100);
    // For non-NFC input, map_offset clamps to original length.
    let decomposed = "e\u{0301}";
    let norm2 = normalize_nfc(decomposed);
    assert_eq!(map_offset(&norm2.offset_map, 100), decomposed.len());
}

#[test]
fn empty_input() {
    let norm = normalize_nfc("");
    assert_eq!(&*norm.text, "");
    // Empty string is NFC, so fast path: empty offset_map.
    assert!(norm.offset_map.is_empty());
}

#[test]
fn stacked_combining_marks_offset() {
    // "a + U+0301 + U+0301" → NFC is "á + U+0301" (first mark absorbed). The
    // remaining U+0301 in NFC output must map to byte 3 in the original (the
    // second mark), not byte 1 (the first, absorbed mark).
    //
    // - Original bytes: a(0), U+0301(1-2), U+0301(3-4), 5 in total.
    // - NFC bytes: á(0-1), U+0301(2-3), 4 in total.
    let input = "a\u{0301}\u{0301}";
    assert_eq!(input.len(), 5); // a=1, U+0301=2, U+0301=2
    let norm = normalize_nfc(input);
    // NFC: á (U+00E9 = 2 bytes) + U+0301 (2 bytes)
    assert_eq!(norm.text.len(), 4);
    // The á at NFC byte 0 maps to orig byte 0 (the 'a').
    assert_eq!(map_offset(&norm.offset_map, 0), 0);
    // The remaining U+0301 at NFC byte 2 maps to orig byte 3 (second mark).
    assert_eq!(map_offset(&norm.offset_map, 2), 3);
}

// Two complete Hangul syllables do not compose, so they are separate segments.
// Coalescing them mapped every byte of the run to the offset of the first
// syllable.
#[test]
fn adjacent_hangul_syllables_keep_their_own_offsets() {
    let norm = normalize_nfc("각각각 e\u{0301}");
    assert_eq!(map_offset(&norm.offset_map, 0), 0);
    assert_eq!(map_offset(&norm.offset_map, 3), 3);
    assert_eq!(map_offset(&norm.offset_map, 6), 6);
}

// The pairs that do compose still must not be split: the walk sees source
// characters, so the trailing jamo follows the vowel, not the LV syllable the
// two of them will become.
#[test]
fn decomposed_hangul_jamo_still_compose() {
    let norm = normalize_nfc("\u{1100}\u{1161}\u{11A8} e\u{0301}");
    assert_eq!(norm.text, "\u{AC01} \u{00E9}");
}

#[test]
fn expanded_combining_mark_maps_to_its_source() {
    let input = "中\u{0344}b";
    let norm = normalize_nfc(input);
    assert_eq!(norm.text, "中\u{0308}\u{0301}b");
    assert_eq!(map_offset(&norm.offset_map, 3), 3);
    assert_eq!(map_offset(&norm.offset_map, 5), 3);
    assert_eq!(map_offset(&norm.offset_map, 7), 5);
}
