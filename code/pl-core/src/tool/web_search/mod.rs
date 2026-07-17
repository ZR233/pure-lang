use pl_model::{
    SearchCommands, SearchRequest, SearchSettings, ToolSchema, WebSearchAction, WebSearchClient,
    WebSearchConfig, WebSearchFilters, WebSearchMode, WebSearchUserLocation,
};
use pl_protocol::{MessageRole, PureError};
use serde_json::{Value, json};

use crate::turn::ToolEffect;

use super::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent,
    run_tool_backend_with_cancellation,
};

pub const TOOL_WEB_SEARCH: &str = "web_search";
const ASSISTANT_CONTEXT_CHAR_LIMIT: usize = 4_000;

/// 使用已门控 OpenAI backend 执行 `/alpha/search` 的普通函数工具。
#[derive(Debug, Clone)]
pub struct WebSearchTool {
    client: WebSearchClient,
    model: String,
    settings: SearchSettings,
    max_output_tokens: Option<u64>,
}

impl WebSearchTool {
    pub fn new(
        client: WebSearchClient,
        model: impl Into<String>,
        config: &WebSearchConfig,
        max_output_tokens: Option<u64>,
    ) -> Self {
        Self {
            client,
            model: model.into(),
            settings: SearchSettings::from_config(config),
            max_output_tokens,
        }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        TOOL_WEB_SEARCH
    }

    fn description(&self) -> &str {
        "Search or open web pages, find text in pages, capture PDF pages, and query finance, weather, sports, or time data."
    }

    fn input_schema(&self) -> Value {
        commands_schema()
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
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let commands =
                serde_json::from_value::<SearchCommands>(input.arguments).map_err(|error| {
                    PureError::ToolExecutionFailed {
                        tool: TOOL_WEB_SEARCH.to_string(),
                        error: format!("invalid input: {error}"),
                    }
                })?;
            let action = command_action(&commands);
            let request = SearchRequest {
                id: format!("{}:{}", input.session_id, input.tool_id),
                model: self.model.clone(),
                input: recent_input(context.parent_session.messages()),
                commands,
                settings: self.settings.clone(),
                max_output_tokens: self.max_output_tokens,
            };
            let cancellation_token = context.options.cancellation_token.clone();
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
            Ok(ToolOutput {
                description: response.output,
                truncated: OutputTruncation::empty(),
                output_file: Default::default(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: vec![ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![artifact],
                }],
            })
        })
    }
}

/// Responses 原生 hosted `web_search` 的 schema 载体。
#[derive(Debug, Clone)]
pub struct HostedWebSearchTool {
    schema: ToolSchema,
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
            schema: ToolSchema::WebSearch {
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

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async {
            Err(PureError::ToolExecutionFailed {
                tool: TOOL_WEB_SEARCH.to_string(),
                error: "hosted web search is executed by the model provider".to_string(),
            })
        })
    }

    fn to_schema(&self) -> ToolSchema {
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

fn commands_schema() -> Value {
    let string = || json!({ "type": "string" });
    let optional_string = || json!({ "type": "string" });
    let object_array = |properties: Value, required: Vec<&str>| {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }
        })
    };
    let query = object_array(
        json!({
            "q": string(),
            "recency": { "type": "integer", "minimum": 0 },
            "domains": { "type": "array", "items": string() }
        }),
        vec!["q"],
    );
    json!({
        "type": "object",
        "properties": {
            "search_query": query.clone(),
            "image_query": query,
            "open": object_array(json!({
                "ref_id": string(), "lineno": { "type": "integer", "minimum": 0 }
            }), vec!["ref_id"]),
            "click": object_array(json!({
                "ref_id": string(), "id": { "type": "integer", "minimum": 0 }
            }), vec!["ref_id", "id"]),
            "find": object_array(json!({
                "ref_id": string(), "pattern": string()
            }), vec!["ref_id", "pattern"]),
            "screenshot": object_array(json!({
                "ref_id": string(), "pageno": { "type": "integer", "minimum": 0 }
            }), vec!["ref_id", "pageno"]),
            "finance": object_array(json!({
                "ticker": string(),
                "type": { "type": "string", "enum": ["equity", "fund", "crypto", "index"] },
                "market": optional_string()
            }), vec!["ticker", "type"]),
            "weather": object_array(json!({
                "location": string(), "start": optional_string(),
                "duration": { "type": "integer", "minimum": 0 }
            }), vec!["location"]),
            "sports": object_array(json!({
                "tool": { "type": "string", "enum": ["sports"] },
                "fn": { "type": "string", "enum": ["schedule", "standings"] },
                "league": { "type": "string", "enum": ["nba", "wnba", "nfl", "nhl", "mlb", "epl", "ncaamb", "ncaawb", "ipl"] },
                "team": optional_string(), "opponent": optional_string(),
                "date_from": optional_string(), "date_to": optional_string(),
                "num_games": { "type": "integer", "minimum": 0 }, "locale": optional_string()
            }), vec!["fn", "league"]),
            "time": object_array(json!({ "utc_offset": string() }), vec!["utc_offset"]),
            "response_length": { "type": "string", "enum": ["short", "medium", "long"] }
        },
        "additionalProperties": false
    })
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
            let ToolSchema::WebSearch {
                external_web_access,
                indexed_web_access,
                ..
            } = tool.to_schema()
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
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
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
    fn standalone_schema_exposes_every_supported_search_command() {
        let schema = commands_schema();
        let properties = schema["properties"].as_object().expect("properties");

        for name in [
            "search_query",
            "image_query",
            "open",
            "click",
            "find",
            "screenshot",
            "finance",
            "weather",
            "sports",
            "time",
            "response_length",
        ] {
            assert!(properties.contains_key(name), "missing command {name}");
        }
        assert_eq!(properties.len(), 11);
        assert_eq!(
            properties["response_length"]["enum"],
            json!(["short", "medium", "long"])
        );
    }

    #[test]
    fn recent_context_starts_at_the_oldest_selected_user_message() {
        let message = |role, text: &str| Message {
            role,
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
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
