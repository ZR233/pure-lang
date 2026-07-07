use std::path::Path;

use pl_protocol::PureError;

use super::parser::{PATCH_RETRY_GUIDANCE, UpdateChunk};
use super::tool_error;

pub(crate) fn apply_chunks(
    content: &str,
    path: &Path,
    chunks: &[UpdateChunk],
) -> Result<String, PureError> {
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut cursor = 0;
    let mut replacements = Vec::new();

    for chunk in chunks {
        if let Some(context) = &chunk.context {
            let Some(context_index) =
                find_sequence(&lines, std::slice::from_ref(context), cursor, false)
            else {
                return Err(tool_error(format!(
                    "failed to find context '{context}' in {}. {PATCH_RETRY_GUIDANCE}",
                    path.display(),
                )));
            };
            cursor = context_index + 1;
        }

        if chunk.old_lines.is_empty() {
            let insert_at = lines.len();
            replacements.push((insert_at, 0, chunk.new_lines.clone()));
            cursor = insert_at;
            continue;
        }

        let Some((start, old_len, new_lines)) = find_chunk_replacement(&lines, chunk, cursor)
        else {
            return Err(tool_error(format!(
                "failed to find expected lines in {}:\n{}\n{PATCH_RETRY_GUIDANCE}",
                path.display(),
                chunk.old_lines.join("\n")
            )));
        };
        replacements.push((start, old_len, new_lines));
        cursor = start + old_len;
    }

    replacements.sort_by_key(|(start, _, _)| *start);
    for (start, old_len, new_lines) in replacements.into_iter().rev() {
        lines.splice(start..start + old_len, new_lines);
    }
    if !lines.last().is_some_and(String::is_empty) {
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

fn find_chunk_replacement(
    lines: &[String],
    chunk: &UpdateChunk,
    cursor: usize,
) -> Option<(usize, usize, Vec<String>)> {
    let mut candidates = vec![(chunk.old_lines.clone(), chunk.new_lines.clone())];
    if let Some(candidate) = duplicated_edge_context_candidate(chunk) {
        candidates.push(candidate);
    }

    for (old_lines, new_lines) in candidates {
        if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof) {
            let matched_lines = lines[start..start + old_lines.len()].to_vec();
            let new_lines = preserve_matched_context_lines(&old_lines, &new_lines, &matched_lines);
            return Some((start, old_lines.len(), new_lines));
        }
        if old_lines.last().is_some_and(String::is_empty) {
            let old_lines = old_lines[..old_lines.len() - 1].to_vec();
            let new_lines = if new_lines.last().is_some_and(String::is_empty) {
                new_lines[..new_lines.len() - 1].to_vec()
            } else {
                new_lines
            };
            if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof) {
                let matched_lines = lines[start..start + old_lines.len()].to_vec();
                let new_lines =
                    preserve_matched_context_lines(&old_lines, &new_lines, &matched_lines);
                return Some((start, old_lines.len(), new_lines));
            }
        }
    }
    None
}

fn preserve_matched_context_lines(
    old_lines: &[String],
    new_lines: &[String],
    matched_lines: &[String],
) -> Vec<String> {
    let mut old_index = 0;
    new_lines
        .iter()
        .map(|line| {
            if old_index < old_lines.len() && lines_equivalent(line, &old_lines[old_index]) {
                let matched = matched_lines[old_index].clone();
                old_index += 1;
                matched
            } else {
                line.clone()
            }
        })
        .collect()
}

fn duplicated_edge_context_candidate(chunk: &UpdateChunk) -> Option<(Vec<String>, Vec<String>)> {
    if chunk.old_lines.len() != 2 || chunk.new_lines.len() <= chunk.old_lines.len() {
        return None;
    }
    let first_old = chunk.old_lines.first()?;
    let last_old = chunk.old_lines.last()?;
    if !lines_equivalent(first_old, last_old) {
        return None;
    }
    let first_new = chunk.new_lines.first()?;
    let last_new = chunk.new_lines.last()?;
    if !lines_equivalent(first_new, first_old) || !lines_equivalent(last_new, last_old) {
        return None;
    }
    Some((vec![last_old.clone()], chunk.new_lines[1..].to_vec()))
}

fn find_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
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

fn lines_equivalent(left: &str, right: &str) -> bool {
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
