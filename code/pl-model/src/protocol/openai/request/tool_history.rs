use std::collections::{HashMap, VecDeque};

use pl_protocol::{
    Message, MessageRole, Result, TOOL_CALLS_METADATA_KEY, ToolCallHistoryMetadata, ToolCallKind,
    ToolResultMetadata,
};

use crate::request::ToolCall;

use super::{OpenAiEndpoint, protocol_error};
/// 从 metadata 中解析 tool_calls（由 CoreSession::push_assistant_tool_calls 写入）。
pub(super) fn parse_tool_calls_from_metadata(
    metadata: &HashMap<String, String>,
) -> Result<Option<Vec<ToolCall>>> {
    ToolCallHistoryMetadata::from_metadata(metadata)
        .map(|metadata| {
            serde_json::from_str(&metadata.tool_calls_json).map_err(|error| {
                protocol_error(format!("invalid assistant tool_calls metadata: {error}"))
            })
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedToolOutput {
    tool_call_id: String,
    call_id: String,
    tool_call_kind: ToolCallKind,
}

pub(super) fn validate_tool_history(
    messages: &[Message],
    endpoint: OpenAiEndpoint,
    allow_leading_tool_results: bool,
) -> Result<()> {
    let mut expected_outputs = VecDeque::new();

    for message in messages {
        match message.role {
            MessageRole::Assistant if message.metadata.contains_key(TOOL_CALLS_METADATA_KEY) => {
                let tool_calls = parse_tool_calls_from_metadata(&message.metadata)?
                    .ok_or_else(|| protocol_error("assistant tool_calls metadata missing"))?;
                for tool_call in tool_calls {
                    if tool_call.id.is_empty() {
                        return Err(protocol_error("assistant tool call has empty id"));
                    }
                    if tool_call.call_id.is_empty() {
                        return Err(protocol_error("assistant tool call has empty call_id"));
                    }
                    let tool_call_kind = tool_call.kind();
                    expected_outputs.push_back(ExpectedToolOutput {
                        tool_call_id: tool_call.id,
                        call_id: tool_call.call_id,
                        tool_call_kind,
                    });
                }
            }
            MessageRole::Tool => {
                let metadata =
                    ToolResultMetadata::from_metadata(&message.metadata).map_err(protocol_error)?;
                let Some(expected) = expected_outputs.pop_front() else {
                    if allow_leading_tool_results {
                        continue;
                    }
                    return Err(protocol_error(
                        "tool result has no preceding assistant tool call",
                    ));
                };
                if metadata.tool_call_id != expected.tool_call_id {
                    return Err(protocol_error(format!(
                        "tool result id {} does not match assistant tool call id {}",
                        metadata.tool_call_id, expected.tool_call_id
                    )));
                }
                if endpoint == OpenAiEndpoint::Responses
                    && metadata.tool_call_call_id.as_deref() != Some(expected.call_id.as_str())
                {
                    return Err(protocol_error(format!(
                        "tool result call_id {:?} does not match assistant tool call call_id {:?}",
                        metadata.tool_call_call_id, expected.call_id
                    )));
                }
                if metadata.tool_call_kind != expected.tool_call_kind {
                    return Err(protocol_error(format!(
                        "tool result kind {} does not match assistant tool call kind {}",
                        metadata.tool_call_kind.as_str(),
                        expected.tool_call_kind.as_str()
                    )));
                }
            }
            MessageRole::System | MessageRole::User | MessageRole::Assistant => {}
        }
    }

    if let Some(expected) = expected_outputs.front() {
        return Err(protocol_error(format!(
            "assistant tool call {} is missing tool output",
            expected.tool_call_id
        )));
    }

    Ok(())
}
