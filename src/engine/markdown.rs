// Markdown-aware text extraction for zhtw-mcp linting.
//
// Uses pulldown-cmark to parse Markdown and identify regions that should be
// excluded from linting: code blocks, inline code, HTML blocks, and YAML
// frontmatter.
//
// Returns byte ranges to exclude before scanning.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::excluded::{merge_ranges_pub, ByteRange};
use super::html_lang::LangScopes;

/// Build excluded byte ranges from Markdown structure.
///
/// Excludes: fenced/indented code blocks, inline code, HTML blocks/tags,
/// and YAML frontmatter (leading --- fences).
///
/// The returned ranges are sorted by start position and non-overlapping.
/// Options controlling Markdown structural exclusion.  Defaults match
/// the historical behavior of [build_markdown_excluded_ranges]: code
/// blocks excluded, blockquotes scanned.
#[derive(Debug, Clone, Copy, Default)]
pub struct MdScanOptions {
    /// When true, fenced/indented code blocks are NOT excluded (i.e. the
    /// scanner sees code-block prose).  Used by the
    /// `MarkdownScanCode` content type.
    pub scan_code_blocks: bool,
    /// When true, the byte ranges of pulldown-cmark `Tag::BlockQuote`
    /// events are excluded.  Implemented via cmark events so that nested
    /// blockquotes (`> >`), lazy continuation lines, and blockquotes
    /// inside list items behave correctly.  Off by default: adopted
    /// blockquote prose is real content.
    pub exempt_blockquotes: bool,
}

impl MdScanOptions {
    /// Construct options for a Markdown scan: pass `scan_code_blocks=true`
    /// when running with the `MarkdownScanCode` content type, and propagate
    /// the caller's `--exempt-blockquotes` flag.  Centralizes the literal
    /// previously copy-pasted across the CLI and MCP entry points.
    pub fn new(scan_code_blocks: bool, exempt_blockquotes: bool) -> Self {
        Self {
            scan_code_blocks,
            exempt_blockquotes,
        }
    }
}

pub fn build_markdown_excluded_ranges(text: &str) -> Vec<ByteRange> {
    build_markdown_excluded_ranges_with_options(text, MdScanOptions::default())
}

/// Return line starts for Markdown blocks that end the section before them.
///
/// Only headings and thematic breaks qualify. Those are the constructs that
/// actually close a section; a list, blockquote, or code fence sits *inside*
/// one, and the sentence before it is the lead-in that introduces it, which is
/// the standard shape of technical zh-TW. Treating those as boundaries turned
/// every "…，如下所示。" before a fence into a formulaic closer.
///
/// The value of asking the parser rather than matching a line prefix is that
/// it knows a heading-shaped line inside a fence is not a heading, and that a
/// setext underline makes the line above it one. Parser event ranges can start
/// after indentation, so normalize each range to its physical source-line
/// start before handing it to the line-oriented scanner.
pub(crate) fn block_boundary_starts(text: &str) -> Vec<usize> {
    // YAML frontmatter is metadata, not a section. The parser has no notion of
    // it: the opening fence reads as a thematic break and the closing one turns
    // the last key line into a setext heading, so both would land here as
    // boundaries. Nothing can precede the first of them, which makes this
    // unreachable for the closer detector today, but the list is supposed to
    // hold blocks that end a section and neither of these does.
    let frontmatter_end = detect_frontmatter(text).unwrap_or(0);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;

    // Depth tracks container nesting. A heading or a break inside a quote or a
    // list item belongs to that container, not to the document: "> # 標題" does
    // not end the section the quote sits in, and counting it flagged the
    // sentence above the quote as a formulaic closer.
    let mut depth = 0usize;
    let mut starts = Vec::new();
    for (event, range) in Parser::new_ext(text, opts).into_offset_iter() {
        match event {
            // Item is not counted: it only ever occurs inside a List, which has
            // already made the depth non-zero.
            Event::Start(Tag::BlockQuote(_) | Tag::List(_)) => depth += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_)) => {
                depth = depth.saturating_sub(1);
            }
            Event::Rule | Event::Start(Tag::Heading { .. })
                if depth == 0 && range.start >= frontmatter_end =>
            {
                starts.push(text[..range.start].rfind(['\n', '\r']).map_or(0, |i| i + 1));
            }
            _ => {}
        }
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Like [build_markdown_excluded_ranges], but fenced/indented code blocks are
/// NOT excluded.  Only inline code (`backtick`), HTML, and YAML frontmatter
/// are excluded.  This allows linting Chinese prose inside code blocks
/// (comments, translated output, etc.) while still protecting inline code
/// and HTML from false positives.
pub fn build_markdown_excluded_ranges_no_code(text: &str) -> Vec<ByteRange> {
    build_markdown_excluded_ranges_with_options(
        text,
        MdScanOptions {
            scan_code_blocks: true,
            ..MdScanOptions::default()
        },
    )
}

/// Build Markdown exclusion ranges with explicit options.  Backbone for the
/// two named wrappers above, plus opt-in features like `exempt_blockquotes`.
pub fn build_markdown_excluded_ranges_with_options(
    text: &str,
    opts: MdScanOptions,
) -> Vec<ByteRange> {
    let mut ranges = Vec::new();

    // Pre-pass: detect YAML frontmatter (leading --- fence). Exclude only the
    // structural tokens (--- fences, key+colon spans, ASCII quote delimiters),
    // leaving value prose scannable so that linting catches issues in
    // title/description.
    if let Some(fm_end) = detect_frontmatter(text) {
        collect_frontmatter_structural_ranges(text, fm_end, &mut ranges);
    }

    collect_container_fence_ranges(text, &mut ranges);

    let parser_opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, parser_opts);
    let mut in_code_block = false;
    let mut code_block_start = 0usize;
    let mut blockquote_depth: usize = 0;
    let mut blockquote_start: usize = 0;
    let mut lang_scopes = LangScopes::new();

    for (event, range) in parser.into_offset_iter() {
        match event {
            // Fenced or indented code blocks: exclude entire block (default) or
            // scan their prose (when scan_code_blocks is set).
            Event::Start(Tag::CodeBlock(_)) if !opts.scan_code_blocks => {
                in_code_block = true;
                code_block_start = range.start;
            }
            Event::End(TagEnd::CodeBlock) if in_code_block => {
                ranges.push(ByteRange {
                    start: code_block_start,
                    end: range.end,
                });
                in_code_block = false;
            }

            Event::Start(Tag::BlockQuote(_)) if opts.exempt_blockquotes => {
                if blockquote_depth == 0 {
                    blockquote_start = range.start;
                }
                blockquote_depth = blockquote_depth.saturating_add(1);
            }
            Event::End(TagEnd::BlockQuote(_)) if opts.exempt_blockquotes => {
                blockquote_depth = blockquote_depth.saturating_sub(1);
                if blockquote_depth == 0 {
                    ranges.push(ByteRange {
                        start: blockquote_start,
                        end: range.end,
                    });
                }
            }

            // Inline code: exclude the span including backticks.
            Event::Code(_) => {
                ranges.push(ByteRange {
                    start: range.start,
                    end: range.end,
                });
            }

            // HTML: the tags themselves are never prose, and a tag carrying a
            // non-Chinese lang also takes the prose it wraps out of the scan.
            // The event text is read from the source rather than from the
            // event's own string so the offsets fed to the tracker are the
            // document's.
            Event::Html(_) | Event::InlineHtml(_) => {
                ranges.push(ByteRange {
                    start: range.start,
                    end: range.end,
                });
                lang_scopes.feed(&text[range.start..range.end], range.start);
            }

            _ => {}
        }
    }

    ranges.extend(lang_scopes.finish(text.len()));

    // Sort and merge (frontmatter + parser ranges may overlap).
    merge_ranges_pub(ranges)
}

/// Build excluded byte ranges for YAML structural tokens.
///
/// Excludes YAML key tokens (the key name + colon) so that bare ASCII colons
/// in key-value separators do not trigger false-positive colon warnings.
/// Only the key portion is excluded; values after the colon are prose and
/// remain scannable.
///
/// Pattern matched: /^\s*\w[\w-]*\s*:/ on each line.
/// Examples excluded: title:, key-name:, summary  :.
/// Not excluded: list items (- value), comments (# text), values.
pub fn build_yaml_excluded_ranges(text: &str) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let mut pos = 0usize;

    for raw_line in text.split('\n') {
        let line_len = raw_line.len();

        if let Some(colon_pos) = yaml_key_colon_pos(raw_line) {
            // Exclude from the start of the line through the ':' (inclusive).
            ranges.push(ByteRange {
                start: pos,
                end: pos + colon_pos + 1,
            });
        }

        pos += line_len + 1; // +1 for the '\n'
    }

    ranges
}

/// Find the byte offset of the YAML key-separator colon in a line, if present.
///
/// Per the YAML spec, a block-mapping key separator is a : followed by a
/// space, tab, or end-of-line.  This handles all common block-mapping forms:
///
/// - key: value          , simple key
/// - key-name: value     , hyphenated key
/// -   indented: value   , indented key
/// - - key: value        , key inside a list item
/// - "quoted-key": value , quoted key (quote skipped; no escape handling)
///
/// Known limitations (acceptable for prose documentation YAML):
/// - Flow mappings without whitespace after : (e.g. {key:"val"}) are not
///   detected; those are rare in documentation YAML and equivalent to JSON.
/// - Only the first key-colon per line is excluded; additional key-colons in
///   flow sequences ({a: 1, b: 2}) on the same line are not excluded.
/// - Escaped quotes inside quoted keys (e.g. "key\"name": v) may confuse
///   the quote-tracking state.
///
/// Returns the byte offset of the : within the line, or None.
/// Colons inside single- or double-quoted strings are skipped.
fn yaml_key_colon_pos(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_quote = false;
    let mut quote_char = b'\0';

    while i < len {
        let b = bytes[i];
        if in_quote {
            if b == quote_char {
                in_quote = false;
            }
        } else if b == b'"' || b == b'\'' {
            in_quote = true;
            quote_char = b;
        } else if b == b':' {
            // YAML key separator: colon must be followed by whitespace or be at
            // EOL.
            let next = bytes.get(i + 1).copied().unwrap_or(b' ');
            if next == b' ' || next == b'\t' || next == b'\r' {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// A Markdown table cell span with `(row, column)` coordinates.
#[derive(Debug, Clone, Copy)]
pub struct TableCellSpan {
    pub start: usize,
    pub end: usize,
    pub row: usize,
    pub col: usize,
}

/// Extract byte ranges of all Markdown table cells with their (row, column).
///
/// Row 0 is the header row (or row 1 if no separator).  Column index is the
/// 0-based position within the row.  Returns empty if the document has no
/// tables.
pub fn extract_table_cell_spans(text: &str) -> Vec<TableCellSpan> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, opts);
    let mut spans = Vec::new();
    let mut in_table = false;
    let mut current_row = 0usize;
    let mut current_col = 0usize;
    let mut cell_active = false;
    let mut cell_start = 0usize;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                current_row = 0;
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) if in_table => {
                current_col = 0;
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) if in_table => {
                current_row += 1;
            }
            Event::Start(Tag::TableCell) if in_table => {
                cell_active = true;
                cell_start = range.start;
            }
            Event::End(TagEnd::TableCell) if in_table && cell_active => {
                spans.push(TableCellSpan {
                    start: cell_start,
                    end: range.end,
                    row: current_row,
                    col: current_col,
                });
                current_col += 1;
                cell_active = false;
            }
            _ => {}
        }
    }
    spans
}

/// Extract byte ranges of heading text inline content in Markdown.
///
/// Returns ranges covering only the inline text inside heading containers
/// (H1-H6), excluding the leading `#` markers and trailing whitespace.
/// Used by the scan pipeline to apply severity boosting on heading text.
///
/// Issues outside the inline text (e.g. inside ATX `#` markers) are not
/// boosted, since they are unlikely to represent real heading prose hits.
///
/// YAML frontmatter is excluded: pulldown-cmark would otherwise interpret
/// the closing `---` as a setext H2 underline, falsely treating frontmatter
/// values as heading text.
pub fn extract_heading_ranges(text: &str) -> Vec<ByteRange> {
    let frontmatter_end = detect_frontmatter(text).unwrap_or(0);

    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, opts);
    let mut ranges = Vec::new();
    let mut in_heading = false;
    let mut current_inline_start: Option<usize> = None;
    let mut current_inline_end: Option<usize> = None;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                in_heading = true;
                current_inline_start = None;
                current_inline_end = None;
            }
            Event::End(TagEnd::Heading(_)) if in_heading => {
                if let (Some(start), Some(end)) = (current_inline_start, current_inline_end) {
                    // Skip false-positive headings synthesised from frontmatter
                    // content + closing "---".
                    if start >= frontmatter_end {
                        ranges.push(ByteRange { start, end });
                    }
                }
                in_heading = false;
            }
            Event::Text(_) | Event::Code(_) | Event::Html(_) | Event::InlineHtml(_)
                if in_heading =>
            {
                if current_inline_start.is_none() {
                    current_inline_start = Some(range.start);
                }
                current_inline_end = Some(range.end);
            }
            _ => {}
        }
    }
    ranges
}

/// Collect YAML frontmatter structural ranges: opening `---` line, closing
/// `---` line, per-line key+colon spans, and bare ASCII `"` / `'` quote
/// bytes used as scalar delimiters.  Values remain scannable; only the
/// 1-byte delimiters are masked from the punctuation scanner so that
/// downstream YAML parsers continue to see ASCII quotes.
fn collect_frontmatter_structural_ranges(text: &str, fm_end: usize, ranges: &mut Vec<ByteRange>) {
    let fm = &text[..fm_end];
    let mut pos = 0usize;

    for raw_line in fm.split('\n') {
        let line_len = raw_line.len();
        let trimmed = raw_line.trim_end_matches('\r');

        if trimmed == "---" {
            // Exclude the entire fence line (and the trailing \n if present).
            let end = (pos + line_len + 1).min(fm_end);
            ranges.push(ByteRange { start: pos, end });
        } else if let Some(colon_pos) = yaml_key_colon_pos(raw_line) {
            // Exclude the key+colon prefix; value text remains scannable.
            ranges.push(ByteRange {
                start: pos,
                end: pos + colon_pos + 1,
            });
        }

        // Preserve ASCII '"' and "'" bytes used as YAML scalar delimiters by
        // excluding them from the punctuation scanner. Without this, the
        // scanner converts '"' to "「"/"」" inside frontmatter values and
        // breaks downstream YAML parsers (regression observed in ai-muninn.com
        // calque blindspot sweep, 2026-05).
        for (i, b) in raw_line.bytes().enumerate() {
            if b == b'"' || b == b'\'' {
                ranges.push(ByteRange {
                    start: pos + i,
                    end: pos + i + 1,
                });
            }
        }

        pos += line_len + 1; // +1 for the '\n'
        if pos >= fm_end {
            break;
        }
    }
}

/// Collect container-block fence lines (:::keyword / :::) as excluded ranges.
/// Used by HackMD and Docusaurus for admonitions.  Only the fence lines
/// themselves are excluded; the prose content between them is still scanned.
fn collect_container_fence_ranges(text: &str, ranges: &mut Vec<ByteRange>) {
    let mut pos = 0usize;
    for raw_line in text.split('\n') {
        let line_len = raw_line.len();
        let trimmed = raw_line.trim_start_matches([' ', '\t']);
        if trimmed.starts_with(":::") {
            ranges.push(ByteRange {
                start: pos,
                end: pos + line_len,
            });
        }
        pos += line_len + 1; // +1 for the '\n'
    }
}

/// Detect YAML frontmatter delimited by --- at the start of the document.
/// Returns the byte offset just past the closing ---\n (or end of closing ---).
fn detect_frontmatter(text: &str) -> Option<usize> {
    // Must start at the very beginning with --- followed by a newline.
    if !text.starts_with("---") {
        return None;
    }

    let after_open = if text.starts_with("---\n") {
        4
    } else if text.starts_with("---\r\n") {
        5
    } else {
        return None;
    };

    // Find the closing --- on its own line.
    let rest = &text[after_open..];
    for (line_start, line) in rest.split('\n').scan(0usize, |pos, line| {
        let start = *pos;
        *pos += line.len() + 1; // +1 for the \n
        Some((start, line))
    }) {
        let trimmed = line.trim_end_matches('\r');
        if trimmed == "---" {
            // End position is after the closing ---\n.
            let abs_end = after_open + line_start + line.len() + 1;
            return Some(abs_end.min(text.len()));
        }
    }

    None
}

#[cfg(test)]
#[path = "../../tests/unit/engine/markdown/tests.rs"]
mod tests;
