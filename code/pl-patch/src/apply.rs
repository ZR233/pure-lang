//! 通过 `PatchBackend` 执行解析后的 hunk 并产出 `PatchOutcome`。

use std::path::Path;

use crate::backend::PatchBackend;
use crate::error::{PATCH_RETRY_GUIDANCE, PatchError, PatchResult};
use crate::matching::{
    find_preserved_json_context_sequence, find_sequence, lines_equivalent,
    supports_preserved_json_context,
};
use crate::outcome::{AppliedChange, PatchOutcome};
use crate::parse::{Hunk, UpdateChunk, parse_patch};

pub async fn apply_patch(
    patch: &str,
    backend: &(impl PatchBackend + Sync),
) -> PatchResult<PatchOutcome> {
    let hunks = parse_patch(patch)?;
    let mut outcome = PatchOutcome::default();

    for hunk in hunks {
        if let Err(error) = apply_hunk(hunk, backend, &mut outcome).await {
            return Err(PatchError::new(format!(
                "{error}{}",
                outcome.failure_suffix(backend)
            )));
        }
    }

    Ok(outcome)
}

async fn apply_hunk(
    hunk: Hunk,
    backend: &(impl PatchBackend + Sync),
    outcome: &mut PatchOutcome,
) -> PatchResult<()> {
    match hunk {
        Hunk::Add { path, content } => {
            let target = backend.resolve_for_write(&path).await?;
            backend.reject_symlink_write(&target).await?;
            let overwritten_content = backend.read_optional_text(&target).await?;
            backend.create_parent_dirs(&target).await?;
            write_text(backend, &target, &content, outcome).await?;
            outcome.record(AppliedChange::Add {
                path: target,
                content,
                overwritten_content,
            });
        }
        Hunk::Delete { path } => {
            let target = backend.resolve_existing(&path).await?;
            backend.reject_symlink_write(&target).await?;
            backend.ensure_file(&target).await?;
            let content = backend.read_to_string(&target).await?;
            backend.remove_file(&target).await?;
            outcome.record(AppliedChange::Delete {
                path: target,
                content,
            });
        }
        Hunk::Update {
            path,
            move_path,
            chunks,
        } => {
            let source = backend.resolve_existing(&path).await?;
            backend.reject_symlink_write(&source).await?;
            let old_content = backend.read_to_string(&source).await?;
            let new_content = if chunks.is_empty() {
                old_content.clone()
            } else {
                apply_chunks(&old_content, &source, &chunks)?
            };

            if let Some(move_path) = move_path {
                let target = backend.resolve_for_write(&move_path).await?;
                backend.reject_symlink_write(&target).await?;
                if target == source {
                    write_text(backend, &source, &new_content, outcome).await?;
                    outcome.record(AppliedChange::Update {
                        path: source,
                        old_content,
                        new_content,
                    });
                    return Ok(());
                }
                let overwritten_target_content = backend.read_optional_text(&target).await?;
                backend.create_parent_dirs(&target).await?;
                write_text(backend, &target, &new_content, outcome).await?;
                let target_change_index = outcome.record(AppliedChange::Add {
                    path: target.clone(),
                    content: new_content.clone(),
                    overwritten_content: overwritten_target_content.clone(),
                });
                backend.remove_file(&source).await?;
                outcome.replace(
                    target_change_index,
                    AppliedChange::Move {
                        source,
                        target,
                        old_content,
                        new_content,
                        overwritten_target_content,
                    },
                );
            } else {
                write_text(backend, &source, &new_content, outcome).await?;
                outcome.record(AppliedChange::Update {
                    path: source,
                    old_content,
                    new_content,
                });
            }
        }
    }
    Ok(())
}

async fn write_text(
    backend: &(impl PatchBackend + Sync),
    path: &Path,
    content: &str,
    outcome: &mut PatchOutcome,
) -> PatchResult<()> {
    backend
        .write_text(path, content)
        .await
        .inspect_err(|_| outcome.mark_inexact())
}

fn apply_chunks(content: &str, path: &Path, chunks: &[UpdateChunk]) -> PatchResult<String> {
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut cursor = 0;
    let mut replacements = Vec::new();
    let allow_preserved_json_context = supports_preserved_json_context(path);

    for chunk in chunks {
        if let Some(context) = &chunk.context {
            let Some(context_index) =
                find_sequence(&lines, std::slice::from_ref(context), cursor, false)
            else {
                return Err(PatchError::new(format!(
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

        let Some((start, old_len, new_lines)) =
            find_chunk_replacement(&lines, chunk, cursor, allow_preserved_json_context)
        else {
            return Err(PatchError::new(format!(
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
    allow_preserved_json_context: bool,
) -> Option<(usize, usize, Vec<String>)> {
    let mut candidates = vec![(chunk.old_lines.clone(), chunk.new_lines.clone())];
    if let Some(candidate) = duplicated_edge_context_candidate(chunk) {
        candidates.push(candidate);
    }

    for (old_lines, new_lines) in candidates {
        if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof).or_else(|| {
            if allow_preserved_json_context {
                find_preserved_json_context_sequence(
                    lines, &old_lines, &new_lines, cursor, chunk.eof,
                )
            } else {
                None
            }
        }) {
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
            if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof).or_else(|| {
                if allow_preserved_json_context {
                    find_preserved_json_context_sequence(
                        lines, &old_lines, &new_lines, cursor, chunk.eof,
                    )
                } else {
                    None
                }
            }) {
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
    let mut old_search_start = 0;
    new_lines
        .iter()
        .map(|line| {
            let Some(relative_index) = old_lines[old_search_start..]
                .iter()
                .position(|old_line| lines_equivalent(line, old_line))
            else {
                return line.clone();
            };
            let old_index = old_search_start + relative_index;
            old_search_start = old_index + 1;
            matched_lines[old_index].clone()
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
