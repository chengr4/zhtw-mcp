// Excluded range builder.
//
// Builds byte ranges that should be excluded from spell-checking: URLs, file
// paths, and @mentions. Code block/inline code exclusion is handled by
// pulldown-cmark (see markdown.rs) for both plain-text and Markdown input,
// replacing the former regex-based backtick patterns.

use std::sync::LazyLock;

use regex::Regex;

// Core type

/// A half-open byte range [start, end) within a UTF-8 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

// Compiled regexes (compiled once, reused forever)

// [^\s「」『』《》]+ matches any non-whitespace character that is not one of
// the six CJK quote/bracket marks used by validate_quote_hierarchy. This
// correctly handles IRIs (URLs with unencoded CJK path segments, common for
// zh.wikipedia.org) while preventing the pattern from swallowing adjacent
// Chinese quotation marks that follow a URL (e.g. [title](url)」prose).
static RE_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[a-zA-Z0-9+.\-]*://[^\s「」『』《》]+").expect("static URL regex")
});

// Same stop-set as RE_URL: halt before CJK quote/bracket characters so that
// 《/path/to/file》 does not swallow the closing 》 into the excluded range.
static RE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\.\.?/|/)[^\s「」『』《》]+").expect("static path regex"));

static RE_MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@[a-zA-Z0-9_]+").expect("static mention regex"));

// Public API

/// Build excluded ranges for content patterns: URLs, file paths, @mentions.
///
/// Code block and inline code exclusion is handled separately by pulldown-cmark
/// (via build_markdown_excluded_ranges in markdown.rs). This function covers
/// only content-pattern exclusions that operate on text structure, not syntax.
///
/// Returned ranges are sorted by start position and non-overlapping.
pub fn build_excluded_ranges(content: &str) -> Vec<ByteRange> {
    let mut ranges: Vec<ByteRange> = Vec::new();

    // 1. URLs (no overlap check). RE_URL uses [^\s「」『』《》]+ so it covers
    //    IRIs (unencoded CJK path segments) while stopping before quote marks.
    add_matched_ranges(content, &RE_URL, &mut ranges, false);

    // 2. File paths (custom handling)
    add_path_ranges(content, &mut ranges);

    // 3. @mentions (with overlap check)
    add_matched_ranges(content, &RE_MENTION, &mut ranges, true);

    merge_ranges_pub(ranges)
}

/// Check whether the byte span [start, end) overlaps any excluded range.
///
/// Uses linear scan for small lists, binary search otherwise.
/// Assumes ranges is sorted and non-overlapping (output of merge_ranges).
pub fn is_excluded(start: usize, end: usize, ranges: &[ByteRange]) -> bool {
    if ranges.is_empty() || start >= end {
        return false;
    }

    if ranges.len() <= 10 {
        return ranges.iter().any(|r| start < r.end && end > r.start);
    }

    // Find the first range whose start >= end. Any overlapping range must be
    // before that index. Because ranges are sorted and non-overlapping, only
    // the immediately preceding range can overlap our span.
    let idx = ranges.partition_point(|r| r.start < end);
    idx > 0 && start < ranges[idx - 1].end
}

// Internal helpers

/// Check whether text ends with a URL (i.e. a URL extends right up to the
/// end of the string). Used by the path backtrack logic to skip paths that
/// are continuations of a URL.
fn is_url_suffix(text: &str) -> bool {
    static RE_URL_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[a-zA-Z0-9+.\-]*://[^\s「」『』《》]+$").expect("static trailing-URL regex")
    });
    RE_URL_SUFFIX.is_match(text)
}

/// Returns true if [start, end) overlaps any existing range.
fn overlaps_any(start: usize, end: usize, ranges: &[ByteRange]) -> bool {
    ranges.iter().any(|r| start < r.end && end > r.start)
}

/// Append all non-overlapping matches of regex to ranges.
/// When check_overlap is false every match is unconditionally added.
fn add_matched_ranges(
    content: &str,
    regex: &Regex,
    ranges: &mut Vec<ByteRange>,
    check_overlap: bool,
) {
    for m in regex.find_iter(content) {
        let start = m.start();
        let end = m.end();
        if !check_overlap || !overlaps_any(start, end, ranges) {
            ranges.push(ByteRange { start, end });
        }
    }
}

/// Port of #addPathRanges.
///
/// For each path match we:
/// 1. Look back up to 50 bytes to see if a URL scheme precedes it.
///    If so, the path is already covered by the URL range -- skip it.
/// 2. Check overlap with existing ranges:
///    - If the new range fully contains the overlapping range, replace it.
///    - If partial overlap, keep the existing range.
///    - If no overlap, append.
fn add_path_ranges(content: &str, ranges: &mut Vec<ByteRange>) {
    const BACKTRACK: usize = 50;

    for m in RE_PATH.find_iter(content) {
        let start = m.start();
        let end = m.end();

        // Backtrack check: is this path part of a URL? Only skip paths that are
        // continuations of a URL.
        let before_start = content.floor_char_boundary(start.saturating_sub(BACKTRACK));
        let before = &content[before_start..start];
        if is_url_suffix(before) {
            continue;
        }

        // Find the first overlapping existing range, if any.
        let overlap_idx = ranges.iter().position(|r| start < r.end && end > r.start);

        match overlap_idx {
            Some(idx) => {
                let existing = ranges[idx];
                // New range fully contains existing -- replace.
                if start <= existing.start && end >= existing.end {
                    ranges[idx] = ByteRange { start, end };
                }
                // Partial overlap -- keep existing, discard new.
            }
            None => {
                ranges.push(ByteRange { start, end });
            }
        }
    }
}

/// Sort by start position and merge overlapping / adjacent ranges.
pub fn merge_ranges_pub(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    if ranges.len() <= 1 {
        return ranges;
    }

    ranges.sort_by_key(|r| r.start);

    let mut merged: Vec<ByteRange> = Vec::with_capacity(ranges.len());

    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }

    merged
}

// Tests

#[cfg(test)]
#[path = "../../tests/unit/engine/excluded/tests.rs"]
mod tests;
