// Line/column position mapping for byte offsets.
//
// Pre-computes newline positions to efficiently convert byte offsets to (line,
// col) coordinates. Column values use UTF-16 code units by default (matching
// LSP spec), with optional UTF-32 (char index) mode.

/// Column encoding mode for position reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEncoding {
    /// UTF-16 code units (LSP default). Surrogate pairs count as 2.
    Utf16,
    /// Unicode scalar values (char count). Each char counts as 1.
    Utf32,
}

/// Pre-computed newline index for fast byte-offset to (line, col) conversion.
pub struct LineIndex<'a> {
    /// Byte offsets of each line start. line_starts[0] is always 0.
    line_starts: Vec<usize>,
    /// The source text (borrowed for column computation).
    text: &'a str,
}

impl<'a> LineIndex<'a> {
    /// Build a line index from source text.
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts, text }
    }

    /// Build a `LineIndex` from pre-computed line starts (used by the
    /// merged detect+lineindex single-pass builder).
    pub fn from_parts(text: &'a str, line_starts: Vec<usize>) -> Self {
        Self { line_starts, text }
    }

    /// Fill `line` and `col` fields on a batch of issues whose offsets are
    /// already sorted ascending.  Single linear pass over the line-start
    /// table, which avoids an O(log n) binary search per issue.
    pub fn fill_line_col_sorted(
        &self,
        issues: &mut [crate::rules::ruleset::Issue],
        encoding: ColumnEncoding,
    ) {
        let mut line_idx = 0;

        // Incremental column cursor: (byte_offset, col_count) from the last
        // issue on the same line. When the next issue is on the same line and
        // at a later offset, we resume counting from the cursor instead of
        // re-scanning from line start.
        let mut cursor_byte: usize = 0;
        let mut cursor_col: usize = 0;

        for issue in issues.iter_mut() {
            // Advance line_idx forward.
            while line_idx + 1 < self.line_starts.len()
                && self.line_starts[line_idx + 1] <= issue.offset
            {
                line_idx += 1;
            }
            let line_byte_start = self.line_starts[line_idx];
            let offset = issue.offset.min(self.text.len());

            // If cursor is on the same line and at or before this offset, count
            // incrementally from cursor. Otherwise reset from line start.
            let (scan_from, base_col) = if cursor_byte >= line_byte_start && cursor_byte <= offset {
                (cursor_byte, cursor_col)
            } else {
                (line_byte_start, 0)
            };

            let delta_slice = &self.text[scan_from..offset];
            let delta_col = match encoding {
                ColumnEncoding::Utf16 => delta_slice.encode_utf16().count(),
                ColumnEncoding::Utf32 => delta_slice.chars().count(),
            };
            let col = base_col + delta_col;

            issue.line = line_idx + 1;
            issue.col = col + 1;

            // Update cursor for next issue.
            cursor_byte = offset;
            cursor_col = col;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/engine/lineindex/tests.rs"]
mod tests;
