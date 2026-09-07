use super::*;
use unicode_normalization::UnicodeNormalization;

/// Deterministic xorshift, so a failure is reproducible from the seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[(self.next() % from.len() as u64) as usize]
    }
}

// The offset map is what every reported span is translated through, so a single
// wrong entry writes a fix over neighbouring text. Four properties have to hold
// for any input, and the alphabet below is chosen to exercise the cases the
// segment walk reasons about: composing pairs, a mark that expands into two
// (U+0344), canonical reordering across combining classes, Hangul jamo that
// compose across starters, and scripts whose marks the old block list missed.
#[test]
fn the_offset_map_holds_for_random_input() {
    const ALPHABET: &[char] = &[
        'a', 'e', '中', 'b', '\u{0301}', '\u{0300}', '\u{0308}', '\u{0344}', '\u{0315}',
        '\u{05B0}', '\u{0654}', '\u{093C}', '\u{094D}', '\u{0915}', '\u{1100}', '\u{1161}',
        '\u{11A8}', '\u{AC00}', 'ا', 'ب', ' ', '。',
    ];
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for case in 0..200_000u32 {
        let len = (rng.next() % 8) as usize;
        let input: String = (0..len).map(|_| rng.pick(ALPHABET)).collect();
        let norm = normalize_nfc(&input);

        let expected: String = input.nfc().collect();
        assert_eq!(&*norm.text, expected, "case {case}: {input:?}");

        if norm.offset_map.is_empty() {
            // Fast path: identity, only taken when the input is already NFC.
            assert_eq!(&*norm.text, input, "case {case}: {input:?}");
            continue;
        }

        assert_eq!(
            norm.offset_map.len(),
            norm.text.len() + 1,
            "case {case}: map length must cover every byte plus the sentinel: {input:?}"
        );
        assert!(
            norm.offset_map.windows(2).all(|w| w[0] <= w[1]),
            "case {case}: map must not run backwards: {input:?} -> {:?}",
            norm.offset_map
        );
        for (i, &origin) in norm.offset_map.iter().enumerate() {
            assert!(
                origin <= input.len() && input.is_char_boundary(origin),
                "case {case}: entry {i} = {origin} is not a char boundary in {input:?}"
            );
        }
    }
}
