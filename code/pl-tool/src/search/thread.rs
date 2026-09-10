//! Independent search over frozen generic context, with no product session access.
use super::{
    ASSISTANT_CONTEXT_CHAR_LIMIT, TOOL_WEB_SEARCH, WEB_SEARCH_DESCRIPTION, WebSearchClient,
};
use pl_core::{
    context::{ContextContent, ContextSnapshot, ContextSource, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use pl_protocol::search::{SearchCommands, SearchRequest, SearchSettings};
use serde_json::Value;
use std::sync::Arc;

/// Parameters resolved by Studio before tool construction; this tool performs no configuration IO.
#[derive(Debug)]
pub struct ThreadSearchOptions {
    pub model: String,
    pub settings: SearchSettings,
    pub max_output_tokens: Option<u64>,
}

/// A Thread-local search instance sharing only its immutable HTTP client configuration.
#[derive(Debug)]
pub struct ThreadWebSearchTool {
    client: WebSearchClient,
    options: ThreadSearchOptions,
}

impl ThreadWebSearchTool {
    /// Binds the explicit search service and its request parameters.
    pub fn new(client: WebSearchClient, options: ThreadSearchOptions) -> Self {
        Self { client, options }
    }

    /// Stable declaration, independent of conversation, current time and search results.
    pub fn declaration() -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            TOOL_WEB_SEARCH,
            WEB_SEARCH_DESCRIPTION,
            schemars::schema_for!(SearchCommands).to_value(),
        )
    }

    /// Transfers an executor without granting any framework control permissions.
    ///
    /// # Errors
    /// Returns an invalid tool identity error.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        Registration::new(TOOL_WEB_SEARCH.into(), declaration, self)
    }
}

impl Tool for ThreadWebSearchTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                TOOL_WEB_SEARCH,
                "unsupported argument encoding",
            )));
        }
        let commands: SearchCommands =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let request = SearchRequest {
            id: format!("{}:{}", context.turn_id, context.call_id),
            model: self.options.model.clone(),
            input: recent_context(&context.context),
            commands,
            settings: self.options.settings.clone(),
            max_output_tokens: self.options.max_output_tokens,
        };
        let response = self.client.search(&request).await.map_err(ToolError::new)?;
        let encoded = serde_json::to_string(&response).map_err(ToolError::new)?;
        let preview = pl_output::bounded_text(&response.output, 12 * 1024, 0);
        let text = if preview.truncated {
            format!(
                "{}\n[Search preview omitted {} bytes; full result retained in history.]",
                preview.text, preview.bytes_omitted
            )
        } else {
            preview.text
        };
        Ok(ToolOutput::new(
            OpaquePayload::new("pl.tool.web-search", 1, encoded).map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(text),
            }],
        ))
    }
}

fn recent_context(context: &ContextSnapshot) -> Option<Vec<Value>> {
    let start = context
        .records
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, record)| record.source == ContextSource::User)
        .take(2)
        .last()?
        .0;
    let mut remaining = ASSISTANT_CONTEXT_CHAR_LIMIT;
    let messages = context.records[start..]
        .iter()
        .filter_map(|record| {
            let role = match record.source {
                ContextSource::User => "user",
                ContextSource::Assistant if record.tool_calls.is_empty() => "assistant",
                ContextSource::Assistant
                | ContextSource::Instruction
                | ContextSource::Runtime { .. }
                | ContextSource::ToolResult { .. } => return None,
            };
            let mut text = record
                .content
                .iter()
                .filter_map(|part| match part {
                    ContextContent::Text { text } => Some(text.as_ref()),
                    ContextContent::Resource { .. } | ContextContent::Opaque { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if role == "assistant" {
                text = text.chars().take(remaining).collect();
                remaining = remaining.saturating_sub(text.chars().count());
            }
            if text.is_empty() {
                None
            } else {
                Some(serde_json::json!({"role":role,"content":text}))
            }
        })
        .collect::<Vec<_>>();
    (!messages.is_empty()).then_some(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::ContextRecord;
    use pretty_assertions::assert_eq;

    #[test]
    fn search_context_uses_recent_conversation_text_without_privileged_or_opaque_material() {
        let record = |id: &str, source, text: &str| ContextRecord {
            id: id.into(),
            turn_id: None,
            source,
            content: vec![ContextContent::Text {
                text: Arc::from(text),
            }],
            tool_calls: Vec::new(),
        };
        let mut records = vec![
            record("system", ContextSource::Instruction, "private instructions"),
            record("old", ContextSource::User, "old question"),
            record("first", ContextSource::User, "recent question"),
            record("answer", ContextSource::Assistant, "recent answer"),
            record(
                "runtime",
                ContextSource::Runtime {
                    source_id: "host".into(),
                },
                "runtime facts",
            ),
            record(
                "tool",
                ContextSource::ToolResult {
                    call_id: "call".into(),
                    tool_id: "tool".into(),
                },
                "private tool result",
            ),
            record("second", ContextSource::User, "current question"),
        ];
        records
            .last_mut()
            .unwrap()
            .content
            .push(ContextContent::Opaque {
                payload: OpaquePayload::text("provider-private bytes"),
            });
        let context = ContextSnapshot {
            revision: 1,
            records: records.into(),
        };
        assert_eq!(
            recent_context(&context),
            Some(vec![
                serde_json::json!({"role":"user", "content":"recent question"}),
                serde_json::json!({"role":"assistant", "content":"recent answer"}),
                serde_json::json!({"role":"user", "content":"current question"}),
            ])
        );
    }
}
