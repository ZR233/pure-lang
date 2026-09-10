//! UTF-8 line boundaries shared by file and note output projections.

/// A requested line lies beyond the available content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRangeError {
    requested_line: usize,
    available_lines: usize,
}

impl std::fmt::Display for LineRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "startLine {} exceeds file length ({} lines)",
            self.requested_line, self.available_lines
        )
    }
}

impl std::error::Error for LineRangeError {}

/// Finds a one-based line start; values at or below one select the start of the content.
///
/// # Errors
/// Returns the requested and available line counts when the start is out of range.
pub fn line_start_byte_offset(content: &str, line_start: usize) -> Result<usize, LineRangeError> {
    if line_start <= 1 {
        return Ok(0);
    }
    let mut current_line = 1;
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == line_start {
                return Ok(idx + 1);
            }
        }
    }
    Err(LineRangeError {
        requested_line: line_start,
        available_lines: current_line,
    })
}

/// Returns the exclusive byte offset for a bounded number of lines.
///
/// # Panics
/// Panics if start_byte is outside the content or is not a UTF-8 character boundary.
/// Callers can obtain a valid offset from line_start_byte_offset.
pub fn line_end_byte_offset(content: &str, start_byte: usize, line_count: Option<usize>) -> usize {
    let Some(line_count) = line_count else {
        return content.len();
    };
    let mut lines_seen = 1;
    for (relative_idx, ch) in content[start_byte..].char_indices() {
        if ch == '\n' {
            if lines_seen == line_count {
                return start_byte + relative_idx + 1;
            }
            lines_seen += 1;
        }
    }
    content.len()
}

/// Counts logical lines without introducing an extra line after a trailing newline.
pub fn logical_line_count(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count().max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_unicode_line_ranges_by_byte_offset() {
        let content = "一\ntwo\n三";
        let start = line_start_byte_offset(content, 2).unwrap();
        let end = line_end_byte_offset(content, start, Some(1));

        assert_eq!(&content[start..end], "two\n");
        assert_eq!(logical_line_count(content), 3);
    }
}
