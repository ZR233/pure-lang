pub(crate) fn line_start_byte_offset(
    content: &str,
    line_start: usize,
) -> std::result::Result<usize, String> {
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
    Err(format!(
        "startLine {line_start} exceeds file length ({current_line} lines)"
    ))
}

pub(crate) fn line_end_byte_offset(
    content: &str,
    start_byte: usize,
    line_count: Option<usize>,
) -> usize {
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

pub(crate) fn logical_line_count(content: &str) -> usize {
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
