use serde_json::Value;

use crate::stream::event::{ModelStreamEvent, ToolInputDeltaPayload, ToolInputPayloadKind};
use pl_trace::TraceTextChannel;

pub(super) fn cached_tokens_from_details(details: &Value) -> Option<u64> {
    details
        .get("cached_tokens")
        .or_else(|| details.get("cache_read_tokens"))
        .or_else(|| details.get("cached_input_tokens"))
        .and_then(Value::as_u64)
}

pub(super) fn output_item_tool_started(item: &Value) -> Option<ModelStreamEvent> {
    let kind = item.get("type")?.as_str()?;
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("call_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .map(String::from);
    let name = item.get("name").and_then(Value::as_str).map(String::from);

    match kind {
        "function_call" => Some(ModelStreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id,
            name,
            payload_kind: ToolInputPayloadKind::FunctionArguments,
        }),
        "custom_tool_call" => Some(ModelStreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id,
            name,
            payload_kind: ToolInputPayloadKind::CustomInput,
        }),
        _ => None,
    }
}

pub(super) fn output_item_tool_completed(item: &Value) -> Option<Vec<ModelStreamEvent>> {
    let kind = item.get("type")?.as_str()?;
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("call_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .map(String::from);
    let name = item.get("name").and_then(Value::as_str).map(String::from);
    match kind {
        "function_call" => {
            let payload = Some(ToolInputDeltaPayload::FunctionArguments(
                value_string(item, "arguments").unwrap_or_default(),
            ));
            Some(vec![
                ModelStreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id,
                    name,
                    payload,
                },
            ])
        }
        "custom_tool_call" => {
            let payload = Some(ToolInputDeltaPayload::CustomInput(
                value_string(item, "input").unwrap_or_default(),
            ));
            Some(vec![
                ModelStreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id,
                    name,
                    payload,
                },
            ])
        }
        _ => None,
    }
}

pub(super) fn assistant_message_identity(
    item: Option<&Value>,
) -> Option<(String, TraceTextChannel)> {
    let item = item?;
    if item.get("type")?.as_str()? != "message" {
        return None;
    }
    if item
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "assistant")
    {
        return None;
    }
    let item_id = item.get("id")?.as_str()?.to_string();
    let channel = match item.get("phase").and_then(Value::as_str) {
        Some("commentary") => TraceTextChannel::Commentary,
        Some("final_answer" | "final") => TraceTextChannel::Final,
        _ => TraceTextChannel::Final,
    };
    Some((item_id, channel))
}

pub(super) fn assistant_message_text(item: &Value) -> Option<String> {
    let text = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .concat();
    (!text.is_empty()).then_some(text)
}

pub(super) fn reasoning_item_id(item: &Value) -> Option<String> {
    if item.get("type")?.as_str()? != "reasoning" {
        return None;
    }
    item.get("id")?.as_str().map(ToOwned::to_owned)
}

pub(super) fn reasoning_summary_texts(item: &Value) -> Option<Vec<String>> {
    let mut summaries = Vec::new();
    for field in ["summary", "content"] {
        if let Some(parts) = item.get(field).and_then(Value::as_array) {
            summaries.extend(
                parts
                    .iter()
                    .filter_map(reasoning_summary_part_text)
                    .map(ToOwned::to_owned),
            );
        }
    }
    (!summaries.is_empty()).then_some(summaries)
}

fn value_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn reasoning_summary_part_text(part: &Value) -> Option<&str> {
    match part {
        Value::String(text) => Some(text.as_str()).filter(|text| !text.is_empty()),
        Value::Object(_) => {
            let kind = part.get("type").and_then(Value::as_str);
            matches!(
                kind,
                Some("summary_text" | "reasoning_summary_text" | "output_text")
            )
            .then(|| part.get("text").and_then(Value::as_str))?
            .filter(|text| !text.is_empty())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
    }
}
