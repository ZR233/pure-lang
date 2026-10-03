//! Frozen assistant history, independent of the final-answer presentation.

use serde::{Deserialize, Serialize};
pub use serde_json::Value;
use std::collections::BTreeSet;

use super::{
    CompletionResponse, Message, MessageContent, MessageRole, ModelContextItem, ToolCallKind,
};
use pl_protocol::{PureError, ResponsesContextItem, ResponsesContextItemKind, Result};

/// Model-owned replay material. Open provider output fields stay opaque at this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantReplay {
    Responses { output: Vec<Value> },
    Chat { content: String },
}

impl AssistantReplay {
    pub(crate) fn semantic_input(
        &self,
        response: &CompletionResponse,
    ) -> Result<Vec<ModelContextItem>> {
        let mut input = Vec::new();
        let mut text = String::new();
        match self {
            Self::Chat { content } => text.clone_from(content),
            Self::Responses { output } => {
                let mut calls = BTreeSet::new();
                let mut ids = BTreeSet::new();
                for item in output {
                    if item
                        .get("role")
                        .and_then(Value::as_str)
                        .is_some_and(|role| role != "assistant")
                    {
                        return Err(invalid("native assistant output changes message authority"));
                    }
                    if let Some(id) = item.get("id").and_then(Value::as_str)
                        && !ids.insert(id)
                    {
                        return Err(invalid("duplicate assistant output item identity"));
                    }
                    match item.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            let parts = item
                                .get("content")
                                .and_then(Value::as_array)
                                .ok_or_else(|| invalid("assistant message has no content parts"))?;
                            for part in parts {
                                if let Some(value) = part
                                    .get("text")
                                    .or_else(|| part.get("refusal"))
                                    .and_then(Value::as_str)
                                {
                                    text.push_str(value);
                                }
                            }
                        }
                        Some(kind @ ("function_call" | "custom_tool_call")) => {
                            let call_id = item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .ok_or_else(|| invalid("assistant call has no identity"))?;
                            let call = response
                                .tool_calls
                                .iter()
                                .find(|call| call.call_id == call_id)
                                .ok_or_else(|| {
                                    invalid("native assistant call has no submitted binding")
                                })?;
                            let (wire_kind, field) = match call.kind() {
                                ToolCallKind::Function => ("function_call", "arguments"),
                                ToolCallKind::Custom => ("custom_tool_call", "input"),
                            };
                            if !calls.insert(call_id)
                                || kind != wire_kind
                                || item.get("name").and_then(Value::as_str)
                                    != Some(call.name.as_str())
                                || item.get(field).and_then(Value::as_str)
                                    != Some(call.payload_text().as_str())
                                || item
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .is_some_and(|id| id != call.id)
                            {
                                return Err(invalid(
                                    "native assistant call differs from submitted binding",
                                ));
                            }
                        }
                        Some("function_call_output" | "custom_tool_call_output") => {
                            return Err(invalid("assistant output cannot fabricate a tool result"));
                        }
                        Some(_) => {
                            let native = ResponsesContextItem::from_wire(item.clone())
                                .unwrap_or_else(|| ResponsesContextItem {
                                    kind: ResponsesContextItemKind::Unknown,
                                    value: item.clone(),
                                });
                            input.push(ModelContextItem::Responses { item: native });
                        }
                        None => return Err(invalid("assistant output has no type")),
                    }
                }
                if calls.len() != response.tool_calls.len() {
                    return Err(invalid("assistant output is missing a submitted call"));
                }
            }
        }
        input.push(ModelContextItem::from(Message {
            presentation: Default::default(),
            role: MessageRole::Assistant,
            content: MessageContent::text(text),
            reasoning_content: response.reasoning_content.clone(),
            tool_calls: (!response.tool_calls.is_empty()).then(|| {
                response
                    .tool_calls
                    .iter()
                    .map(super::ToolCall::history_record)
                    .collect()
            }),
            tool_result: None,
            metadata: Default::default(),
        }));
        Ok(input)
    }
}

/// Consumes only facts present in frames that predate complete replay recording.
/// It deliberately does not recover text or phase from presentation projections.
pub(crate) fn recorded_input(response: &CompletionResponse) -> Result<Vec<ModelContextItem>> {
    let mut input = Vec::new();
    for native in &response.responses_context_items {
        if native
            .value
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|role| role != "assistant")
        {
            return Err(invalid(
                "native assistant material changes message authority",
            ));
        }
        input.push(ModelContextItem::Responses {
            item: native.clone(),
        });
    }
    input.push(ModelContextItem::from(Message {
        role: MessageRole::Assistant,
        content: MessageContent::text(response.content.clone().unwrap_or_default()),
        presentation: Default::default(),
        reasoning_content: response.reasoning_content.clone(),
        tool_calls: (!response.tool_calls.is_empty()).then(|| {
            response
                .tool_calls
                .iter()
                .map(super::ToolCall::history_record)
                .collect()
        }),
        tool_result: None,
        metadata: Default::default(),
    }));
    Ok(input)
}

fn invalid(message: &str) -> PureError {
    PureError::Protocol(message.into())
}

#[derive(Debug, Clone)]
pub(crate) struct ReplaySpan {
    pub start: usize,
    pub semantic: Vec<ModelContextItem>,
    pub output: Vec<Value>,
}
