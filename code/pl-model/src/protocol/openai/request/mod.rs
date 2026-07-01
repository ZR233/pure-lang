use pl_protocol::{PureError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::OpenAiEndpoint;
use crate::model_info::ModelInfo;
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
    validate_tool_history(&request.messages, endpoint)?;
    match endpoint {
        OpenAiEndpoint::Responses => {
            let mut body = to_object_map(&ResponsesRequestBody::from_request(request)?)?;
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Responses(body))
        }
        OpenAiEndpoint::ChatCompletions => {
            let mut body = to_object_map(&ChatRequestBody::from_request(request)?)?;
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Chat(body))
        }
    }
}

fn protocol_error(message: impl Into<String>) -> PureError {
    let msg = message.into();
    PureError::LlmError(format!("OpenAI request protocol error: {msg}"))
}
