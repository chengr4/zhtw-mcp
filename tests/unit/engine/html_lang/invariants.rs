use super::*;

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

// The tag matcher reads whatever an author wrote, and a Markdown file about
// HTML is full of half-written tags. Three properties have to hold for any
// input: the walk terminates, it never indexes off a character boundary, and
// every range it reports is inside the text and non-empty. The fragments below
// are the shapes that drive its branches, including the ones that made earlier
// versions spin: an attribute with no value, a bare equals, an unterminated
// quote, and a lone angle bracket.
#[test]
fn the_tag_walk_holds_for_random_input() {
    const FRAGMENTS: &[&str] = &[
        "<",
        ">",
        "</",
        "/>",
        "<span",
        "<div",
        "<p",
        "<li",
        "<td",
        "<script",
        "<br",
        "lang=\"en\"",
        "lang='zh-TW'",
        "lang=",
        "lang",
        "lang=\"\"",
        "class=x",
        "=",
        "\"",
        "'",
        " ",
        "\n",
        "<!--",
        "-->",
        "<![CDATA[",
        "]]>",
        "中文",
        "a",
        "<span lang=\"en\">",
        "</span>",
        "</div>",
        "</script>",
        "<!DOCTYPE html>",
        "<?xml?>",
        "<中文>",
        "<a-b:c.d>",
    ];
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for case in 0..200_000u32 {
        let len = (rng.next() % 10) as usize;
        let text: String = (0..len).map(|_| rng.pick(FRAGMENTS)).collect();

        // Feeding the whole string is the worst case: real callers hand over
        // only the parser's HTML events, which are shorter.
        let mut tracker = LangScopes::new();
        tracker.feed(&text, 0);
        let ranges = tracker.finish(text.len());

        for r in &ranges {
            assert!(r.start < r.end, "case {case}: empty range in {text:?}");
            assert!(
                r.end <= text.len(),
                "case {case}: range past end in {text:?}"
            );

            // Slicing panics on a boundary the walk got wrong, which is the
            // failure a byte-oriented scanner over UTF-8 can produce.
            let _ = &text[r.start..r.end];
        }
        for pair in ranges.windows(2) {
            assert!(
                pair[0].end <= pair[1].start,
                "case {case}: overlapping ranges in {text:?}"
            );
        }
    }
}
