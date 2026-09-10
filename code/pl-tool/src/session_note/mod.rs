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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::test_support::input;
    use pl_core::{
        thread::extensions::{ExtensionMutation, ExtensionRecord},
        tool::{
            ToolOutput,
            opaque::{Tool, ToolError},
        },
    };
    #[derive(Default)]
    struct TestRuntime {
        context: std::sync::Mutex<Option<pl_core::tool::opaque::CallContext>>,
    }
    fn runtime() -> TestRuntime {
        TestRuntime::default()
    }
    impl TestRuntime {
        fn session_note(&self) -> pl_protocol::SessionNote {
            let guard = self.context.lock().unwrap();
            let record = guard
                .as_ref()
                .and_then(|c| c.extensions.get("pl.tool.session-note"));
            pl_protocol::SessionNote {
                revision: record.map_or(0, |r| r.revision),
                content: record.map_or_else(String::new, |r| r.payload.content().to_owned()),
                content_hash: String::new(),
                updated_at: 0,
            }
        }
    }
    async fn execute(kind: SessionNoteToolKind, arguments: Value, runtime: &TestRuntime) -> Value {
        let output = try_execute(kind, arguments, runtime).await.unwrap();
        serde_json::from_str(output.payload().content()).unwrap()
    }
    async fn try_execute(
        kind: SessionNoteToolKind,
        arguments: Value,
        runtime: &TestRuntime,
    ) -> std::result::Result<ToolOutput, ToolError> {
        let context = runtime
            .context
            .lock()
            .unwrap()
            .get_or_insert_with(crate::test_support::thread_context)
            .clone();
        let tool = dynamic::NoteTool::new(kind);
        let output = tool.execute(input(arguments), context.clone()).await?;
        let mut next = context;
        for mutation in output.extension_mutations() {
            match mutation {
                ExtensionMutation::Put { id, payload, .. } => {
                    next.extension_sequence += 1;
                    std::sync::Arc::make_mut(&mut next.extensions).insert(
                        id.clone(),
                        ExtensionRecord {
                            revision: next.extension_sequence,
                            payload: payload.clone(),
                        },
                    );
                }
                ExtensionMutation::Delete { .. } => {
                    panic!("note operations do not delete extension records")
                }
            }
        }
        *runtime.context.lock().unwrap() = Some(next);
        Ok(output)
    }

    #[tokio::test]
    async fn writes_and_reads_only_the_requested_unicode_lines() {
        let runtime = runtime();
        let written = execute(
            SessionNoteToolKind::Write,
            json!({"content": "一\ntwo\n三\nfour", "expectedRevision": 0}),
            &runtime,
        )
        .await;
        let read = execute(
            SessionNoteToolKind::Read,
            json!({"startLine": 2, "maxLines": 2, "expectedRevision": 1}),
            &runtime,
        )
        .await;

        assert_eq!(written["revision"], 1);
        assert_eq!(read["text"], "two\n三\n");
        assert_eq!(read["startLine"], 2);
        assert_eq!(read["endLine"], 3);
        assert_eq!(read["nextStartLine"], 4);
    }

    #[tokio::test]
    async fn searches_with_context_and_rejects_stale_cursor() {
        let runtime = runtime();
        execute(
            SessionNoteToolKind::Write,
            json!({
                "content": "before\nTODO first\nmiddle\ntodo second\nafter",
                "expectedRevision": 0
            }),
            &runtime,
        )
        .await;
        let first = execute(
            SessionNoteToolKind::Search,
            json!({
                "query": "todo",
                "literal": true,
                "caseSensitive": false,
                "contextLines": 1,
                "limit": 1
            }),
            &runtime,
        )
        .await;
        assert_eq!(first["count"], 1);
        assert_eq!(first["matches"][0]["line"], 2);
        assert_eq!(first["matches"][0]["before"][0]["text"], "before");
        let cursor = first["nextCursor"].as_str().unwrap().to_string();

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "TODO changed", "expectedRevision": 1}),
            &runtime,
        )
        .await;
        let error = try_execute(
            SessionNoteToolKind::Search,
            json!({
                "query": "todo",
                "literal": true,
                "caseSensitive": false,
                "contextLines": 1,
                "limit": 1,
                "cursor": cursor
            }),
            &runtime,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("cursor is stale"));
    }

    #[tokio::test]
    async fn search_treats_blank_cursor_as_the_first_page() {
        let runtime = runtime();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "TODO first\nTODO second", "expectedRevision": 0}),
            &runtime,
        )
        .await;

        let result = execute(
            SessionNoteToolKind::Search,
            json!({"query": "TODO", "literal": true, "limit": 1, "cursor": "  "}),
            &runtime,
        )
        .await;

        assert_eq!(result["count"], 1);
        assert_eq!(result["matches"][0]["line"], 1);
        assert!(result["nextCursor"].is_string());
    }

    #[tokio::test]
    async fn search_rejects_page_numbers_with_actionable_cursor_guidance() {
        let runtime = runtime();
        let error = try_execute(
            SessionNoteToolKind::Search,
            json!({"query": "TODO", "cursor": "0"}),
            &runtime,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("omit cursor on the first page"));
        assert!(error.to_string().contains("exact nextCursor"));
    }

    #[tokio::test]
    async fn search_supports_regex_crlf_and_more_than_two_full_pages() {
        let content = (1..=450)
            .map(|line| format!("Item-{line:03}\r\n"))
            .collect::<String>();
        let request = |cursor| search::SearchRequest {
            query: "Item-[0-9]{3}".to_string(),
            case_sensitive: true,
            literal: false,
            context_lines: 0,
            limit: 200,
            cursor,
            revision: 1,
        };
        let first = search::search(&content, request(None)).unwrap();
        let second = search::search(&content, request(first.next_cursor)).unwrap();
        let third = search::search(&content, request(second.next_cursor)).unwrap();

        assert_eq!(first.count, 200);
        assert_eq!(second.count, 200);
        assert_eq!(third.count, 50);
        let last = serde_json::to_value(&third.matches[49]).unwrap();
        assert_eq!(last["line"], 450);
        assert_eq!(last["text"], "Item-450");
        assert_eq!(third.next_cursor, None);
    }

    #[tokio::test]
    async fn search_rejects_invalid_and_oversized_queries() {
        let runtime = runtime();
        for query in ["(".to_string(), "x".repeat(4097)] {
            let error = try_execute(
                SessionNoteToolKind::Search,
                json!({"query": query}),
                &runtime,
            )
            .await
            .unwrap_err();
            assert!(
                error.to_string().contains("invalid search query")
                    || error.to_string().contains("query exceeds")
            );
        }
    }

    #[tokio::test]
    async fn failed_multi_hunk_patch_does_not_change_the_note() {
        let runtime = runtime();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "old\n", "expectedRevision": 0}),
            &runtime,
        )
        .await;
        let error = try_execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                    "expectedRevision": 1,
                    "patch": "*** Begin Patch\n*** Update File: session-note.md\n@@\n-old\n+new\n*** Update File: other.md\n@@\n-missing\n+value\n*** End Patch"
                }),
            &runtime,
        )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("session-note.md"));
        let note = runtime.session_note();
        assert_eq!(note.revision, 1);
        assert_eq!(note.content, "old\n");
    }

    #[tokio::test]
    async fn patch_can_create_update_and_clear_the_note() {
        let runtime = runtime();
        let added = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 0,
                "patch": "*** Begin Patch\n*** Add File: session-note.md\n+first\n*** End Patch"
            }),
            &runtime,
        )
        .await;
        let updated = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 1,
                "patch": "*** Begin Patch\n*** Update File: session-note.md\n@@\n-first\n+second\n*** End Patch"
            }),
            &runtime,
        )
        .await;
        let cleared = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 2,
                "patch": "*** Begin Patch\n*** Delete File: session-note.md\n*** End Patch"
            }),
            &runtime,
        )
        .await;

        assert_eq!(added["revision"], 1);
        assert_eq!(updated["revision"], 2);
        assert_eq!(cleared["revision"], 3);
        assert_eq!(runtime.session_note().content, "");

        let recreated = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 3,
                "patch": "*** Begin Patch\n*** Add File: session-note.md\n+third\n*** End Patch"
            }),
            &runtime,
        )
        .await;
        assert_eq!(recreated["revision"], 4);
        assert_eq!(runtime.session_note().content, "third\n");
    }

    #[tokio::test]
    async fn patch_rejects_moves_and_oversized_staged_content_atomically() {
        let runtime = runtime();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "old\n", "expectedRevision": 0}),
            &runtime,
        )
        .await;
        let moved = try_execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                    "expectedRevision": 1,
                    "patch": "*** Begin Patch\n*** Update File: session-note.md\n*** Move to: moved.md\n@@\n-old\n+new\n*** End Patch"
                }),
            &runtime,
        )
            .await
            .unwrap_err();
        assert!(moved.to_string().contains("do not support moves"));

        let addition = format!("+{}", "x".repeat(MAX_SESSION_NOTE_BYTES + 1));
        let oversized_patch = format!(
            "*** Begin Patch\n*** Update File: session-note.md\n@@\n-old\n{addition}\n*** End Patch"
        );
        let oversized = try_execute(
            SessionNoteToolKind::ApplyPatch,
            json!({"expectedRevision": 1, "patch": oversized_patch}),
            &runtime,
        )
        .await
        .unwrap_err();

        assert!(oversized.to_string().contains("exceeds"));
        assert_eq!(runtime.session_note().revision, 1);
        assert_eq!(runtime.session_note().content, "old\n");
    }

    #[tokio::test]
    async fn rejects_oversized_notes_and_revision_conflicts() {
        let runtime = runtime();
        let boundary = execute(
            SessionNoteToolKind::Write,
            json!({
                "content": "x".repeat(MAX_SESSION_NOTE_BYTES),
                "expectedRevision": 0
            }),
            &runtime,
        )
        .await;
        assert_eq!(boundary["totalBytes"], MAX_SESSION_NOTE_BYTES);

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "", "expectedRevision": 1}),
            &runtime,
        )
        .await;
        let oversized = try_execute(
            SessionNoteToolKind::Write,
            json!({
                "content": "x".repeat(MAX_SESSION_NOTE_BYTES + 1),
                "expectedRevision": 2
            }),
            &runtime,
        )
        .await
        .unwrap_err();
        assert!(oversized.to_string().contains("exceeds"));

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "current", "expectedRevision": 2}),
            &runtime,
        )
        .await;
        let conflict = try_execute(
            SessionNoteToolKind::Write,
            json!({"content": "stale", "expectedRevision": 2}),
            &runtime,
        )
        .await
        .unwrap_err();
        assert!(conflict.to_string().contains("revision mismatch"));
    }
}
