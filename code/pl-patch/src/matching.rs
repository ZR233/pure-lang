//! patch 上下文行匹配启发式：空白/Unicode 等价、JSON 键保留匹配。

use std::path::Path;

pub(crate) fn find_sequence(
    lines: &[String],
    pattern: &[String],
    start: usize,
    eof: bool,
) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start.min(lines.len()));
    }
    if pattern.len() > lines.len() {
        return None;
    }
    let last_start = lines.len().saturating_sub(pattern.len());
    if eof {
        let end_indices = [last_start];
        if let Some(index) = find_sequence_in_indices(lines, pattern, end_indices) {
            return Some(index);
        }
    }
    if start > last_start {
        return None;
    }
    find_sequence_in_indices(lines, pattern, start..=last_start)
}

fn find_sequence_in_indices(
    lines: &[String],
    pattern: &[String],
    indices: impl IntoIterator<Item = usize> + Clone,
) -> Option<usize> {
    for index in indices.clone() {
        if lines[index..index + pattern.len()] == *pattern {
            return Some(index);
        }
    }
    for index in indices.clone() {
        if pattern
            .iter()
            .enumerate()
            .all(|(offset, expected)| lines[index + offset].trim_end() == expected.trim_end())
        {
            return Some(index);
        }
    }
    for index in indices.clone() {
        if pattern
            .iter()
            .enumerate()
            .all(|(offset, expected)| lines[index + offset].trim() == expected.trim())
        {
            return Some(index);
        }
    }
    indices.into_iter().find(|&index| {
        pattern.iter().enumerate().all(|(offset, expected)| {
            normalize_line_for_match(&lines[index + offset]) == normalize_line_for_match(expected)
        })
    })
}

pub(crate) fn lines_equivalent(left: &str, right: &str) -> bool {
    left.trim() == right.trim() || normalize_line_for_match(left) == normalize_line_for_match(right)
}

fn normalize_line_for_match(line: &str) -> String {
    line.trim()
        .chars()
        .map(|ch| match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

pub(crate) fn supports_preserved_json_context(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("arb")
        })
}

pub(crate) fn find_preserved_json_context_sequence(
    lines: &[String],
    old_lines: &[String],
    new_lines: &[String],
    start: usize,
    eof: bool,
) -> Option<usize> {
    if old_lines.is_empty() || old_lines.len() > lines.len() {
        return None;
    }
    let preserved = preserved_old_lines(old_lines, new_lines);
    if !preserved.iter().any(|preserved| *preserved) {
        return None;
    }
    let last_start = lines.len().saturating_sub(old_lines.len());
    let matches_at = |index: usize| {
        old_lines.iter().enumerate().all(|(offset, expected)| {
            let actual = &lines[index + offset];
            lines_equivalent(actual, expected)
                || preserved[offset] && same_json_property_key(actual, expected)
        })
    };
    if eof && matches_at(last_start) {
        return Some(last_start);
    }
    (start..=last_start).find(|index| matches_at(*index))
}

fn preserved_old_lines(old_lines: &[String], new_lines: &[String]) -> Vec<bool> {
    let mut preserved = vec![false; old_lines.len()];
    let mut new_search_start = 0;
    for (old_index, old_line) in old_lines.iter().enumerate() {
        let Some(relative_index) = new_lines[new_search_start..]
            .iter()
            .position(|new_line| lines_equivalent(old_line, new_line))
        else {
            continue;
        };
        preserved[old_index] = true;
        new_search_start += relative_index + 1;
    }
    preserved
}

fn same_json_property_key(left: &str, right: &str) -> bool {
    json_property_key(left)
        .zip(json_property_key(right))
        .is_some_and(|(left, right)| left == right)
}

fn json_property_key(line: &str) -> Option<&str> {
    let property = line.trim_start().strip_prefix('"')?;
    let key_end = property.find("\":")?;
    let key = &property[..key_end];
    (!key.contains("\\\"")).then_some(key)
}
