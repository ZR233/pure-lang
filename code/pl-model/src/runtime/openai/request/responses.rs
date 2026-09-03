use pl_protocol::{
    AttachmentModality, ContentPart, MessageContent, MessageRole, Result, ToolCallKind,
    ToolCallRecord, ToolSpec,
};
use serde::Serialize;

use crate::completion::{CompletionRequest, ReasoningConfig, ReasoningSummary};
use crate::model::info::MediaWireFormat;

use super::body::ToolFormatBody;
use super::content::{
    MediaRepresentationPlan, media_url, message_content_text, tool_media_content,
};
use super::protocol_error;
use super::tool_history::{record_arguments_text, record_custom_input, tool_callers_by_call_id};
#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponsesRequestBody {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<ResponsesInputItem>,
    stream: bool,
    tool_choice: String,
    parallel_tool_calls: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ResponsesReasoning>,
}

impl ResponsesRequestBody {
    pub(super) fn from_request(
        request: &CompletionRequest,
        model: &crate::model::ModelInfo,
        prompt_cache_key: Option<&str>,
    ) -> Result<Self> {
        let mut input = Vec::new();
        let media_plan = MediaRepresentationPlan::for_request(request, model)?;
        let history = request
            .input
            .iter()
            .filter_map(|item| item.as_message())
            .cloned()
            .collect::<Vec<_>>();
        let tool_callers = tool_callers_by_call_id(&history);

        for item in &request.input {
            if let pl_protocol::ModelContextItem::ToolMedia { items } = item {
                let content = tool_media_content(items);
                input.push(ResponsesInputItem::message(
                    ResponsesRole::User,
                    responses_content_for_message(
                        &content,
                        MessageRole::User,
                        &request.prepared_content,
                        &media_plan,
                    )?,
                ));
                continue;
            }
            let msg = match item {
                pl_protocol::ModelContextItem::Message { message }
                | pl_protocol::ModelContextItem::ToolResult { message, .. } => message,
                pl_protocol::ModelContextItem::Compaction { encrypted_content } => {
                    input.push(ResponsesInputItem::typed(
                        ResponsesTypedInputItem::Compaction {
                            encrypted_content: encrypted_content.clone(),
                        },
                    ));
                    continue;
                }
                pl_protocol::ModelContextItem::Responses { item } => {
                    input.push(ResponsesInputItem::Native(item.value.clone()));
                    continue;
                }
                pl_protocol::ModelContextItem::ToolMedia { .. } => unreachable!(),
            };
            match msg.role {
                MessageRole::Assistant if msg.tool_calls.is_some() => {
                    let text = message_content_text(&msg.content);
                    if !text.is_empty() {
                        input.push(ResponsesInputItem::message(
                            ResponsesRole::Assistant,
                            vec![ResponsesContent::OutputText { text }],
                        ));
                    }
                    if let Some(tool_calls) = msg.tool_calls.as_ref() {
                        input.extend(tool_calls.iter().map(ResponsesInputItem::from));
                    }
                }
                MessageRole::Tool => {
                    let record = msg.tool_result.as_ref().ok_or_else(|| {
                        protocol_error("tool result message missing typed tool_result record")
                    })?;
                    let caller = tool_callers.get(&record.call_id).cloned();
                    let output = message_content_text(&msg.content);
                    match record.kind {
                        ToolCallKind::Function => {
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::FunctionCallOutput {
                                    call_id: record.call_id.clone(),
                                    output,
                                    caller,
                                },
                            ));
                        }
                        ToolCallKind::Custom => {
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::CustomToolCallOutput {
                                    call_id: record.call_id.clone(),
                                    output,
                                    caller,
                                },
                            ));
                        }
                    }
                }
                MessageRole::System | MessageRole::User | MessageRole::Assistant => {
                    input.push(ResponsesInputItem::message(
                        ResponsesRole::from(msg.role),
                        responses_content_for_message(
                            &msg.content,
                            msg.role,
                            &request.prepared_content,
                            &media_plan,
                        )?,
                    ));
                }
            }
        }

        let tools = (!request.tools.is_empty())
            .then(|| request.tools.iter().map(ResponsesTool::from).collect());

        Ok(Self {
            model: model
                .request_profile
                .api_model
                .clone()
                .unwrap_or_else(|| model.slug.clone()),
            instructions: request.instructions.clone(),
            input,
            stream: true,
            tool_choice: request.tool_choice.clone(),
            parallel_tool_calls: request.parallel_tool_calls,
            tools,
            temperature: request.temperature,
            store: false,
            prompt_cache_key: prompt_cache_key.map(ToString::to_string),
            reasoning: request.reasoning.as_ref().map(ResponsesReasoning::from),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ResponsesInputItem {
    Typed(ResponsesTypedInputItem),
    Native(serde_json::Value),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesTypedInputItem {
    Message {
        role: ResponsesRole,
        content: Vec<ResponsesContent>,
    },
    FunctionCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        arguments: String,
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    CustomToolCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        input: String,
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    CustomToolCallOutput {
        call_id: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    Compaction {
        encrypted_content: String,
    },
}

impl ResponsesInputItem {
    fn typed(item: ResponsesTypedInputItem) -> Self {
        Self::Typed(item)
    }

    fn message(role: ResponsesRole, content: Vec<ResponsesContent>) -> Self {
        Self::typed(ResponsesTypedInputItem::Message { role, content })
    }
}

impl From<&ToolCallRecord> for ResponsesInputItem {
    fn from(record: &ToolCallRecord) -> Self {
        let caller = record.caller.clone();
        match record.kind {
            ToolCallKind::Function => Self::typed(ResponsesTypedInputItem::FunctionCall {
                id: None,
                name: record.name.clone(),
                arguments: record_arguments_text(&record.arguments),
                call_id: record.call_id.clone(),
                caller,
            }),
            ToolCallKind::Custom => Self::typed(ResponsesTypedInputItem::CustomToolCall {
                id: None,
                name: record.name.clone(),
                input: record_custom_input(&record.arguments),
                call_id: record.call_id.clone(),
                caller,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ResponsesRole {
    Developer,
    User,
    Assistant,
    Tool,
}

impl From<MessageRole> for ResponsesRole {
    fn from(role: MessageRole) -> Self {
        match role {
            MessageRole::System => Self::Developer,
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
            MessageRole::Tool => Self::Tool,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContent {
    InputText { text: String },
    InputImage { image_url: String },
    OutputText { text: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesTool {
    Function {
        name: String,
        description: String,
        parameters: serde_json::Value,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<pl_protocol::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormatBody,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<pl_protocol::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    ProgrammaticToolCalling,
    WebSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        external_web_access: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        indexed_web_access: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<pl_protocol::WebSearchFilters>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_location: Option<pl_protocol::WebSearchUserLocation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_context_size: Option<pl_protocol::WebSearchContextSize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_content_types: Option<Vec<String>>,
    },
}

impl From<&ToolSpec> for ResponsesTool {
    fn from(tool: &ToolSpec) -> Self {
        match tool {
            ToolSpec::Function {
                name,
                description,
                input_schema,
                allowed_callers,
                output_schema,
            } => Self::Function {
                name: name.clone(),
                description: description.clone(),
                parameters: input_schema.clone(),
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSpec::Custom {
                name,
                description,
                format,
                allowed_callers,
                output_schema,
            } => Self::Custom {
                name: name.clone(),
                description: description.clone(),
                format: ToolFormatBody::from(format),
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSpec::ProgrammaticToolCalling => Self::ProgrammaticToolCalling,
            ToolSpec::WebSearch {
                dialect,
                external_web_access,
                indexed_web_access,
                filters,
                user_location,
                search_context_size,
                search_content_types,
            } => {
                let deepseek = *dialect == pl_protocol::HostedWebSearchDialect::DeepSeekResponses;
                Self::WebSearch {
                    external_web_access: (!deepseek).then_some(*external_web_access),
                    indexed_web_access: (!deepseek).then_some(*indexed_web_access).flatten(),
                    filters: (!deepseek).then_some(filters.clone()).flatten(),
                    user_location: (!deepseek).then_some(user_location.clone()).flatten(),
                    search_context_size: (!deepseek).then_some(*search_context_size).flatten(),
                    search_content_types: (!deepseek)
                        .then_some(search_content_types.clone())
                        .flatten(),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ResponsesReasoningSummary>,
}

impl From<&ReasoningConfig> for ResponsesReasoning {
    fn from(reasoning: &ReasoningConfig) -> Self {
        Self {
            summary: reasoning
                .summary
                .and_then(ResponsesReasoningSummary::from_summary),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ResponsesReasoningSummary {
    Auto,
}

impl ResponsesReasoningSummary {
    fn from_summary(summary: ReasoningSummary) -> Option<Self> {
        match summary {
            ReasoningSummary::Auto | ReasoningSummary::Enabled => Some(Self::Auto),
            ReasoningSummary::Disabled => None,
        }
    }
}

fn responses_content_for_message(
    content: &MessageContent,
    role: MessageRole,
    prepared_content: &[crate::completion::PreparedContentPart],
    media_plan: &MediaRepresentationPlan,
) -> Result<Vec<ResponsesContent>> {
    let mut response_content = Vec::new();
    for part in &content.parts {
        match part {
            ContentPart::Text { text } => {
                if role == MessageRole::Assistant {
                    response_content.push(ResponsesContent::OutputText { text: text.clone() });
                } else {
                    response_content.push(ResponsesContent::InputText { text: text.clone() });
                }
            }
            ContentPart::Attachment {
                attachment_id,
                modality,
                media_type,
                ..
            } => match modality {
                AttachmentModality::Image => {
                    if media_plan.wire(*modality)? != MediaWireFormat::ResponsesInputImage {
                        return Err(protocol_error(
                            "Chat media wire cannot be serialized by Responses",
                        ));
                    }
                    response_content.push(ResponsesContent::InputImage {
                        image_url: media_url(
                            attachment_id,
                            media_type,
                            *modality,
                            prepared_content,
                            media_plan,
                        )?,
                    });
                }
                AttachmentModality::File | AttachmentModality::Video => {
                    return Err(protocol_error(format!(
                        "Responses does not support {:?} attachments",
                        modality
                    )));
                }
            },
        }
    }
    Ok(response_content)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::info::{ModelInfo, ResponsesMaxTokensField};
    use crate::runtime::openai::OpenAiProtocol;
    use crate::runtime::openai::test_support::{bundled_model, request_with_effort};
    use pl_protocol::ToolSpec;

    #[test]
    fn responses_parallel_tool_calls_wire_is_unchanged() {
        let request = request_with_effort("high");
        let model = ModelInfo::fallback("responses-compatible");

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert_eq!(body["parallel_tool_calls"], serde_json::json!(true));
    }

    #[test]
    fn responses_body_writes_effort_via_parameter_wire() {
        let model = bundled_model("gpt-5.5");
        let body = OpenAiProtocol::responses()
            .build_request_body_with_model(&request_with_effort("high"), &model);

        assert_eq!(body["reasoning"]["effort"], serde_json::json!("high"));
    }

    #[test]
    fn responses_body_writes_gpt56_max_effort_via_parameter_wire() {
        let model = bundled_model("gpt-5.6-sol");
        let body = OpenAiProtocol::responses()
            .build_request_body_with_model(&request_with_effort("max"), &model);

        assert_eq!(body["reasoning"]["effort"], serde_json::json!("max"));
    }

    #[test]
    fn responses_body_maps_enabled_reasoning_summary_to_auto() {
        let model = bundled_model("gpt-5.5");
        let mut request = request_with_effort("medium");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Enabled);

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert_eq!(body["reasoning"]["summary"], serde_json::json!("auto"));
    }

    #[test]
    fn responses_body_omits_disabled_reasoning_summary() {
        let model = bundled_model("gpt-5.5");
        let mut request = request_with_effort("medium");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert!(body["reasoning"].get("summary").is_none());
    }

    #[test]
    fn request_codec_only_writes_invocation_prompt_cache_key_for_responses() {
        let request = request_with_effort("medium");
        let model = ModelInfo::fallback("gpt-5.5");

        let responses_body = serde_json::to_value(
            OpenAiProtocol::responses()
                .build_request(&request, &model, Some("project-cache-key"))
                .unwrap(),
        )
        .unwrap();
        let chat_body = serde_json::to_value(
            OpenAiProtocol::chat()
                .build_request(&request, &model, Some("project-cache-key"))
                .unwrap(),
        )
        .unwrap();

        assert_eq!(responses_body["store"], serde_json::json!(false));
        assert!(responses_body.get("previous_response_id").is_none());
        assert_eq!(
            responses_body["prompt_cache_key"],
            serde_json::json!("project-cache-key")
        );
        assert!(chat_body.get("store").is_none());
        assert!(chat_body.get("previous_response_id").is_none());
        assert!(chat_body.get("prompt_cache_key").is_none());
    }

    #[test]
    fn responses_body_omits_profiled_max_tokens_by_default() {
        let mut request = request_with_effort("medium");
        request.max_tokens = Some(8192);

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert!(body.get("max_output_tokens").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn responses_body_can_use_profiled_max_output_tokens_field() {
        let mut model = ModelInfo::fallback("responses-like");
        model.request_profile.responses_max_tokens_field = ResponsesMaxTokensField::MaxOutputTokens;
        let mut request = request_with_effort("medium");
        request.max_tokens = Some(8192);

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert_eq!(body["max_output_tokens"], serde_json::json!(8192));
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn responses_body_can_use_profiled_compatible_max_tokens_fields() {
        let mut request = request_with_effort("medium");
        request.max_tokens = Some(8192);

        let mut max_tokens_model = ModelInfo::fallback("responses-like");
        max_tokens_model.request_profile.responses_max_tokens_field =
            ResponsesMaxTokensField::MaxTokens;
        let max_tokens_body =
            OpenAiProtocol::responses().build_request_body_with_model(&request, &max_tokens_model);

        let mut max_completion_model = ModelInfo::fallback("responses-like");
        max_completion_model
            .request_profile
            .responses_max_tokens_field = ResponsesMaxTokensField::MaxCompletionTokens;
        let max_completion_body = OpenAiProtocol::responses()
            .build_request_body_with_model(&request, &max_completion_model);

        assert_eq!(max_tokens_body["max_tokens"], serde_json::json!(8192));
        assert!(max_tokens_body.get("max_output_tokens").is_none());
        assert_eq!(
            max_completion_body["max_completion_tokens"],
            serde_json::json!(8192)
        );
        assert!(max_completion_body.get("max_output_tokens").is_none());
    }

    #[test]
    fn responses_body_writes_custom_grammar_tool() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSpec::custom_grammar(
            "apply_patch",
            "edit files",
            "lark",
            "start: patch",
        )];

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], serde_json::json!("custom"));
        assert_eq!(body["tools"][0]["name"], serde_json::json!("apply_patch"));
        assert_eq!(
            body["tools"][0]["format"],
            serde_json::json!({
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: patch"
            })
        );
    }

    #[test]
    fn responses_body_writes_programmatic_tool_callers() {
        let model = bundled_model("gpt-5.6-sol");
        let mut request = request_with_effort("xhigh");
        let read_file = ToolSpec::function(
            "read_file",
            "read a file",
            serde_json::json!({"type": "object"}),
        )
        .allow_programmatic(serde_json::json!({
            "type": "object",
            "additionalProperties": true
        }));
        request.tools = vec![read_file, ToolSpec::ProgrammaticToolCalling];

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["allowed_callers"],
            serde_json::json!(["direct", "programmatic"])
        );
        assert_eq!(body["tools"][0]["output_schema"]["type"], "object");
        assert_eq!(body["tools"][1]["type"], "programmatic_tool_calling");
    }

    #[test]
    fn responses_body_writes_current_hosted_web_search_shape() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSpec::WebSearch {
            dialect: pl_protocol::HostedWebSearchDialect::OpenAiResponses,
            external_web_access: true,
            indexed_web_access: Some(true),
            filters: Some(pl_protocol::WebSearchFilters {
                allowed_domains: vec!["example.com".to_string()],
            }),
            user_location: Some(pl_protocol::WebSearchUserLocation {
                kind: pl_protocol::WebSearchUserLocationType::Approximate,
                country: Some("US".to_string()),
                region: Some("CA".to_string()),
                city: None,
                timezone: Some("America/Los_Angeles".to_string()),
            }),
            search_context_size: Some(pl_protocol::WebSearchContextSize::High),
            search_content_types: None,
        }];

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], "web_search");
        assert_eq!(body["tools"][0]["external_web_access"], true);
        assert_eq!(body["tools"][0]["indexed_web_access"], true);
        assert_eq!(
            body["tools"][0]["filters"]["allowed_domains"][0],
            "example.com"
        );
        assert_eq!(body["tools"][0]["user_location"]["type"], "approximate");
        assert_eq!(body["tools"][0]["search_context_size"], "high");
        assert!(body["tools"][0].get("search_content_types").is_none());
    }

    #[test]
    fn deepseek_responses_body_writes_only_native_web_search_type() {
        let mut request = request_with_effort("high");
        request.tools = vec![ToolSpec::WebSearch {
            dialect: pl_protocol::HostedWebSearchDialect::DeepSeekResponses,
            external_web_access: true,
            indexed_web_access: None,
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        }];
        let model = bundled_model("deepseek-v4-flash");

        let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

        assert_eq!(body["tools"], serde_json::json!([{"type": "web_search"}]));
    }
}
