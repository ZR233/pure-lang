//! Independent search over frozen generic context, with no product session access.
use super::{
    ASSISTANT_CONTEXT_CHAR_LIMIT, DEEPSEEK_SEARCH_CONTEXT_CHAR_LIMIT,
    DEEPSEEK_WEB_SEARCH_DESCRIPTION, TOOL_DEEPSEEK_WEB_SEARCH, TOOL_WEB_SEARCH,
    WEB_SEARCH_DESCRIPTION, WebSearchClient,
};
use pl_core::{
    context::{ContextContent, ContextSnapshot, ContextSource, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use pl_model::provider::deepseek::search::{
    SearchClient, SearchRequest as DeepSeekSearchRequest, SearchResponse,
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
                | ContextSource::InstructionSnapshot { .. }
                | ContextSource::Instruction
                | ContextSource::Runtime { .. }
                | ContextSource::RuntimeFact { .. }
                | ContextSource::AgentMessage { .. }
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

/// Arguments accepted by the DeepSeek standalone search tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeepSeekWebSearchArgs {
    /// Natural-language search query.
    query: String,
}

/// A Thread-local DeepSeek search instance sharing only its immutable client configuration.
pub struct ThreadDeepSeekWebSearchTool {
    client: SearchClient,
}

impl std::fmt::Debug for ThreadDeepSeekWebSearchTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadDeepSeekWebSearchTool")
            .finish_non_exhaustive()
    }
}

impl ThreadDeepSeekWebSearchTool {
    /// Binds the resolved standalone DeepSeek search client.
    pub fn new(client: SearchClient) -> Self {
        Self { client }
    }

    /// Stable declaration, independent of conversation, current time and search results.
    pub fn declaration() -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            TOOL_DEEPSEEK_WEB_SEARCH,
            DEEPSEEK_WEB_SEARCH_DESCRIPTION,
            schemars::schema_for!(DeepSeekWebSearchArgs).to_value(),
        )
    }

    /// Transfers an executor without granting any framework control permissions.
    ///
    /// # Errors
    /// Returns an invalid tool identity error.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        Registration::new(TOOL_DEEPSEEK_WEB_SEARCH.into(), declaration, self)
    }
}

impl Tool for ThreadDeepSeekWebSearchTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                TOOL_DEEPSEEK_WEB_SEARCH,
                "unsupported argument encoding",
            )));
        }
        let args: DeepSeekWebSearchArgs =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let request = DeepSeekSearchRequest { query: args.query };
        match self
            .client
            .search(&request, context.cancellation.clone())
            .await
        {
            Ok(response) => deepseek_search_output(&response, None),
            Err(error) => match error.observed_response() {
                Some(observed) => {
                    // Keep the observed facts in history, but make the model-visible block
                    // unambiguously a failure even when no usable source was returned.
                    let notice = format!("[DeepSeek web search failed: {error}]");
                    let output = deepseek_search_output(observed, Some(notice))?;
                    Err(ToolError::new(error).with_output(output))
                }
                None => Err(ToolError::new(error)),
            },
        }
    }
}

/// Serializes the complete response for history while projecting a bounded model-visible summary.
fn deepseek_search_output(
    response: &SearchResponse,
    context_prefix: Option<String>,
) -> Result<ToolOutput, ToolError> {
    let encoded = serde_json::to_string(response).map_err(ToolError::new)?;
    let payload =
        OpaquePayload::new("pl.tool.deepseek-web-search", 1, encoded).map_err(ToolError::new)?;
    let summary = project_deepseek_summary(response);
    let text = match (context_prefix, summary.is_empty()) {
        (Some(prefix), false) => format!("{prefix}\n{summary}"),
        (Some(prefix), true) => prefix,
        (None, _) => summary,
    };
    Ok(ToolOutput::new(
        payload,
        vec![ContextContent::Text {
            text: Arc::from(text),
        }],
    ))
}

fn project_deepseek_summary(response: &SearchResponse) -> String {
    let mut sections = Vec::with_capacity(response.sources.len());
    for source in &response.sources {
        let mut section = String::new();
        match source.title.as_deref().filter(|title| !title.is_empty()) {
            Some(title) => section.push_str(title),
            None => section.push_str(&source.url),
        }
        section.push('\n');
        section.push_str(&source.url);
        if let Some(snippet) = source
            .snippet
            .as_deref()
            .filter(|snippet| !snippet.is_empty())
        {
            section.push('\n');
            section.push_str(snippet);
        }
        if let Some(published) = source
            .published_at
            .as_deref()
            .filter(|published| !published.is_empty())
        {
            section.push('\n');
            section.push_str(published);
        }
        sections.push(section);
    }
    let joined = sections.join("\n\n");
    let bounded = pl_output::bounded_text(&joined, DEEPSEEK_SEARCH_CONTEXT_CHAR_LIMIT, 0);
    if bounded.truncated {
        format!(
            "{}\n[DeepSeek search preview omitted {} bytes; full result retained in history.]",
            bounded.text, bounded.bytes_omitted
        )
    } else {
        bounded.text
    }
}
