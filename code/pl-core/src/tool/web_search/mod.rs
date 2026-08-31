mod client;
mod input;

use futures::FutureExt;
use pl_model::{
    HostedWebSearchDialect, SearchCommands, SearchRequest, SearchSettings, ToolSpec,
    WebSearchAction, WebSearchConfig, WebSearchFilters, WebSearchMode, WebSearchUserLocation,
};
use pl_protocol::{MessageRole, PureError};
use serde_json::{Value, json};

use crate::turn::ToolEffect;

pub(crate) use self::client::WebSearchClient;
use self::input::parse_commands;
use super::{
    BoxFuture, OutputTruncation, Tool, ToolCallContext, ToolDirective, ToolExecution, ToolInput,
    ToolResult, ToolSessionRuntime, TypedTool, run_tool_backend_with_cancellation,
};

pub const TOOL_WEB_SEARCH: &str = "web_search";
const ASSISTANT_CONTEXT_CHAR_LIMIT: usize = 4_000;
const WEB_SEARCH_DESCRIPTION: &str = "Search or open web pages, find text in pages, capture PDF pages, and query finance, weather, sports, or time data. Pass commands as arrays of objects, for example {\"search_query\":[{\"q\":\"latest Flutter release\"}]} or {\"open\":[{\"ref_id\":\"turn0search0\"}]}. Multiple commands may be combined in one call.";

/// 使用已门控 OpenAI backend 执行 `/alpha/search` 的普通函数工具。
#[derive(Debug, Clone)]
pub struct WebSearchTool {
    client: WebSearchClient,
    model: String,
    settings: SearchSettings,
    max_output_tokens: Option<u64>,
    session_runtime: ToolSessionRuntime,
}

impl WebSearchTool {
    pub(crate) fn new(
        client: WebSearchClient,
        model: impl Into<String>,
        config: &WebSearchConfig,
        max_output_tokens: Option<u64>,
        session_runtime: ToolSessionRuntime,
    ) -> Self {
        Self {
            client,
            model: model.into(),
            settings: SearchSettings::from_config(config),
            max_output_tokens,
            session_runtime,
        }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        TOOL_WEB_SEARCH
    }

    fn description(&self) -> &str {
        WEB_SEARCH_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        TypedTool::<SearchCommands>::new(self.name(), self.description()).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            let commands = parse_commands(input.arguments)?;
            let action = command_action(&commands);
            let request = SearchRequest {
                id: format!(
                    "{}:{}",
                    context.identity().turn_id,
                    context.identity().item_id
                ),
                model: self.model.clone(),
                input: recent_input(self.session_runtime.parent_session().messages()),
                commands,
                settings: self.settings.clone(),
                max_output_tokens: self.max_output_tokens,
            };
            let cancellation_token = context.cancellation_token();
            let response = run_tool_backend_with_cancellation(
                self.client.search(&request),
                cancellation_token,
                || PureError::ToolExecutionFailed {
                    tool: TOOL_WEB_SEARCH.to_string(),
                    error: "tool execution cancelled".to_string(),
                },
            )
            .await?;
            let artifact = json!({
                "kind": "webSearch",
                "action": action,
                "results": response.results,
            });
            Ok(ToolResult::from_runtime_text(
                response.output,
                OutputTruncation::empty(),
                Default::default(),
                Some(0),
                false,
                vec![ToolDirective::OutputArtifacts {
                    artifacts: vec![artifact],
                }],
            ))
        }
        .boxed()
    }
}

/// Responses 原生 hosted `web_search` 的 schema 载体。
#[derive(Debug, Clone)]
pub struct HostedWebSearchTool {
    schema: ToolSpec,
}

impl HostedWebSearchTool {
    pub fn from_config(config: &WebSearchConfig) -> Option<Self> {
        let (external_web_access, indexed_web_access) = match config.mode {
            WebSearchMode::Cached => (false, None),
            WebSearchMode::Indexed => (true, Some(true)),
            WebSearchMode::Live => (true, None),
            WebSearchMode::Disabled => return None,
        };
        Some(Self {
            schema: ToolSpec::WebSearch {
                dialect: HostedWebSearchDialect::OpenAiResponses,
                external_web_access,
                indexed_web_access,
                filters: (!config.allowed_domains.is_empty()).then(|| WebSearchFilters {
                    allowed_domains: config.allowed_domains.clone(),
                }),
                user_location: config
                    .location
                    .as_ref()
                    .filter(|location| !location.is_empty())
                    .map(WebSearchUserLocation::from),
                search_context_size: config.context_size,
                search_content_types: None,
            },
        })
    }

    pub fn deepseek() -> Self {
        Self {
            schema: ToolSpec::WebSearch {
                dialect: HostedWebSearchDialect::DeepSeekResponses,
                external_web_access: true,
                indexed_web_access: None,
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        }
    }
}

impl Tool for HostedWebSearchTool {
    fn name(&self) -> &str {
        TOOL_WEB_SEARCH
    }

    fn description(&self) -> &str {
        "Search the web."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execution(&self) -> ToolExecution {
        ToolExecution::ProviderHosted
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async {
            Err(PureError::ToolExecutionFailed {
                tool: TOOL_WEB_SEARCH.to_string(),
                error: "hosted web search is executed by the model provider".to_string(),
            })
        }
        .boxed()
    }

    fn spec(&self) -> ToolSpec {
        self.schema.clone()
    }
}

fn recent_input(messages: &[pl_protocol::Message]) -> Option<Vec<Value>> {
    let selected_user_indexes = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message.role == MessageRole::User)
        .take(2)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let start = selected_user_indexes.last().copied()?;
    let mut remaining_assistant_chars = ASSISTANT_CONTEXT_CHAR_LIMIT;
    let input = messages[start..]
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant if !message.metadata.contains_key("tool_calls") => {
                    "assistant"
                }
                MessageRole::System | MessageRole::Tool | MessageRole::Assistant => return None,
            };
            let mut text = crate::message::message_content_text(&message.content);
            if text.trim_start().starts_with("<environment_context>") {
                return None;
            }
            if role == "assistant" {
                text = text.chars().take(remaining_assistant_chars).collect();
                remaining_assistant_chars =
                    remaining_assistant_chars.saturating_sub(text.chars().count());
            }
            (!text.is_empty()).then(|| {
                json!({
                    "type": "message",
                    "role": role,
                    "content": [{
                        "type": if role == "assistant" { "output_text" } else { "input_text" },
                        "text": text,
                    }]
                })
            })
        })
        .collect::<Vec<_>>();
    (!input.is_empty()).then_some(input)
}

fn command_action(commands: &SearchCommands) -> WebSearchAction {
    commands
        .search_query
        .as_deref()
        .and_then(query_action)
        .or_else(|| commands.image_query.as_deref().and_then(query_action))
        .or_else(|| {
            commands.open.as_deref().and_then(|operations| {
                operations
                    .first()
                    .map(|operation| WebSearchAction::OpenPage {
                        url: literal_url(&operation.ref_id),
                    })
            })
        })
        .or_else(|| {
            commands.find.as_deref().and_then(|operations| {
                operations
                    .first()
                    .map(|operation| WebSearchAction::FindInPage {
                        url: literal_url(&operation.ref_id),
                        pattern: Some(operation.pattern.clone()),
                    })
            })
        })
        .unwrap_or(WebSearchAction::Other)
}

fn query_action(queries: &[pl_model::SearchQuery]) -> Option<WebSearchAction> {
    match queries {
        [] => None,
        [query] => Some(WebSearchAction::Search {
            query: Some(query.q.clone()),
            queries: Vec::new(),
        }),
        queries => Some(WebSearchAction::Search {
            query: None,
            queries: queries.iter().map(|query| query.q.clone()).collect(),
        }),
    }
}

fn literal_url(value: &str) -> Option<String> {
    (value.starts_with("http://") || value.starts_with("https://")).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use pl_protocol::{Message, MessageContent};

    use super::*;

    #[test]
    fn hosted_mode_mapping_is_exact() {
        for (mode, external, indexed) in [
            (WebSearchMode::Cached, false, None),
            (WebSearchMode::Indexed, true, Some(true)),
            (WebSearchMode::Live, true, None),
        ] {
            let tool = HostedWebSearchTool::from_config(&WebSearchConfig {
                mode,
                ..WebSearchConfig::default()
            })
            .expect("enabled mode");
            let ToolSpec::WebSearch {
                external_web_access,
                indexed_web_access,
                ..
            } = tool.spec()
            else {
                panic!("hosted schema");
            };
            assert_eq!(external_web_access, external);
            assert_eq!(indexed_web_access, indexed);
        }
        assert!(
            HostedWebSearchTool::from_config(&WebSearchConfig {
                mode: WebSearchMode::Disabled,
                ..WebSearchConfig::default()
            })
            .is_none()
        );
    }

    #[test]
    fn recent_context_keeps_two_user_turns_and_visible_assistant_text() {
        let message = |role, text: &str| Message {
            role,
            content: MessageContent::text(text.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        };
        let input = recent_input(&[
            message(MessageRole::System, "system"),
            message(MessageRole::User, "old"),
            message(MessageRole::Assistant, "old answer"),
            message(MessageRole::User, "previous"),
            message(MessageRole::Assistant, "visible answer"),
            message(MessageRole::User, "current"),
        ])
        .expect("context");
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[2]["role"], "user");
    }

    #[test]
    fn recent_context_starts_at_the_oldest_selected_user_message() {
        let message = |role, text: &str| Message {
            role,
            content: MessageContent::text(text.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        };
        let input = recent_input(&[
            message(MessageRole::Assistant, "unrelated prelude"),
            message(MessageRole::User, "only user"),
        ])
        .expect("context");

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }
}
