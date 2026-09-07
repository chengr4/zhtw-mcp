// MMSEG word segmenter.
//
// Scope: word boundaries only, NOT a full Chinese NLP toolkit. No POS tagging,
// no parsing. Designed for heuristic analysis in dunhao detection dunhao
// detection, ambiguity resolution, and variant context awareness.
//
// Algorithm: MMSEG (Chih-Hao Tsai, 1996) 4-rule chunk scoring. At each
// position, generate candidate 3-word chunks (up to max_word_len chars each);
// score by 4 rules in order:
//   (1) max total matched characters in chunk
//   (2) max average word length (fewest total words)
//   (3) min variance of word lengths
//   (4) max sum of frequency weights of single-character words in chunk
// Select winning chunk's first word, advance, repeat. Complexity: O(n × L^3), L
// = max_word_len (typically ≤ 10), so O(n).
//
// Lexicon: built from spelling rule vocabulary (from+to terms), a general zh-TW
// prose vocabulary (~180 common words), and a curated stop-word list of common
// function words and particles. Freq weights: rule terms=1, general vocab=5,
// stop words=10.

use std::collections::HashSet;

use rustc_hash::FxHashMap;

// CharTrie: character-indexed trie for O(L) dict lookups

/// A node in the character trie.  Each node optionally stores a freq weight
/// (non-zero = terminal) and a map from the next character to child nodes.
#[derive(Default)]
struct TrieNode {
    /// Non-zero when this node marks a complete dictionary word.
    freq: u32,
    /// Whether this word is a rule 'from' term (excluded from boundary checks).
    is_rule_from: bool,

    // FxHashMap: char keys don't need SipHash's DoS resistance, and this map is
    // probed once per character on the scan hot path (~56% of spelling-only
    // time) as well as filled on every startup.
    children: FxHashMap<char, TrieNode>,
}

/// Character-indexed trie built from the segmenter dictionary.
///
/// Provides O(L) lookup per position by walking one trie edge per character,
/// eliminating per-substring hashing in bitmap construction and MMSEG probing.
struct CharTrie {
    root: FxHashMap<char, TrieNode>,
}

impl CharTrie {
    fn new() -> Self {
        Self {
            root: FxHashMap::default(),
        }
    }

    /// Insert a word with its freq weight and rule_from flag.
    /// Returns `true` if this is a new word (freq was 0 before).
    fn insert(&mut self, word: &str, freq: u32, is_rule_from: bool) -> bool {
        let mut chars = word.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return false,
        };
        let mut node = self.root.entry(first).or_default();
        for ch in chars {
            node = node.children.entry(ch).or_default();
        }
        let is_new = node.freq == 0;
        // For freq: keep the higher value (stop words override rule terms).
        if freq > node.freq {
            node.freq = freq;
        }
        if is_rule_from {
            node.is_rule_from = true;
        }
        is_new
    }

    /// Walk the trie from a position in the char array, yielding all matches.
    ///
    /// Calls `callback(char_len, freq, is_rule_from)` for each prefix match
    /// (length >= 1).  Stops early if the trie path dies.
    #[inline]
    fn walk_matches<F>(&self, chars: &[(usize, char)], pos: usize, mut callback: F)
    where
        F: FnMut(usize, u32, bool),
    {
        let n = chars.len();
        if pos >= n {
            return;
        }
        let first_ch = chars[pos].1;
        let Some(mut node) = self.root.get(&first_ch) else {
            return;
        };
        // Check single-char match.
        if node.freq > 0 {
            callback(1, node.freq, node.is_rule_from);
        }
        // Extend.
        for depth in 1.. {
            let idx = pos + depth;
            if idx >= n {
                break;
            }
            let ch = chars[idx].1;
            match node.children.get(&ch) {
                Some(child) => {
                    node = child;
                    if node.freq > 0 {
                        callback(depth + 1, node.freq, node.is_rule_from);
                    }
                }
                None => break,
            }
        }
    }

    /// Check if a single-char key exists in the trie root with freq > 0.
    #[inline]
    fn single_char_freq(&self, ch: char) -> u32 {
        match self.root.get(&ch) {
            Some(node) if node.freq > 0 => node.freq,
            _ => 0,
        }
    }

    /// Insert a word (higher freq wins), returning true if new.
    /// Also updates max_char_len if this word is longer than current max.
    fn insert_tracking(
        &mut self,
        word: &str,
        freq: u32,
        is_rule_from: bool,
        max_char_len: &mut usize,
    ) -> bool {
        let char_len = word.chars().count();
        if char_len > *max_char_len {
            *max_char_len = char_len;
        }
        self.insert(word, freq, is_rule_from)
    }

    /// Insert only if the word is not already in the trie (freq == 0).
    /// Returns true if the word was newly inserted.
    fn insert_if_absent(&mut self, word: &str, freq: u32, max_char_len: &mut usize) -> bool {
        if word.is_empty() {
            return false;
        }
        // Check if already present by walking the trie.
        let existing = self.get_freq_internal(word);
        if existing > 0 {
            return false;
        }
        let char_len = word.chars().count();
        if char_len > *max_char_len {
            *max_char_len = char_len;
        }
        self.insert(word, freq, false);
        true
    }

    /// Internal freq lookup (not gated by #[cfg(test)]).
    fn get_freq_internal(&self, word: &str) -> u32 {
        let mut chars = word.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return 0,
        };
        let Some(mut node) = self.root.get(&first) else {
            return 0;
        };
        for ch in chars {
            match node.children.get(&ch) {
                Some(child) => node = child,
                None => return 0,
            }
        }
        node.freq
    }

    /// Look up a word's freq weight.
    ///
    /// Returns `Some(freq)` if found, `None` otherwise.
    #[cfg(test)]
    fn get_freq(&self, word: &str) -> Option<u32> {
        let mut chars = word.chars();
        let first = chars.next()?;
        let mut node = self.root.get(&first)?;
        for ch in chars {
            node = node.children.get(&ch)?;
        }
        if node.freq > 0 {
            Some(node.freq)
        } else {
            None
        }
    }

    /// Whether a word exists in the trie (freq > 0).
    #[cfg(test)]
    fn contains(&self, word: &str) -> bool {
        self.get_freq(word).is_some()
    }
}

/// A lightweight MMSEG word segmenter.
///
/// The dictionary maps words to frequency weights: stop words get 10 (higher
/// morphemic freedom for Rule 4 tie-breaking), rule vocabulary gets 1.
pub struct Segmenter {
    /// Number of entries in the dictionary.  Computed at construction time
    /// by counting new insertions into the CharTrie.
    word_count: usize,
    /// Character trie built from dictionary entries for O(L) forward walks.
    trie: CharTrie,
    /// Maximum word length (in chars) across all dictionary entries.
    /// Computed at construction time so long entries (e.g. country names)
    /// are reachable without a hardcoded constant.
    /// Invariant: max_word_len <= MAX_WORD_LEN_LIMIT (enforced at
    /// construction).
    max_word_len: usize,
    /// Rule 'from' terms: cn-style patterns that the AC scanner is trying
    /// to detect.  Excluded from word-boundary straddle checks so that one
    /// rule's pattern doesn't suppress another rule's match.
    /// The trie also carries `is_rule_from` per-node for hot-path lookups;
    /// this field is retained for test assertions.
    #[allow(dead_code)]
    rule_from_terms: HashSet<String>,
}

/// A single token produced by segmentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The token text.
    pub text: String,
    /// Byte offset in the original input.
    pub offset: usize,
    /// Whether this token was found in the dictionary.
    pub in_dict: bool,
}

/// Pre-computed bitmap for word-boundary straddle resolution.
///
/// For each byte position, records whether any non-rule dictionary word
/// crosses it, and (for crossed positions) the minimum byte start of any
/// crossing word.  Both start and end boundary checks are answered
/// directly from the bitmap -- no per-hit segmenter call needed.
pub struct BoundaryBitmap {
    /// Bit-packed crossed flags: bit `pos` is set when some non-rule dict
    /// word straddles byte position `pos`.  Stored as Vec<u64> for 8x
    /// memory reduction vs Vec<bool> and better cache utilization.
    crossed: Vec<u64>,
    /// Number of byte positions covered (= text.len() + 1).
    len: usize,
    /// For crossed positions: minimum start byte of any crossing word.
    /// Used for exact end-boundary resolution without segmenter fallback.
    /// `u32::MAX` sentinel means "not crossed".
    min_cross_start: Vec<u32>,
}

impl BoundaryBitmap {
    /// An empty bitmap where all lookups return false.
    ///
    /// Used for short texts where building the bitmap is not worth the cost
    /// (the per-hit segmenter is called directly instead).
    pub fn empty() -> Self {
        Self {
            crossed: Vec::new(),
            len: 0,
            min_cross_start: Vec::new(),
        }
    }

    /// Whether the bitmap is empty (no precomputation done).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Test the crossed bit at position `pos`.
    #[inline]
    fn is_crossed(&self, pos: usize) -> bool {
        let word = pos / 64;
        let bit = pos % 64;
        word < self.crossed.len() && (self.crossed[word] & (1u64 << bit)) != 0
    }

    /// Whether any non-rule dictionary word crosses the start position.
    /// This is an exact answer -- no segmenter fallback needed.
    #[inline]
    pub fn start_straddles(&self, pos: usize) -> bool {
        pos < self.len && self.is_crossed(pos)
    }

    /// Whether any non-rule dictionary word crosses the end position
    /// with start <= match_start (i.e. the word starts outside the match).
    /// This is an exact answer -- no segmenter fallback needed.
    ///
    /// Uses min_cross_start: if the earliest-starting crossing word
    /// starts at or before match_start, at least one word violates the
    /// boundary from outside. If all crossing words start after
    /// match_start, they're inside the match (different segmentation).
    #[inline]
    pub fn end_straddles(&self, end: usize, match_start: usize) -> bool {
        if end >= self.len || !self.is_crossed(end) {
            return false;
        }
        (self.min_cross_start[end] as usize) <= match_start
    }
}

// Internal type: (char_length, freq_weight, in_dict).
type ChunkWord = (usize, u32, bool);

/// Hard limit on max_word_len, matching the stack buffer size in
/// word_straddles_boundary_inner.  Enforced at construction so release
/// builds cannot silently truncate boundary-straddle probes.
const MAX_WORD_LEN_LIMIT: usize = 32;

impl Segmenter {
    /// Build a segmenter from an iterator of dictionary words (all get freq=1).
    pub fn new(words: impl IntoIterator<Item = String>) -> Self {
        let mut trie = CharTrie::new();
        let mut word_count: usize = 0;
        let mut max_word_len: usize = 1;
        for word in words {
            if !word.is_empty() && trie.insert_tracking(&word, 1, false, &mut max_word_len) {
                word_count += 1;
            }
        }
        assert!(
            max_word_len <= MAX_WORD_LEN_LIMIT,
            "max_word_len ({max_word_len}) exceeds stack buffer limit ({MAX_WORD_LEN_LIMIT})"
        );
        Self {
            word_count,
            trie,
            max_word_len,
            rule_from_terms: HashSet::new(),
        }
    }

    /// Build a segmenter from spelling rule vocabulary.
    ///
    /// Extracts from, to, context_clues, and negative_context_clues
    /// entries from each rule (freq=1), then adds the curated stop-word list
    /// (freq=10, overriding any lower value from rules).
    pub fn from_rules(rules: &[crate::rules::ruleset::SpellingRule]) -> Self {
        let mut trie = CharTrie::new();
        let mut rule_from_terms: HashSet<String> = HashSet::new();
        let mut word_count: usize = 0;
        let mut max_word_len: usize = 1;

        // Helper: insert into trie, track word_count and max_word_len.
        let mut insert = |word: &str, freq: u32, is_rule_from: bool| {
            if !word.is_empty() && trie.insert_tracking(word, freq, is_rule_from, &mut max_word_len)
            {
                word_count += 1;
            }
        };

        // Extract terms from rules (from, to, and context_clues).
        for rule in rules {
            if !rule.disabled {
                rule_from_terms.insert(rule.from.clone());
                insert(&rule.from, 1, true);
                for to in &rule.to {
                    if !to.is_empty() {
                        insert(to, 1, false);
                    }
                }
                if let Some(clues) = &rule.context_clues {
                    for clue in clues {
                        insert(clue, 1, false);
                    }
                }
                if let Some(neg_clues) = &rule.negative_context_clues {
                    for clue in neg_clues {
                        insert(clue, 1, false);
                    }
                }
            }
        }

        // General vocabulary gets freq=5 (between rule terms and stop words).
        // Use insert_if_absent: don't overwrite rule terms (freq=1) that are
        // already present: preserves the original or_insert semantics.
        for w in GENERAL_VOCAB {
            if trie.insert_if_absent(w, 5, &mut max_word_len) {
                word_count += 1;
            }
        }

        // Stop words get freq=10 (favours common function words at Rule 4
        // tie-breaks). Use insert_tracking: higher freq always wins, so stop
        // words override freq=1 from rules even if already present.
        for w in STOP_WORDS {
            if trie.insert_tracking(w, 10, false, &mut max_word_len) {
                word_count += 1;
            }
        }

        assert!(
            max_word_len <= MAX_WORD_LEN_LIMIT,
            "max_word_len ({max_word_len}) exceeds stack buffer limit ({MAX_WORD_LEN_LIMIT})"
        );
        Self {
            word_count,
            trie,
            max_word_len,
            rule_from_terms,
        }
    }

    /// Return all candidate ChunkWord values that start at pos.
    ///
    /// Always includes the single character at pos:
    ///   - in_dict=true  if it is a dictionary entry (with its freq weight)
    ///   - in_dict=false if it is an OOV fallback (freq=0)
    ///
    /// Additionally includes every multi-character dict match starting at pos.
    /// Uses the character trie for a single forward walk instead of per-length
    /// HashMap probes.
    fn candidates_at(&self, chars: &[(usize, char)], pos: usize) -> Vec<ChunkWord> {
        let mut result = Vec::new();

        // Single-char candidate (always present).
        let single_freq = self.trie.single_char_freq(chars[pos].1);
        let in_dict = single_freq > 0;
        result.push((1, single_freq, in_dict));

        // Multi-char dictionary matches via trie walk.
        self.trie
            .walk_matches(chars, pos, |char_len, freq, _is_rule_from| {
                if char_len >= 2 {
                    result.push((char_len, freq, true));
                }
            });

        result
    }

    /// Select the best first-word token at pos using MMSEG 4-rule chunk
    /// scoring.
    ///
    /// Generates all candidate 3-word chunks (shorter at end-of-string) and
    /// returns the first word of the highest-scoring chunk.
    fn best_first_word(&self, chars: &[(usize, char)], pos: usize) -> ChunkWord {
        let n = chars.len();
        let w1_candidates = self.candidates_at(chars, pos);

        // Stack-allocated chunk: [words; 3] + length, avoids heap alloc per
        // comparison.
        let mut best: Option<([ChunkWord; 3], usize)> = None;

        for &w1 in &w1_candidates {
            let pos2 = pos + w1.0;

            if pos2 >= n {
                let chunk = ([w1, ZERO_WORD, ZERO_WORD], 1);
                if best.as_ref().is_none_or(|b| chunk_beats_arr(&chunk, b)) {
                    best = Some(chunk);
                }
                continue;
            }

            let w2_candidates = self.candidates_at(chars, pos2);

            for &w2 in &w2_candidates {
                let pos3 = pos2 + w2.0;

                if pos3 >= n {
                    let chunk = ([w1, w2, ZERO_WORD], 2);
                    if best.as_ref().is_none_or(|b| chunk_beats_arr(&chunk, b)) {
                        best = Some(chunk);
                    }
                    continue;
                }

                let w3_candidates = self.candidates_at(chars, pos3);

                for &w3 in &w3_candidates {
                    let chunk = ([w1, w2, w3], 3);
                    if best.as_ref().is_none_or(|b| chunk_beats_arr(&chunk, b)) {
                        best = Some(chunk);
                    }
                }
            }
        }

        best.expect("non-empty text always produces at least one chunk")
            .0[0]
    }

    /// Segment text into tokens using MMSEG 4-rule chunk scoring.
    ///
    /// Returns a vec of Token with byte offsets into the original text.
    pub fn segment(&self, text: &str) -> Vec<Token> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let n = chars.len();
        let mut tokens = Vec::new();
        let mut i = 0;

        while i < n {
            let (word_len, _freq, word_in_dict) = self.best_first_word(&chars, i);
            let start_byte = chars[i].0;
            let end_idx = i + word_len;
            let end_byte = if end_idx < n {
                chars[end_idx].0
            } else {
                text.len()
            };
            tokens.push(Token {
                text: text[start_byte..end_byte].to_string(),
                offset: start_byte,
                in_dict: word_in_dict,
            });
            i += word_len;
        }

        tokens
    }

    /// Count the number of "words" (multi-char dictionary tokens) in the
    /// segmented text. Useful for dunhao heuristic (short items = 1-3 words).
    pub fn word_count(&self, text: &str) -> usize {
        self.segment(text)
            .iter()
            .filter(|t| t.in_dict && t.text.chars().count() > 1)
            .count()
    }

    /// Check if any of the given clue words appear in the segmented text
    /// as dictionary-matched tokens, or as character-aligned substrings of
    /// dictionary-matched tokens.
    ///
    /// The substring check handles "clue absorption": when MMSEG Rule 1
    /// prefers a longer token (e.g. "下拉菜單") that contains a clue word
    /// ("下拉") as a prefix/infix/suffix, the clue never surfaces as a
    /// standalone token.  Checking substrings at character boundaries
    /// recovers these absorbed clues.
    pub fn has_context_clue(&self, text: &str, clues: &[&str]) -> bool {
        let tokens = self.segment(text);
        clues.iter().any(|clue| {
            tokens
                .iter()
                .any(|t| t.in_dict && (t.text == *clue || token_contains_clue(&t.text, clue)))
        })
    }

    /// Count how many distinct clue words appear in the segmented text as
    /// dictionary-matched tokens or as character-aligned substrings of
    /// dictionary-matched tokens.  Segments text once, then checks all
    /// clues against the token set.  Duplicate entries in clues are counted
    /// only once each (distinct-word semantics).
    pub fn count_context_clues(&self, text: &str, clues: &[&str]) -> usize {
        let tokens = self.segment(text);
        let dict_tokens: Vec<&str> = tokens
            .iter()
            .filter(|t| t.in_dict)
            .map(|t| t.text.as_str())
            .collect();
        let mut seen = std::collections::HashSet::new();
        clues
            .iter()
            .filter(|&&c| {
                seen.insert(c)
                    && dict_tokens
                        .iter()
                        .any(|&tok| tok == c || token_contains_clue(tok, c))
            })
            .count()
    }

    /// Number of entries in the dictionary.
    pub fn dict_size(&self) -> usize {
        self.word_count
    }

    /// Check if a known dictionary word straddles the given byte boundary.
    ///
    /// Returns `true` if there exists a dictionary entry of length >= 2 chars
    /// that starts before `boundary` and ends after it.  This catches false
    /// AC matches where the pattern spans two distinct words: e.g. "積分"
    /// inside "累積分佈" (累積 + 分佈).
    ///
    /// Rule 'from' terms are excluded: they are cn-style patterns, not
    /// legitimate word boundaries, so one rule's pattern must not suppress
    /// another rule's match (e.g. '文件內容' must not suppress '讀文件').
    ///
    /// Non-CJK characters act as natural word boundaries.
    ///
    /// Cost: O(L^2) dictionary lookups where L = max_word_len (typically <=
    /// 10).
    pub fn word_straddles_boundary(&self, text: &str, boundary: usize) -> bool {
        self.word_straddles_boundary_with_limit(text, boundary, None)
    }

    /// Like `word_straddles_boundary`, but only considers dictionary words
    /// whose start position is strictly before `no_walk_after`.  When checking
    /// the *end* boundary of a match, pass `Some(match_start)` as
    /// `no_walk_after`
    /// so that dictionary words beginning inside the match (e.g. "目的"
    /// overlapping the end of "項目") are ignored: they represent a
    /// different segmentation, not a boundary violation.
    ///
    /// Pass `None` to disable the limit (equivalent to
    /// `word_straddles_boundary`).
    pub fn word_straddles_boundary_with_limit(
        &self,
        text: &str,
        boundary: usize,
        no_walk_after: Option<usize>,
    ) -> bool {
        use super::scan::is_cjk_ideograph;

        if boundary > text.len() || !text.is_char_boundary(boundary) {
            return false;
        }

        // Fast guard: if the char immediately before the boundary is not CJK,
        // the backward walk will produce zero start positions and the function
        // is guaranteed to return false. Avoids the loop setup cost for
        // boundaries at ASCII, whitespace, or punctuation edges.
        if boundary > 0 {
            let prev = text.floor_char_boundary(boundary - 1);
            if let Some(ch) = text[prev..].chars().next() {
                if !is_cjk_ideograph(ch) {
                    return false;
                }
            }
        } else {
            return false;
        }

        // Similarly, if the char at the boundary (the first char after it) is
        // not CJK, no dictionary word can extend across it.
        if boundary < text.len() {
            if let Some(ch) = text[boundary..].chars().next() {
                if !is_cjk_ideograph(ch) {
                    return false;
                }
            }
        } else {
            return false;
        }

        self.word_straddles_boundary_inner(text, boundary, no_walk_after)
    }

    /// Check whether a known dictionary word straddles either edge of the
    /// byte range [start, end).  Combined check avoids two separate function
    /// calls for the same match span.  For the end boundary, dictionary words
    /// starting inside the match are ignored (see
    /// `word_straddles_boundary_with_limit`).
    pub fn match_straddles_word_boundary(&self, text: &str, start: usize, end: usize) -> bool {
        self.word_straddles_boundary(text, start)
            || self.word_straddles_boundary_with_limit(text, end, Some(start))
    }

    /// Pre-compute a [`BoundaryBitmap`] with exact straddle answers.
    pub fn build_boundary_bitmap(&self, text: &str) -> BoundaryBitmap {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        self.build_boundary_bitmap_from_chars(text, &chars)
    }

    /// Like [`build_boundary_bitmap`] but reuses a pre-collected char index
    /// to avoid a redundant `char_indices()` pass when the caller already has
    /// it.
    ///
    /// Uses the character trie for O(L) forward walks instead of per-substring
    /// HashMap probes, eliminating the dominant bottleneck (56% of
    /// spelling_only).
    pub fn build_boundary_bitmap_from_chars(
        &self,
        text: &str,
        chars: &[(usize, char)],
    ) -> BoundaryBitmap {
        use super::scan::is_cjk_ideograph;

        debug_assert!(
            text.len() <= u32::MAX as usize,
            "BoundaryBitmap uses u32 for byte positions; text exceeds 4GB"
        );
        let cap = text.len() + 1;
        let words = cap.div_ceil(64);
        let mut crossed = vec![0u64; words];
        let mut min_cross_start = vec![u32::MAX; cap];

        let n = chars.len();

        for i in 0..n {
            let (start_byte, ch) = chars[i];
            if !is_cjk_ideograph(ch) {
                continue;
            }

            // Walk the trie from position i, collecting all multi-char
            // non-rule-from dict matches. One trie descent replaces up to
            // max_word_len HashMap probes per position.
            self.trie
                .walk_matches(chars, i, |char_len, _freq, is_rule_from| {
                    if char_len < 2 || is_rule_from {
                        return;
                    }
                    let end_idx = i + char_len;
                    let sb = start_byte as u32;
                    for j in (i + 1)..end_idx.min(n) {
                        if !is_cjk_ideograph(chars[j - 1].1) || !is_cjk_ideograph(chars[j].1) {
                            continue;
                        }
                        let pos = chars[j].0;
                        let w = pos / 64;
                        crossed[w] |= 1u64 << (pos % 64);
                        if sb < min_cross_start[pos] {
                            min_cross_start[pos] = sb;
                        }
                    }
                });
        }

        BoundaryBitmap {
            crossed,
            len: cap,
            min_cross_start,
        }
    }

    /// Inner implementation of boundary straddling check, called after
    /// the fast CJK guard has confirmed both sides are CJK.
    ///
    /// `no_walk_after`: if `Some(offset)`, skip candidate start positions at
    /// or after this byte offset.  Used when checking the end boundary of a
    /// match to ignore dictionary words starting inside the match span.
    fn word_straddles_boundary_inner(
        &self,
        text: &str,
        boundary: usize,
        no_walk_after: Option<usize>,
    ) -> bool {
        use super::scan::is_cjk_ideograph;

        let max_back = self.max_word_len.saturating_sub(1);

        // Stack buffer sized to MAX_WORD_LEN_LIMIT (enforced at construction).
        const BUF: usize = MAX_WORD_LEN_LIMIT;
        let mut starts = [(0usize, 0usize); BUF];
        let mut n_starts = 0;
        let mut pos = boundary;
        for chars_before in 1..=max_back.min(starts.len()) {
            if pos == 0 {
                break;
            }
            pos = text.floor_char_boundary(pos - 1);

            // Skip start positions strictly inside the match span: words
            // starting there are not external boundary violations. Still
            // consider a candidate that starts exactly at the match start,
            // because a longer dictionary word may extend past the right edge.
            if let Some(limit) = no_walk_after {
                if pos > limit {
                    continue;
                }
            }

            // pos is strictly below the boundary by the walk above, so the
            // slice is non-empty. Spelled as a match rather than an unwrap so
            // the invariant is checked where it is relied on, the way the probe
            // loop below tests bpos against the length.
            let Some(ch) = text[pos..].chars().next() else {
                break;
            };
            if !is_cjk_ideograph(ch) {
                break;
            }
            starts[n_starts] = (pos, chars_before);
            n_starts += 1;
        }

        // For each start, walk the trie forward past the boundary looking for
        // non-rule-from dict words that straddle it. (max_word_len <= BUF is
        // enforced at construction via assert.)
        for &(start, chars_before) in &starts[..n_starts] {
            // Build a char array from start position forward. Preserve the old
            // semantics: once probing steps past the boundary into a non-CJK
            // character, stop before including it in the probe.
            let mut probe: [(usize, char); BUF] = [(0, '\0'); BUF];
            let mut plen = 0;
            let mut bpos = start;
            while bpos < text.len() && plen < self.max_word_len.min(BUF) {
                let ch = text[bpos..].chars().next().unwrap();
                // Non-CJK past the boundary terminates (matches old behavior).
                if plen > chars_before && !is_cjk_ideograph(ch) {
                    break;
                }
                probe[plen] = (bpos, ch);
                plen += 1;
                bpos += ch.len_utf8();
            }
            // Walk trie on this probe array.
            let mut found = false;
            self.trie
                .walk_matches(&probe[..plen], 0, |char_len, _freq, is_rule_from| {
                    if found || is_rule_from || char_len <= chars_before {
                        return;
                    }

                    // Word of char_len starting at 'start' extends past
                    // boundary.
                    found = true;
                });
            if found {
                return true;
            }
        }

        false
    }
}

/// Check if clue is a character-aligned substring of token, but not equal
/// to the full token (the caller handles exact matches separately).
///
/// "Character-aligned" means the clue starts and ends at a char boundary within
/// the token.  For CJK text every char is a boundary, but this also works for
/// mixed scripts.  The clue must be non-empty and strictly shorter than the
/// token.
pub(crate) fn token_contains_clue(token: &str, clue: &str) -> bool {
    if clue.is_empty() || clue.len() >= token.len() {
        return false;
    }
    // Walk char boundaries in the token to find substring matches.
    let clue_bytes = clue.as_bytes();
    let token_bytes = token.as_bytes();
    for (byte_offset, _) in token.char_indices() {
        if byte_offset + clue.len() > token.len() {
            break;
        }
        if &token_bytes[byte_offset..byte_offset + clue.len()] == clue_bytes {
            // Verify the match ends at a char boundary.
            let end = byte_offset + clue.len();
            if token.is_char_boundary(end) {
                return true;
            }
        }
    }
    false
}

const ZERO_WORD: ChunkWord = (0, 0, false);

/// Returns true if chunk a scores strictly better than chunk b (stack-array
/// variant).
fn chunk_beats_arr(a: &([ChunkWord; 3], usize), b: &([ChunkWord; 3], usize)) -> bool {
    compare_chunks(&a.0[..a.1], &b.0[..b.1]) == std::cmp::Ordering::Greater
}

/// Compare two chunks under MMSEG 4-rule scoring (returns Ordering for a vs b).
///
/// Rules applied in order (first non-tie decides):
///   1. Max total chars in chunk.
///   2. Max average word length (= min word count when totals are equal).
///   3. Min variance of word lengths (scaled to integers; avoids floats).
///   4. Max sum of freq weights of single-character words in chunk.
///
/// Deterministic tiebreaker after Rule 4: longer first word (leftmost-longest).
fn compare_chunks(a: &[ChunkWord], b: &[ChunkWord]) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    // Rule 1: max total chars.
    let a_total: usize = a.iter().map(|&(l, _, _)| l).sum();
    let b_total: usize = b.iter().map(|&(l, _, _)| l).sum();
    match a_total.cmp(&b_total) {
        Ordering::Equal => {}
        ord => return ord,
    }

    // Rule 2: max average word length. With equal totals: higher average ⟺
    // fewer words.
    let a_count = a.len();
    let b_count = b.len();

    // Fewer words in a → a wins. b_count.cmp(&a_count) is Greater when b has
    // more words, i.e. a has fewer words, meaning a wins (return Greater).
    match b_count.cmp(&a_count) {
        Ordering::Equal => {}
        ord => return ord,
    }

    // Rule 3: min variance of word lengths. Totals and counts are equal here,
    // so the mean is identical. Use Σ (li × count - total)² as an integer proxy
    // for variance × count².
    let a_var: i64 = a
        .iter()
        .map(|&(l, _, _)| {
            let d = l as i64 * a_count as i64 - a_total as i64;
            d * d
        })
        .sum();
    let b_var: i64 = b
        .iter()
        .map(|&(l, _, _)| {
            let d = l as i64 * b_count as i64 - b_total as i64;
            d * d
        })
        .sum();

    // Lower variance wins. b_var.cmp(&a_var) is Greater when b_var > a_var,
    // meaning a has lower variance, meaning a wins (return Greater).
    match b_var.cmp(&a_var) {
        Ordering::Equal => {}
        ord => return ord,
    }

    // Rule 4: max sum of freq weights of single-character words.
    let a_sf: u32 = a
        .iter()
        .filter(|&&(l, _, _)| l == 1)
        .map(|&(_, f, _)| f)
        .sum();
    let b_sf: u32 = b
        .iter()
        .filter(|&&(l, _, _)| l == 1)
        .map(|&(_, f, _)| f)
        .sum();
    match a_sf.cmp(&b_sf) {
        Ordering::Equal => {}
        ord => return ord,
    }

    // Deterministic tiebreaker: leftmost-longest (prefer longer first word).
    a[0].0.cmp(&b[0].0)
}

/// General zh-TW vocabulary supplement for natural prose segmentation.
///
/// Fills gaps where the rule dict (~1500 terms) + stop words (~100) lack
/// common prose words, forcing single-char fallback and degrading context
/// clue recall.  Categories: abstract nouns, time/location words, common
/// verbs, adjectives, and connectives not already in STOP_WORDS.
/// Freq=5 in from_rules() (between rule terms at 1 and stop words at 10).
static GENERAL_VOCAB: &[&str] = &[
    // Abstract nouns
    "概念", "方式", "過程", "關係", "部分", "功能", "內容", "意思", "目的", "能力", "經驗", "影響",
    "效果", "需求", "條件", "原因", "原則", "基礎", "標準", "範圍", "程度", "價值", "意義", "特點",
    "優勢", "優點", "缺點", "特性", "規則", "機制", "角色", "領域", "層面", "趨勢", "因素", "行為",
    "狀態", "現象", "事件", "資源", "環境", "結構", "活動", "理論", "實踐", "策略", "方案", "目標",
    "任務", "責任", "權利", "義務", "制度", "組織", "範例", // Time words
    "目前", "之後", "期間", "之前", "當時", "現在", "以前", "以後", "未來", "同時", "隨時", "平時",
    "近年", "長期", "短期", "階段", "時期", // Location / relational words
    "之中", "之外", "其中", "之間", "以內", "以外", "附近", "周圍", "上方", "下方", "左右", "前方",
    "後方", "內部", "外部", // Common verbs (not in STOP_WORDS)
    "提供", "包含", "支援", "處理", "進行", "開始", "完成", "建立", "設定", "選擇", "表示", "認為",
    "發現", "決定", "解決", "產生", "實現", "利用", "管理", "保持", "改變", "增加", "減少", "達到",
    "獲得", "接受", "執行", "分析", "研究", "討論", "參與", "考慮", "存在", "屬於", "成為", "需要",
    "希望", "相信", "了解", "注意", "準備", "嘗試", "避免", "發展", "設計", "測試", "定義", "描述",
    "比較", "適合", "允許", "維護", "確認", "推動",
    // Adjectives / degree adverbs (not in STOP_WORDS)
    "主要", "重要", "基本", "一般", "相關", "不同", "具體", "特別", "必須", "可能", "通常", "經常",
    "其實", "逐漸", "幾乎", "相當", "確實", "顯然", "至少", "大約", "往往", "甚至", "正確", "適當",
    "完整", "足夠", "明確", "有效", "直接", "自動", "預期",
    // Connectives / discourse markers (not in STOP_WORDS)
    "例如", "另外", "此外", "然而", "總之", "因此", "根據", "透過", "對於", "關於", "隨著", "除了",
    "包括", "針對", "藉由", "依據", "即使", "無論", "否則", "同樣", "尤其", "反而", "首先", "其次",
    "最後", "進而", "從而",
    // Academic / technical prose (word-boundary disambiguation)
    "累積", "引導", "分佈", "序列", "函數", "變數", "模型", "估計", "觀測", "假設", "推導", "證明",
    "收斂", "機率", "隨機", "樣本", "頻率", "密度", "偏差", "變異", "差分", "形式", "排程",
    // Party and role nouns ending in 方, which abut 差異/差距/差別 and make
    // short rules such as 方差 fire across the boundary.
    "對方", "雙方", "官方", "我方", "資方", "勞方", "借方", "貸方", "校方", "院方", "軍方", "警方",
    // Verbs ending in 導 or 制, which abut 彈性/彈劾 and 導入/導致/導向.
    "輔導", "領導", "指導", "報導", "主導", "倡導", "教導", "疏導", "強制", "抑制", "管制", "限制",
    "控制",
    // Other high-frequency words that cross the START of a short rule term, the
    // only side the straddle check can block from: 播報 covers the 報 of 報文,
    // 分組 the 組 of 組播. A word that merely follows the term cannot block it,
    // so the cases where the term comes first are handled by rule exceptions.
    "小組", "群組", "情報", "通報", "回報", "簡報", "申報", "習慣", "部門", "專門", "播報",
    // Verbs ending in 坐, which abut 標高/標語/標示/標榜 and would otherwise
    // let the 坐標 rule fire across the boundary. Only verbs whose first
    // character rarely ends another word: 對坐 would break 針對坐標, 端坐 would
    // break 終端坐標, and 圍坐 would break 範圍坐標.
    "乘坐", "靜坐", "打坐", "就坐", "久坐", "危坐",
    // Left-hand words that abut the other two-character rules across a
    // boundary: 不停|靠近, 馬匹|配種, 平方|差, 督導|彈藥, 官網|格式, 海報|文案,
    // 分組|播送, 遮掩|碼頭.  The straddle check can only block from the left,
    // so this is where these belong rather than in per-rule exceptions.
    "不停", "馬匹", "平方", "督導", "官網", "海報", "分組", "遮掩",
    // Words ending in 外, which abut the rules 外設/外置/外鍵. Without these
    // 額外設定 rewrites to 額硬體周邊定 and 海外設廠 to 海硬體周邊廠. 另外 and
    // 此外 already block the straddle from the connective list above.
    "額外", "意外", "格外", "海外", "國外", "對外", "除外", "例外", "課外", "戶外", "室外", "郊外",
    "中外", "局外", "場外", "分外",
    // Words ending in 標, which abut 標清/標量/標誌著, so 達標清單 rewrote to
    // 達標準畫質單 and 投標量 to 投純量.
    //
    // A blocker has to be in the trie and must not itself be a rule "from"
    // term, because the straddle probe skips those: 目標 blocks from the list
    // above, 指標 and 座標 block as rule "to" terms, and the rest are here.
    "達標", "投標", "招標", "得標", "流標", "中標", "路標", "音標", "浮標", "商標", "錦標",
    // Words ending in 此, which abut 此外: 如此外向 became 如向, 彼此外貌
    // became 彼貌, 由此外推 became 由推. 因此 already blocks it from the
    // connective list above, which is why that one case looked fine.
    "如此", "彼此", "對此", "由此", "除此", "就此", "至此", "為此", "據此", "藉此", "從此", "於此",
    "特此", "在此", "自此",
    // Words ending in 源, which abut 源碼/源文件/源代碼. 來源文件 became
    // 來原始檔 and 電源文件 became 電原始檔.
    "來源", "電源", "水源", "光源", "財源", "貨源", "能源", "熱源", "震源", "音源",
];

/// Common Chinese function words and particles used to help segmentation.
/// These are not rule terms but high-frequency words that appear between
/// meaningful content words.  They carry freq=10 in from_rules().
static STOP_WORDS: &[&str] = &[
    // Pronouns
    "我", "你", "他", "她", "它", "我們", "你們", "他們", "自己", // Demonstratives
    "這", "那", "這個", "那個", "這些", "那些", "這裡", "那裡", // Particles
    "的", "了", "著", "過", "嗎", "呢", "吧", "啊", "呀", "啦",
    // Prepositions / conjunctions
    "在", "把", "被", "讓", "給", "跟", "和", "與", "或", "但", "而", "因為", "所以", "如果",
    "雖然", "但是", "而且", "或者", "不過", // Auxiliary verbs / common verbs
    "是", "有", "沒有", "會", "能", "可以", "要", "應該", "不", "去", "來", "做", "用", "說", "看",
    "想", "知道", // Adverbs
    "很", "太", "非常", "都", "也", "就", "才", "又", "再", "已經", "正在", "一直", "還", "更",
    "最", // Measure words / quantifiers
    "個", "位", "件", "條", "種", "些", "每", "各", "多", "少",
    // Common nouns (high frequency)
    "人", "時候", "地方", "東西", "問題", "方法", "情況", "結果", "時間", "工作", "國家", "公司",
    "系統", "技術", "使用", // Numbers
    "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "萬",
];

#[cfg(test)]
#[path = "../../tests/unit/engine/segment/tests.rs"]
mod tests;
