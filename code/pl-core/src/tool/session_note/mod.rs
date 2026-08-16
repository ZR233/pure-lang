mod patch;
mod schema;
mod search;

use futures::FutureExt;
use pl_model::ToolSchema;
use pl_protocol::{PureError, Result};
use serde_json::{Value, json};

use crate::tool::text_document::{
    line_end_byte_offset, line_start_byte_offset, logical_line_count,
};
use crate::tool::{
    BoxFuture, Tool, ToolContext, ToolExecutionResult, ToolInput, ToolOutput,
    deserialize_tool_input,
};
use crate::turn::ToolEffect;

pub use schema::*;

const DEFAULT_READ_LINES: usize = 200;
const MAX_READ_LINES: usize = 500;
const DEFAULT_SEARCH_MATCH_LIMIT: usize = 100;
const MAX_SEARCH_MATCH_LIMIT: usize = 200;
const MAX_CONTEXT_LINES: usize = 20;

#[derive(Debug, Clone)]
pub struct SessionNoteTool {
    kind: SessionNoteToolKind,
}

impl SessionNoteTool {
    pub const fn new(kind: SessionNoteToolKind) -> Self {
        Self { kind }
    }
}

impl Tool for SessionNoteTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.kind.supports_parallel_tool_calls()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        async move {
            let result = match self.kind {
                SessionNoteToolKind::Read => read_note(input.arguments, &context)?,
                SessionNoteToolKind::Search => search_note(input.arguments, &context)?,
                SessionNoteToolKind::Write => write_note(input.arguments, &context)?,
                SessionNoteToolKind::ApplyPatch => {
                    apply_note_patch(input.arguments, &context).await?
                }
            };
            ToolExecutionResult::<Value>::json(result).map(ToolExecutionResult::into_tool_output)
        }
        .boxed()
    }

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

fn read_note(arguments: Value, context: &ToolContext) -> Result<Value> {
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
    let note = context.working_set.session_note();
    validate_expected_revision(
        TOOL_READ_SESSION_NOTE,
        input.expected_revision,
        note.revision,
    )?;
    let start = line_start_byte_offset(&note.content, start_line).map_err(|error| {
        tool_error(
            TOOL_READ_SESSION_NOTE,
            error.replace("file length", "session note length"),
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

fn search_note(arguments: Value, context: &ToolContext) -> Result<Value> {
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
    let note = context.working_set.session_note();
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

fn write_note(arguments: Value, context: &ToolContext) -> Result<Value> {
    let input: WriteInput = deserialize_tool_input(TOOL_WRITE_SESSION_NOTE, arguments)?;
    let expected_revision = input.expected_revision();
    let note = context
        .working_set
        .replace_session_note(expected_revision, input.content)
        .map_err(|error| tool_error(TOOL_WRITE_SESSION_NOTE, error))?;
    Ok(note_result("written", &note))
}

async fn apply_note_patch(arguments: Value, context: &ToolContext) -> Result<Value> {
    let input: ApplyPatchInput = deserialize_tool_input(TOOL_APPLY_SESSION_NOTE_PATCH, arguments)?;
    let expected_revision = input.expected_revision();
    let current = context.working_set.session_note();
    validate_expected_revision(
        TOOL_APPLY_SESSION_NOTE_PATCH,
        Some(expected_revision),
        current.revision,
    )?;
    let staged = patch::apply(
        (!current.content.is_empty()).then_some(current.content),
        &input.patch,
    )
    .await?;
    let note = context
        .working_set
        .replace_session_note(expected_revision, staged)
        .map_err(|error| tool_error(TOOL_APPLY_SESSION_NOTE_PATCH, error))?;
    Ok(note_result("patched", &note))
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

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{Tool, WorkspaceAccess};
    use crate::{AgentSession, TurnOptions};

    fn context() -> ToolContext {
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        }
    }

    fn input(arguments: Value) -> ToolInput {
        ToolInput {
            arguments,
            session_id: "session-1".to_string(),
            tool_id: "call-1".to_string(),
            revision_base: 0,
        }
    }

    async fn execute(kind: SessionNoteToolKind, arguments: Value, context: ToolContext) -> Value {
        let output = SessionNoteTool::new(kind)
            .execute(input(arguments), context)
            .await
            .unwrap();
        serde_json::from_str(&output.description).unwrap()
    }

    #[tokio::test]
    async fn writes_and_reads_only_the_requested_unicode_lines() {
        let context = context();
        let written = execute(
            SessionNoteToolKind::Write,
            json!({"content": "一\ntwo\n三\nfour", "expectedRevision": 0}),
            context.clone(),
        )
        .await;
        let read = execute(
            SessionNoteToolKind::Read,
            json!({"startLine": 2, "maxLines": 2, "expectedRevision": 1}),
            context,
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
        let context = context();
        execute(
            SessionNoteToolKind::Write,
            json!({
                "content": "before\nTODO first\nmiddle\ntodo second\nafter",
                "expectedRevision": 0
            }),
            context.clone(),
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
            context.clone(),
        )
        .await;
        assert_eq!(first["count"], 1);
        assert_eq!(first["matches"][0]["line"], 2);
        assert_eq!(first["matches"][0]["before"][0]["text"], "before");
        let cursor = first["nextCursor"].as_str().unwrap().to_string();

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "TODO changed", "expectedRevision": 1}),
            context.clone(),
        )
        .await;
        let error = SessionNoteTool::new(SessionNoteToolKind::Search)
            .execute(
                input(json!({
                    "query": "todo",
                    "literal": true,
                    "caseSensitive": false,
                    "contextLines": 1,
                    "limit": 1,
                    "cursor": cursor
                })),
                context,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("cursor is stale"));
    }

    #[tokio::test]
    async fn search_treats_blank_cursor_as_the_first_page() {
        let context = context();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "TODO first\nTODO second", "expectedRevision": 0}),
            context.clone(),
        )
        .await;

        let result = execute(
            SessionNoteToolKind::Search,
            json!({"query": "TODO", "literal": true, "limit": 1, "cursor": "  "}),
            context,
        )
        .await;

        assert_eq!(result["count"], 1);
        assert_eq!(result["matches"][0]["line"], 1);
        assert!(result["nextCursor"].is_string());
    }

    #[tokio::test]
    async fn search_rejects_page_numbers_with_actionable_cursor_guidance() {
        let error = SessionNoteTool::new(SessionNoteToolKind::Search)
            .execute(input(json!({"query": "TODO", "cursor": "0"})), context())
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
        let context = context();
        for query in ["(".to_string(), "x".repeat(4097)] {
            let error = SessionNoteTool::new(SessionNoteToolKind::Search)
                .execute(input(json!({"query": query})), context.clone())
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
        let context = context();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "old\n", "expectedRevision": 0}),
            context.clone(),
        )
        .await;
        let error = SessionNoteTool::new(SessionNoteToolKind::ApplyPatch)
            .execute(
                input(json!({
                    "expectedRevision": 1,
                    "patch": "*** Begin Patch\n*** Update File: session-note.md\n@@\n-old\n+new\n*** Update File: other.md\n@@\n-missing\n+value\n*** End Patch"
                })),
                context.clone(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("session-note.md"));
        let note = context.working_set.session_note();
        assert_eq!(note.revision, 1);
        assert_eq!(note.content, "old\n");
    }

    #[tokio::test]
    async fn patch_can_create_update_and_clear_the_note() {
        let context = context();
        let added = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 0,
                "patch": "*** Begin Patch\n*** Add File: session-note.md\n+first\n*** End Patch"
            }),
            context.clone(),
        )
        .await;
        let updated = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 1,
                "patch": "*** Begin Patch\n*** Update File: session-note.md\n@@\n-first\n+second\n*** End Patch"
            }),
            context.clone(),
        )
        .await;
        let cleared = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 2,
                "patch": "*** Begin Patch\n*** Delete File: session-note.md\n*** End Patch"
            }),
            context.clone(),
        )
        .await;

        assert_eq!(added["revision"], 1);
        assert_eq!(updated["revision"], 2);
        assert_eq!(cleared["revision"], 3);
        assert_eq!(context.working_set.session_note().content, "");

        let recreated = execute(
            SessionNoteToolKind::ApplyPatch,
            json!({
                "expectedRevision": 3,
                "patch": "*** Begin Patch\n*** Add File: session-note.md\n+third\n*** End Patch"
            }),
            context.clone(),
        )
        .await;
        assert_eq!(recreated["revision"], 4);
        assert_eq!(context.working_set.session_note().content, "third\n");
    }

    #[tokio::test]
    async fn patch_rejects_moves_and_oversized_staged_content_atomically() {
        let context = context();
        execute(
            SessionNoteToolKind::Write,
            json!({"content": "old\n", "expectedRevision": 0}),
            context.clone(),
        )
        .await;
        let moved = SessionNoteTool::new(SessionNoteToolKind::ApplyPatch)
            .execute(
                input(json!({
                    "expectedRevision": 1,
                    "patch": "*** Begin Patch\n*** Update File: session-note.md\n*** Move to: moved.md\n@@\n-old\n+new\n*** End Patch"
                })),
                context.clone(),
            )
            .await
            .unwrap_err();
        assert!(moved.to_string().contains("do not support moves"));

        let addition = format!("+{}", "x".repeat(crate::MAX_SESSION_NOTE_BYTES + 1));
        let oversized_patch = format!(
            "*** Begin Patch\n*** Update File: session-note.md\n@@\n-old\n{addition}\n*** End Patch"
        );
        let oversized = SessionNoteTool::new(SessionNoteToolKind::ApplyPatch)
            .execute(
                input(json!({"expectedRevision": 1, "patch": oversized_patch})),
                context.clone(),
            )
            .await
            .unwrap_err();

        assert!(oversized.to_string().contains("exceeds"));
        assert_eq!(context.working_set.session_note().revision, 1);
        assert_eq!(context.working_set.session_note().content, "old\n");
    }

    #[tokio::test]
    async fn rejects_oversized_notes_and_revision_conflicts() {
        let context = context();
        let boundary = execute(
            SessionNoteToolKind::Write,
            json!({
                "content": "x".repeat(crate::MAX_SESSION_NOTE_BYTES),
                "expectedRevision": 0
            }),
            context.clone(),
        )
        .await;
        assert_eq!(boundary["totalBytes"], crate::MAX_SESSION_NOTE_BYTES);

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "", "expectedRevision": 1}),
            context.clone(),
        )
        .await;
        let oversized = SessionNoteTool::new(SessionNoteToolKind::Write)
            .execute(
                input(json!({
                    "content": "x".repeat(crate::MAX_SESSION_NOTE_BYTES + 1),
                    "expectedRevision": 2
                })),
                context.clone(),
            )
            .await
            .unwrap_err();
        assert!(oversized.to_string().contains("exceeds"));

        execute(
            SessionNoteToolKind::Write,
            json!({"content": "current", "expectedRevision": 2}),
            context.clone(),
        )
        .await;
        let conflict = SessionNoteTool::new(SessionNoteToolKind::Write)
            .execute(
                input(json!({"content": "stale", "expectedRevision": 2})),
                context,
            )
            .await
            .unwrap_err();
        assert!(conflict.to_string().contains("revision mismatch"));
    }
}
