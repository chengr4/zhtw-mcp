// Google Translate calibration layer for cross-strait term verification.
//
// Pipeline (calibrate, not confirm):
//   1. Scanner finds issues; some have 'english' fields from matched rules.
//   2. Extract ±sentence context around each issue, deduplicate.
//   3. Single google_translate_raw() call (zh→en) on a sentinel-delimited payload.
//   4. For each issue with 'english' field, check if content-word anchors
//      appear in the corresponding translated segment.
//   5. Set issue.anchor_match = Some(true/false/None) as annotation.
//   6. No severity mutation. Pure annotation. Fail-open on API failure.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

use crate::rules::ruleset::Issue;

/// Errors from the Google Translate API layer.
#[derive(Debug)]
pub enum TranslateError {
    /// Network or I/O error.
    Io(String),
    /// HTTP rate limit (429) or server error (5xx).
    RateLimit(u16),
    /// JSON parse error in the response.
    Parse(String),
    /// Outbound network calls are disabled by policy.
    Disabled,
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "translate I/O error: {msg}"),
            Self::RateLimit(code) => write!(f, "translate rate-limited (HTTP {code})"),
            Self::Parse(msg) => write!(f, "translate parse error: {msg}"),
            Self::Disabled => write!(f, "outbound network calls are disabled by {NO_NETWORK_ENV}"),
        }
    }
}

const GOOGLE_TRANSLATE_URL: &str = "https://translate.googleapis.com/translate_a/single";

/// Environment variable that disables every outbound network call.
///
/// Set to any value other than empty or "0" to refuse calibration.
pub const NO_NETWORK_ENV: &str = "ZHTW_NO_NETWORK";

/// Whether outbound network calls are refused by operator policy.
///
/// This module is the only code in the crate that opens a socket, and what it
/// sends is sentence-sized excerpts of the document being linted. A linter
/// runs over unpublished writing, and under MCP the decision to pass "verify"
/// belongs to a model rather than to the person whose text it is. An operator
/// therefore needs a switch that a caller cannot argue its way past, which is
/// why this is read from the environment rather than taken as a parameter.
pub fn network_disabled() -> bool {
    disabled_by(std::env::var_os(NO_NETWORK_ENV).as_deref())
}

/// Refuse a network-dependent flag when the operator has disabled egress.
///
/// One place, so the CLI paths and the MCP path cannot drift apart on what the
/// policy is or how it is phrased. `flag` is what the caller passed, named back
/// to them: "--verify" for the CLI, "verify" for the tool argument.
pub fn refuse_if_network_disabled(flag: &str) -> Result<(), String> {
    if network_disabled() {
        return Err(format!(
            "{flag} needs a network call to Google Translate, which {NO_NETWORK_ENV} forbids"
        ));
    }
    Ok(())
}

/// The value test, split out from reading the environment.
///
/// Kept separate so the off-values can be tested without either mutating
/// process-global state or, far worse, driving a real `--verify` run just to
/// observe that it was not refused: that would have posted the fixture to
/// Google from the test suite for this very feature.
fn disabled_by(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}
const USER_AGENT: &str = "Mozilla/5.0 (compatible; zhtw-anchor/2.0)";

/// Maximum payload bytes sent to Google Translate in a single request.
/// The free endpoint rejects overly long URLs; keep well under the ~8KB
/// practical limit for GET query strings.
const MAX_PAYLOAD_BYTES: usize = 4096;

/// English stopwords to exclude from anchor matching.  These are so common
/// in any translation that matching on them provides zero signal.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "shall", "should", "may", "might", "must", "can",
    "could", "of", "in", "to", "for", "with", "on", "at", "from", "by", "as", "into", "through",
    "during", "before", "after", "above", "below", "between", "under", "again", "further", "then",
    "once", "here", "there", "when", "where", "why", "how", "all", "each", "every", "both", "few",
    "more", "most", "other", "some", "such", "no", "nor", "not", "only", "own", "same", "so",
    "than", "too", "very", "just", "about", "also", "and", "but", "or", "if", "while", "that",
    "this", "these", "those", "it", "its", "he", "she", "they", "them", "we", "you", "i", "me",
    "my", "your", "his", "her", "our", "their", "what", "which", "who", "whom", "s", "t", "don",
    "doesn", "didn", "won", "wouldn", "shouldn", "couldn",
];

/// Tokenize English text into lowercase words, splitting on whitespace and
/// punctuation (preserving hyphens and apostrophes within words).  Hyphenated
/// and possessive tokens also emit their sub-parts.
pub(crate) fn tokenize_words(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for chunk in text.split(|c: char| {
        c.is_ascii_whitespace() || (c.is_ascii_punctuation() && c != '-' && c != '\'')
    }) {
        if chunk.is_empty() {
            continue;
        }
        tokens.push(chunk.to_string());
        // Emit sub-parts for hyphenated or possessive tokens.
        if chunk.contains('-') || chunk.contains('\'') {
            for sub in chunk.split(['-', '\'']) {
                if !sub.is_empty() && sub != chunk {
                    tokens.push(sub.to_string());
                }
            }
        }
    }
    tokens
}

/// Call Google Translate free endpoint (client=gtx).
///
/// Returns `TranslateError::Parse` if the response is valid JSON but contains
/// no translated text fragments (endpoint shape change).
pub(crate) fn google_translate_raw(
    text: &str,
    src: &str,
    tgt: &str,
) -> Result<String, TranslateError> {
    // Backstop, not the primary check. Callers reject "verify" up front with a
    // message naming the flag; this is here so a future call path that forgets
    // to ask still cannot open the socket.
    if network_disabled() {
        return Err(TranslateError::Disabled);
    }

    let url = format!(
        "{GOOGLE_TRANSLATE_URL}?client=gtx&sl={src}&tl={tgt}&dt=t&q={}",
        urlencoding::encode(text)
    );

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    );
    let body_str = match agent.get(&url).header("User-Agent", USER_AGENT).call() {
        Ok(mut resp) => match resp.body_mut().read_to_string() {
            Ok(s) => s,
            Err(e) => return Err(TranslateError::Io(e.to_string())),
        },
        Err(ureq::Error::StatusCode(code @ (429 | 500..=599))) => {
            return Err(TranslateError::RateLimit(code));
        }
        Err(e) => {
            return Err(TranslateError::Io(e.to_string()));
        }
    };

    let body: serde_json::Value =
        serde_json::from_str(&body_str).map_err(|e| TranslateError::Parse(e.to_string()))?;

    // Response format: [[["translated text","source text",null,null,N], ...],
    // ...]
    let mut result = String::new();
    if let Some(outer) = body.as_array() {
        if let Some(inner) = outer.first().and_then(|v| v.as_array()) {
            for item in inner {
                if let Some(arr) = item.as_array() {
                    if let Some(s) = arr.first().and_then(|v| v.as_str()) {
                        result.push_str(s);
                    }
                }
            }
        }
    }

    // #4: Detect endpoint shape change, valid JSON but no translated fragments.
    if result.is_empty() {
        return Err(TranslateError::Parse(
            "response JSON had no translated text fragments".into(),
        ));
    }

    Ok(result)
}

/// Result of a calibration run.
#[derive(Debug, Clone)]
pub struct CalibrateResult {
    /// Whether the API call succeeded.
    pub api_ok: bool,
    /// The full translated text (empty if API failed).
    pub translated: String,
    /// Number of tokens in the translation.
    pub token_count: usize,
    /// Issues where anchor was found in translation.
    pub matched: usize,
    /// Issues where anchor was NOT found in translation.
    pub unmatched: usize,
    /// Issues with no `english` field (left as None).
    pub no_english: usize,
}

/// Sentinel prefix for segment delimiters in the translation payload.
/// Chosen to be unlikely to appear in Chinese text and stable through
/// translation (numbers are preserved verbatim by Google Translate).
const SENTINEL_PREFIX: &str = "###SEG";

/// Extract ±sentence context around an issue offset, bounded by CJK sentence
/// punctuation (。！？) or paragraph breaks (\n), up to ~40 characters in
/// each direction.
fn extract_issue_context(text: &str, offset: usize) -> &str {
    let offset = offset.min(text.len());
    let offset = text.floor_char_boundary(offset);

    fn is_sentence_boundary(c: char) -> bool {
        matches!(c, '\n' | '\r' | '。' | '！' | '？' | '；')
    }

    // Scan backward by characters.
    let mut start = offset;
    let mut chars_back = 0;
    for (idx, c) in text[..offset].char_indices().rev() {
        if is_sentence_boundary(c) {
            start = idx + c.len_utf8();
            break;
        }
        start = idx;
        chars_back += 1;
        if chars_back >= 40 {
            break;
        }
    }

    // Scan forward by characters.
    let mut end = offset;
    let mut chars_fwd = 0;
    for (idx, c) in text[offset..].char_indices() {
        if is_sentence_boundary(c) {
            end = offset + idx;
            break;
        }
        end = offset + idx + c.len_utf8();
        chars_fwd += 1;
        if chars_fwd >= 40 {
            break;
        }
    }

    &text[start..end]
}

/// Filter anchor words to content words only (remove stopwords and
/// single-character tokens that are likely noise).
fn content_anchor_words(english: &str) -> Vec<String> {
    let stopset: HashSet<&str> = STOPWORDS.iter().copied().collect();
    english
        .split('/')
        .flat_map(|v| tokenize_words(v.trim()))
        .map(|t| t.to_lowercase())
        .filter(|w| w.len() > 1 && !stopset.contains(w.as_str()))
        .collect()
}

/// Calibrate issues by translating their context sentences and checking for
/// English anchor matches.
///
/// - `Some(true)`: anchor present in non-empty translation segment.
/// - `Some(false)`: anchor absent in non-empty translation segment.
/// - `None`: calibration not attempted (no `english` field, API failure,
///   empty input, empty translation, no content words in anchor).
///
/// Pure annotation. No severity mutation. Fail-open on API failure.
pub fn calibrate_issues(text: &str, issues: &mut [Issue]) -> CalibrateResult {
    let mut result = CalibrateResult {
        api_ok: false,
        translated: String::new(),
        token_count: 0,
        matched: 0,
        unmatched: 0,
        no_english: 0,
    };

    // Short-circuit: nothing to calibrate.
    if text.trim().is_empty() || issues.is_empty() {
        result.no_english = issues.len();
        return result;
    }

    // Collect unique context sentences for issues that have english fields. Map
    // each issue index to its context segment index. #6: Use HashMap<String,
    // usize> instead of HashSet + position() scan.
    let mut segments: Vec<String> = Vec::new();
    let mut segment_map: HashMap<String, usize> = HashMap::new();
    let mut issue_to_segment: Vec<Option<usize>> = Vec::with_capacity(issues.len());

    for issue in issues.iter() {
        if issue.english.is_none() {
            issue_to_segment.push(None);
            result.no_english += 1;
            continue;
        }

        let ctx = extract_issue_context(text, issue.offset).to_string();
        if ctx.trim().is_empty() {
            issue_to_segment.push(None);
            result.no_english += 1;
            continue;
        }

        let seg_idx = *segment_map.entry(ctx.clone()).or_insert_with(|| {
            let idx = segments.len();
            segments.push(ctx);
            idx
        });
        issue_to_segment.push(Some(seg_idx));
    }

    if segments.is_empty() {
        return result;
    }

    // #5: Cap payload size. If the joined payload exceeds MAX_PAYLOAD_BYTES,
    // truncate to the segments that fit. Issues referencing truncated segments
    // will get anchor_match = None (fail-open). Also cap individual segments: a
    // single oversized context must not blow past the URL limit and disable
    // calibration for the entire batch.
    let max_segment_bytes = MAX_PAYLOAD_BYTES / 2; // no single segment > half budget
    let max_segments = {
        let mut total = 0usize;
        let mut count = 0usize;
        for seg in &segments {
            // Account for sentinel + newline overhead per segment.
            let overhead = SENTINEL_PREFIX.len() + 6 + 1; // "###SEGnn\n"
            let seg_cost = seg.len().min(max_segment_bytes) + overhead;
            if total + seg_cost > MAX_PAYLOAD_BYTES && count > 0 {
                break;
            }
            total += seg_cost;
            count += 1;
        }
        count
    };

    // #2: Use sentinel markers instead of bare \n for segment delimiting.
    // Format: "###SEG0\n<segment0>\n###SEG1\n<segment1>\n..." After
    // translation, find markers to recover per-segment text. Note: if user text
    // literally contains "###SEG\d+", sentinel parsing could be corrupted.
    // Acceptable risk: this pattern is vanishingly rare in zh-TW.
    let mut payload = String::new();
    let mut truncated_segments: HashSet<usize> = HashSet::new();
    for (i, seg) in segments.iter().enumerate() {
        if i >= max_segments {
            break;
        }
        if !payload.is_empty() {
            payload.push('\n');
        }

        // Truncate oversized segments at a char boundary to stay within budget.
        // Truncated segments get anchor_match = None (the anchor word might
        // have been beyond the truncation point: evaluating would create false
        // negatives).
        let truncated = if seg.len() > max_segment_bytes {
            truncated_segments.insert(i);
            &seg[..seg.floor_char_boundary(max_segment_bytes)]
        } else {
            seg.as_str()
        };
        payload.push_str(&format!("{SENTINEL_PREFIX}{i}\n{truncated}"));
    }

    // Translate.
    let translation = match google_translate_raw(&payload, "zh", "en") {
        Ok(t) => t,
        Err(_) => {
            // Fail-open: all anchor_match = None.
            return result;
        }
    };

    result.api_ok = true;
    result.translated = translation.clone();

    // #2: Parse sentinel-delimited translation back into per-segment results.
    // Look for "###SEGn" markers (case-insensitive, Google may capitalize).
    let translated_segments = parse_sentinel_segments(&translation, segments.len());

    // Tokenize each segment.
    let segment_tokens: Vec<HashSet<String>> = translated_segments
        .iter()
        .map(|seg| {
            tokenize_words(seg)
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect()
        })
        .collect();

    result.token_count = segment_tokens.iter().map(|s| s.len()).sum();

    // Check each issue against its corresponding segment.
    for (i, issue) in issues.iter_mut().enumerate() {
        let seg_idx = match issue_to_segment.get(i).copied().flatten() {
            Some(idx) => idx,
            None => continue, // no english field → anchor_match stays None
        };

        // Segment was dropped (payload cap) or individually truncated → no
        // signal. Truncated segments might have lost the anchor word beyond the
        // cut point.
        if seg_idx >= max_segments || truncated_segments.contains(&seg_idx) {
            continue;
        }

        let english = match &issue.english {
            Some(e) => e.clone(),
            None => continue,
        };

        // Get the token set for this segment.
        let tokens = match segment_tokens.get(seg_idx) {
            Some(t) if !t.is_empty() => t,
            _ => continue, // empty/missing → no signal
        };

        // #1: Filter to content words only (no stopwords, no single chars).
        let anchors = content_anchor_words(&english);
        if anchors.is_empty() {
            // All anchor words were stopwords → no signal, not a false
            // negative.
            continue;
        }

        let found = anchors.iter().any(|w| tokens.contains(w));

        issue.anchor_match = Some(found);
        if found {
            result.matched += 1;
        } else {
            result.unmatched += 1;
        }
    }

    result
}

/// Parse sentinel-delimited translation output.
///
/// Looks for `###SEGn` markers (case-insensitive) and extracts the text
/// between consecutive markers.  Returns a Vec indexed by segment number;
/// missing segments get empty strings.
fn parse_sentinel_segments(translation: &str, expected_count: usize) -> Vec<String> {
    let lower = translation.to_lowercase();
    let sentinel_lower = SENTINEL_PREFIX.to_lowercase();

    // Find all marker positions: (byte_offset, segment_number).
    let mut markers: Vec<(usize, usize)> = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find(&sentinel_lower) {
        let abs_pos = search_from + pos;
        let after_prefix = abs_pos + sentinel_lower.len();
        // Parse the segment number immediately after the prefix.
        let num_str: String = lower[after_prefix..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = num_str.parse::<usize>() {
            let content_start = after_prefix + num_str.len();
            // Skip optional newline after marker.
            let content_start = if translation.as_bytes().get(content_start) == Some(&b'\n') {
                content_start + 1
            } else {
                content_start
            };
            markers.push((content_start, n));
        }
        search_from = abs_pos + sentinel_lower.len();
    }

    let mut result = vec![String::new(); expected_count];

    if markers.is_empty() {
        // No markers found: Google may have stripped them. Fall back to newline
        // splitting for backward compat (best-effort).
        for (i, line) in translation.split('\n').enumerate() {
            if i < expected_count {
                result[i] = line.trim().to_string();
            }
        }
        return result;
    }

    // For each marker, extract text from content_start to the byte position
    // where the next marker begins (searching for the sentinel prefix in the
    // original case-insensitive text).
    for (idx, &(start, seg_num)) in markers.iter().enumerate() {
        if seg_num >= expected_count {
            continue;
        }
        let end = if idx + 1 < markers.len() {
            // Find where the next sentinel prefix starts in the original text.
            // markers[idx+1].0 is the content start (after "###SEGn\n"), so we
            // need to back up to find the "###SEG" prefix itself.
            let next_content = markers[idx + 1].0;
            // Search backwards from next_content for the sentinel prefix.
            lower[..next_content]
                .rfind(&sentinel_lower)
                .unwrap_or(next_content)
        } else {
            translation.len()
        };
        result[seg_num] = translation[start..end].trim().to_string();
    }

    result
}

#[cfg(test)]
#[path = "../../tests/unit/engine/translate/tests.rs"]
mod tests;
