//! Which register a document is written in.
//!
//! A peer of [`crate::engine::zhtype`]: character and phrase tables plus one
//! classifier that reads raw text before the scan and answers a single
//! question about the whole document.
//!
//! Evidence only, never a rule. This decides which detectors should hold their
//! tongue, never what to rewrite. A false formal reading costs a suppression;
//! no false reading costs an edit, so every gate below errs toward Casual.

use crate::engine::scan::{char_bounded_end, is_cjk_ideograph};
use crate::rules::ruleset::Register;

// Forms that occur in 公文 and formal correspondence and effectively nowhere
// else, so one anywhere in the document settles the register. A letter that
// opens on two paragraphs of context still signs off 謹啟. The flag marks an
// anchor that also has to end a phrase. 此致 is the one that needs it: it is a
// prefix of 致命, 致使 and 致力, so 此致命的錯誤 opened a document with what
// looked like a sign-off. A 公文 puts 此致 on a line of its own before 敬禮, so
// demanding a boundary after it costs nothing real.
//
// The others do not take the test, because they legitimately run into the next
// word: 謹此陳報 and 敬啟者： are what those look like.
const FORMAL_ANCHORS: &[(&str, bool)] = &[
    ("敬啟者", false),
    ("謹啟", false),
    ("謹此", false),
    ("茲就", false),
    ("鈞鑒", false),
    ("台端", false),
    ("惠請", false),
    ("此致", true),
    ("特此函達", false),
    ("相應函復", false),
];

// How a contract refers to itself in its own opening. The determiner is the
// whole of the evidence: 合約 on its own is the subject of any article about
// contract law, and scoping the bare noun to the head was not enough, because a
// note short enough to be all head is exactly the casual writing that then lost
// its findings.
//
// 本 only. 該合約 and 此合約 are anaphoric, which is how an article refers to a
// contract it has just named, so they carry no evidence that the document in
// hand is the contract. Missing a contract whose opening never says 本合約 is
// the cheaper mistake, for the same reason the anchors take a boundary test.
const FORMAL_HEAD_MARKERS: &[&str] = &["本合約", "本契約"];

// How much of the document counts as the head for the weaker markers.
const FORMAL_HEAD_CHARS: usize = 100;

/// Whether `anchor` occurring at `at` starts a phrase rather than continuing a
/// word.
///
/// Chinese has no spaces to search between, so a bare substring test reads
/// 台端 out of 平台端 and 此致 out of 因此致使, and either one silently turns a
/// technical document formal. The tell is the character in front: an anchor
/// that opens a salutation follows a line break, punctuation or nothing at
/// all, while a false hit follows the ideograph that owns it.
///
/// Deliberately one-sided. 此致敬禮 and 敬啟者： continue into CJK on the right
/// and are exactly what this looks for, so only the left side is tested.
///
/// The cost of being wrong is asymmetric, which is why this errs strict: a
/// missed 公文 leaves the linter where it was before the register existed,
/// while a false formal reading silently drops real findings. That is the
/// trade that rejects 王大明謹啟, whose sign-off runs straight on from the
/// name, and it is the right way to be wrong.
fn starts_a_phrase(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        // Digits and Latin letters are as much a word in progress as an
        // ideograph is: 2024台端 and 版本3此致 are not salutations.
        .is_none_or(|prev| !is_cjk_ideograph(prev) && !prev.is_alphanumeric())
}

/// Whether what follows `end` closes the anchor rather than continuing a word.
fn ends_a_phrase(text: &str, end: usize) -> bool {
    text[end..]
        .chars()
        .next()
        .is_none_or(|next| !is_cjk_ideograph(next))
}

fn has_formal_anchor(text: &str) -> bool {
    FORMAL_ANCHORS.iter().any(|&(anchor, needs_trailing)| {
        text.match_indices(anchor).any(|(at, m)| {
            starts_a_phrase(text, at) && (!needs_trailing || ends_a_phrase(text, at + m.len()))
        })
    })
}

/// Decide whether `text` is written in a formal register.
pub(crate) fn detect_register(text: &str) -> Register {
    if has_formal_anchor(text) {
        return Register::Formal;
    }
    let head_end = char_bounded_end(text, 0, FORMAL_HEAD_CHARS);
    let head = &text[..head_end];
    if FORMAL_HEAD_MARKERS.iter().any(|m| {
        head.match_indices(m)
            .any(|(at, _)| starts_a_phrase(head, at))
    }) {
        return Register::Formal;
    }
    Register::Casual
}

#[cfg(test)]
#[path = "../../tests/unit/engine/register/tests.rs"]
mod tests;
