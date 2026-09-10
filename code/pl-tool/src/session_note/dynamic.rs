//! Session note access through opaque Thread extensions instead of core business state.
use super::*;
use pl_core::context::{ContextContent, OpaquePayload};
use pl_core::thread::extensions::ExtensionMutation;
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
};
use std::sync::Arc;

const NOTE_FORMAT: &str = "pl.tool.session-note";

/// A note operation with no mutable state outside its owning Thread.
#[derive(Debug)]
pub struct NoteTool {
    kind: SessionNoteToolKind,
}

/// Registers one operation with explicit state-update authority only for writes and patches.
///
/// # Errors
/// Propagates an invalid tool identity.
pub fn registration(
    kind: SessionNoteToolKind,
    declaration: OpaquePayload,
) -> std::result::Result<Registration, RegistryError> {
    let registration = Registration::new(kind.name().into(), declaration, NoteTool { kind })?;
    Ok(match kind {
        SessionNoteToolKind::Read | SessionNoteToolKind::Search => registration,
        SessionNoteToolKind::Write | SessionNoteToolKind::ApplyPatch => {
            registration.with_extension_updates()
        }
    })
}

impl Tool for NoteTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> std::result::Result<ToolOutput, ToolError> {
        self.execute_note(input, context)
            .await
            .map_err(ToolError::new)
    }
}

impl NoteTool {
    /// Constructs an operation over the invocation's immutable extension snapshot.
    pub fn new(kind: SessionNoteToolKind) -> Self {
        Self { kind }
    }
    async fn execute_note(&self, input: OpaquePayload, context: CallContext) -> Result<ToolOutput> {
        let arguments: Value = serde_json::from_str(input.content())?;
        let previous = context.extensions.get(NOTE_FORMAT);
        if previous.is_some_and(|record| {
            record.payload.format() != NOTE_FORMAT || record.payload.version() != 1
        }) {
            return Err(tool_error(
                self.kind.name(),
                "unsupported saved note format or version",
            ));
        }
        let mut note = pl_protocol::SessionNote {
            revision: previous.map_or(0, |record| record.revision),
            content: previous
                .map_or_else(String::new, |record| record.payload.content().to_owned()),
            content_hash: String::new(),
            updated_at: 0,
        };
        note.content_hash = pl_core::context::content_hash(note.content.as_bytes());
        let mut mutations = Vec::new();
        let result = match self.kind {
            SessionNoteToolKind::Read => read_note(arguments, &note)?,
            SessionNoteToolKind::Search => search_note(arguments, &note)?,
            SessionNoteToolKind::Write | SessionNoteToolKind::ApplyPatch => {
                let (expected, content, status) = match self.kind {
                    SessionNoteToolKind::Write => {
                        let input: WriteInput =
                            deserialize_tool_input(self.kind.name(), arguments)?;
                        (input.expected_revision(), input.content, "written")
                    }
                    SessionNoteToolKind::ApplyPatch => {
                        let input: ApplyPatchInput =
                            deserialize_tool_input(self.kind.name(), arguments)?;
                        let content = patch::apply(
                            (!note.content.is_empty()).then(|| note.content.clone()),
                            &input.patch,
                        )
                        .await?;
                        (input.expected_revision(), content, "patched")
                    }
                    SessionNoteToolKind::Read | SessionNoteToolKind::Search => {
                        unreachable!("write operation selected above")
                    }
                };
                validate_expected_revision(self.kind.name(), Some(expected), note.revision)?;
                if content.len() > MAX_SESSION_NOTE_BYTES {
                    return Err(tool_error(
                        self.kind.name(),
                        "session note exceeds 1048576 bytes",
                    ));
                }
                if content != note.content {
                    note.revision = context
                        .extension_sequence
                        .checked_add(1)
                        .ok_or_else(|| tool_error(self.kind.name(), "note revision exhausted"))?;
                    note.content = content;
                    note.content_hash = pl_core::context::content_hash(note.content.as_bytes());
                    mutations.push(ExtensionMutation::Put {
                        id: NOTE_FORMAT.into(),
                        expected_revision: previous.map(|record| record.revision),
                        payload: OpaquePayload::new(NOTE_FORMAT, 1, note.content.clone())
                            .map_err(|error| tool_error(self.kind.name(), error))?,
                    });
                }
                note_result(status, &note)
            }
        };
        let result = serde_json::to_string(&result)?;
        Ok(ToolOutput::new(
            OpaquePayload::text(result.clone()),
            vec![ContextContent::Text {
                text: Arc::from(result),
            }],
        )
        .with_extension_mutations(mutations))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn note_write_preserves_original_text_and_read_returns_only_the_requested_page() {
        let text = "  第一行\r\nsecond line\n";
        let context = CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Default::default(),
            catalog: Arc::from([]),
            extension_sequence: 7,
        };
        let input = OpaquePayload::new(
            "application/json",
            1,
            serde_json::json!({"expectedRevision":0,"content":text}).to_string(),
        )
        .unwrap();
        let output = NoteTool {
            kind: SessionNoteToolKind::Write,
        }
        .execute(input, context.clone())
        .await
        .unwrap();
        let ExtensionMutation::Put {
            payload,
            expected_revision,
            ..
        } = &output.extension_mutations()[0]
        else {
            panic!("note write");
        };
        assert_eq!(*expected_revision, None);
        assert_eq!(payload.content(), text);
        let receipt: Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(receipt["revision"], 8);
        let mut read_context = context;
        read_context.extensions = Arc::new(std::collections::BTreeMap::from([(
            NOTE_FORMAT.into(),
            pl_core::thread::extensions::ExtensionRecord {
                revision: 8,
                payload: payload.clone(),
            },
        )]));
        let input = OpaquePayload::new(
            "application/json",
            1,
            r#"{"startLine":2,"maxLines":1,"expectedRevision":8}"#,
        )
        .unwrap();
        let output = NoteTool {
            kind: SessionNoteToolKind::Read,
        }
        .execute(input, read_context)
        .await
        .unwrap();
        assert!(output.extension_mutations().is_empty());
        let page: Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(page["text"], "second line\n");
    }
}
