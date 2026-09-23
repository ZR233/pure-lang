mod dynamic;
mod patch;
pub use dynamic::{NoteTool, registration};
mod schema;
mod search;

use pl_protocol::Result;
use serde_json::{Value, json};

use crate::deserialize_tool_input;
use pl_output::text::{line_end_byte_offset, line_start_byte_offset, logical_line_count};

use crate::tool_error;

pub use schema::*;

const MAX_SESSION_NOTE_BYTES: usize = 1024 * 1024;

const DEFAULT_READ_LINES: usize = 200;
const MAX_READ_LINES: usize = 500;
const DEFAULT_SEARCH_MATCH_LIMIT: usize = 100;
const MAX_SEARCH_MATCH_LIMIT: usize = 200;
const MAX_CONTEXT_LINES: usize = 20;

fn read_note(arguments: Value, note: &pl_protocol::SessionNote) -> Result<Value> {
    let input: ReadInput = deserialize_tool_input(TOOL_READ_SESSION_NOTE, arguments)?;
    let start_line = input.start_line.unwrap_or(1);
    if start_line == 0 {
        return Err(tool_error(TOOL_READ_SESSION_NOTE, "startLine is 1-based"));
    }
    let max_lines = input.max_lines.unwrap_or(DEFAULT_READ_LINES);
    if !(1..=MAX_READ_LINES).contains(&max_lines) {
        return Err(tool_error(
            TOOL_READ_SESSION_NOTE,
            format!("maxLines must be between 1 and {MAX_READ_LINES}"),
        ));
    }
    validate_expected_revision(
        TOOL_READ_SESSION_NOTE,
        input.expected_revision,
        note.revision,
    )?;
    let start = line_start_byte_offset(&note.content, start_line).map_err(|error| {
        tool_error(
            TOOL_READ_SESSION_NOTE,
            error
                .to_string()
                .replace("file length", "session note length"),
        )
    })?;
    let end = line_end_byte_offset(&note.content, start, Some(max_lines));
    let text = note.content[start..end].to_string();
    let returned_lines = logical_line_count(&text);
    let end_line = if returned_lines == 0 {
        start_line.saturating_sub(1)
    } else {
        start_line.saturating_add(returned_lines.saturating_sub(1))
    };
    let next_start_line = (end < note.content.len()).then_some(end_line.saturating_add(1));
    Ok(json!({
        "revision": note.revision,
        "contentHash": note.content_hash,
        "totalBytes": note.content.len(),
        "totalLines": logical_line_count(&note.content),
        "startLine": start_line,
        "endLine": end_line,
        "nextStartLine": next_start_line,
        "text": text,
    }))
}

fn search_note(arguments: Value, note: &pl_protocol::SessionNote) -> Result<Value> {
    let input: SearchInput = deserialize_tool_input(TOOL_SEARCH_SESSION_NOTE, arguments)?;
    let context_lines = input.context_lines.unwrap_or(0);
    if context_lines > MAX_CONTEXT_LINES {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            format!("contextLines must be between 0 and {MAX_CONTEXT_LINES}"),
        ));
    }
    let limit = input.limit.unwrap_or(DEFAULT_SEARCH_MATCH_LIMIT);
    if !(1..=MAX_SEARCH_MATCH_LIMIT).contains(&limit) {
        return Err(tool_error(
            TOOL_SEARCH_SESSION_NOTE,
            format!("limit must be between 1 and {MAX_SEARCH_MATCH_LIMIT}"),
        ));
    }
    let result = search::search(
        &note.content,
        search::SearchRequest {
            query: input.query.clone(),
            case_sensitive: input.case_sensitive.unwrap_or(true),
            literal: input.literal.unwrap_or(false),
            context_lines,
            limit,
            cursor: input.cursor,
            revision: note.revision,
        },
    )?;
    Ok(json!({
        "query": input.query,
        "revision": note.revision,
        "contentHash": note.content_hash,
        "matches": result.matches,
        "count": result.count,
        "nextCursor": result.next_cursor,
    }))
}

fn note_result(status: &str, note: &pl_protocol::SessionNote) -> Value {
    json!({
        "status": status,
        "revision": note.revision,
        "contentHash": note.content_hash,
        "totalBytes": note.content.len(),
        "totalLines": logical_line_count(&note.content),
    })
}

fn validate_expected_revision(tool: &str, expected: Option<u64>, current: u64) -> Result<()> {
    if let Some(expected) = expected
        && expected != current
    {
        return Err(tool_error(
            tool,
            format!("session note revision mismatch: expected {expected}, current {current}"),
        ));
    }
    Ok(())
}
