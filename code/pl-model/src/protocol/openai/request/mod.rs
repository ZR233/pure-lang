use pl_protocol::{Message, ModelContextItem, PureError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::OpenAiEndpoint;
use crate::model_info::{ModelInfo, ResponsesMaxTokensField};
use crate::request::CompletionRequest;

mod body;
mod chat;
mod content;
mod responses;
mod tool_history;

use body::{finalize_body, to_object_map};
use chat::ChatRequestBody;
use responses::ResponsesRequestBody;
use tool_history::validate_tool_history;

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum OpenAiRequestBody {
    Responses(Map<String, Value>),
    Chat(Map<String, Value>),
}

pub(crate) fn build_openai_request_body(
    endpoint: OpenAiEndpoint,
    request: &CompletionRequest,
    model: &ModelInfo,
) -> Result<OpenAiRequestBody> {
    let messages = messages_after_last_compaction(&request.input);
    validate_tool_history(
        &messages,
        endpoint,
        endpoint == OpenAiEndpoint::Responses && request.previous_response_id.is_some(),
    )?;
    match endpoint {
        OpenAiEndpoint::Responses => {
            let mut body = to_object_map(&ResponsesRequestBody::from_request(request)?)?;
            apply_responses_max_tokens_field(
                &mut body,
                request.max_tokens,
                model.request_profile.responses_max_tokens_field,
            );
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Responses(body))
        }
        OpenAiEndpoint::ChatCompletions => {
            let mut body = to_object_map(&ChatRequestBody::from_request(request, model)?)?;
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Chat(body))
        }
    }
}

fn messages_after_last_compaction(input: &[ModelContextItem]) -> Vec<Message> {
    let start = input
        .iter()
        .rposition(ModelContextItem::is_compaction)
        .map_or(0, |index| index + 1);
    input[start..]
        .iter()
        .filter_map(ModelContextItem::as_message)
        .cloned()
        .collect()
}

fn apply_responses_max_tokens_field(
    body: &mut Map<String, Value>,
    max_tokens: Option<u64>,
    field: ResponsesMaxTokensField,
) {
    let Some(max_tokens) = max_tokens else {
        return;
    };
    let key = match field {
        ResponsesMaxTokensField::Omit => return,
        ResponsesMaxTokensField::MaxOutputTokens => "max_output_tokens",
        ResponsesMaxTokensField::MaxTokens => "max_tokens",
        ResponsesMaxTokensField::MaxCompletionTokens => "max_completion_tokens",
    };
    body.insert(key.to_string(), Value::from(max_tokens));
}

fn protocol_error(message: impl Into<String>) -> PureError {
    let msg = message.into();
    PureError::LlmError(format!("OpenAI request protocol error: {msg}"))
}
