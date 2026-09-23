use crate::completion::{
    CompletionPresentationItem, CompletionPresentationItemKind, CompletionPresentationPart,
    CompletionPresentationPartKind,
};
use serde_json::Value;

use crate::completion::stream::event::{
    ModelStreamEvent, ToolInputDeltaPayload, ToolInputPayloadKind,
};
use crate::runtime::openai::identity::responses_tool_identity;
use pl_protocol::search::WebSearchAction;
use pl_protocol::trace::TraceTextChannel;
use pl_protocol::{ResponsesContextItem, ToolCallCaller};

pub(super) fn output_item_tool_started(item: &Value) -> Option<ModelStreamEvent> {
    let kind = item.get("type")?.as_str()?;
    let (item_id, call_id) = responses_tool_identity(
        item.get("id").and_then(Value::as_str),
        item.get("call_id").and_then(Value::as_str),
        kind,
    );
    let name = item.get("name").and_then(Value::as_str).map(String::from);

    match kind {
        "function_call" => Some(ModelStreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id: Some(call_id),
            name,
            payload_kind: ToolInputPayloadKind::FunctionArguments,
        }),
        "custom_tool_call" => Some(ModelStreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id: Some(call_id),
            name,
            payload_kind: ToolInputPayloadKind::CustomInput,
        }),
        "web_search_call" => Some(ModelStreamEvent::WebSearchStarted {
            item_id,
            action: web_search_action(item.get("action")),
        }),
        _ => None,
    }
}

pub(super) fn output_item_tool_completed(item: &Value) -> Option<Vec<ModelStreamEvent>> {
    let kind = item.get("type")?.as_str()?;
    let (item_id, call_id) = responses_tool_identity(
        item.get("id").and_then(Value::as_str),
        item.get("call_id").and_then(Value::as_str),
        kind,
    );
    let name = item.get("name").and_then(Value::as_str).map(String::from);
    match kind {
        "function_call" => {
            let payload = Some(ToolInputDeltaPayload::FunctionArguments(
                value_string(item, "arguments").unwrap_or_default(),
            ));
            let mut events = tool_caller_event(item, &item_id)
                .into_iter()
                .collect::<Vec<_>>();
            events.extend([
                ModelStreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: Some(call_id.clone()),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id: Some(call_id),
                    name,
                    payload,
                },
            ]);
            Some(events)
        }
        "custom_tool_call" => {
            let payload = Some(ToolInputDeltaPayload::CustomInput(
                value_string(item, "input").unwrap_or_default(),
            ));
            let mut events = tool_caller_event(item, &item_id)
                .into_iter()
                .collect::<Vec<_>>();
            events.extend([
                ModelStreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: Some(call_id.clone()),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id: Some(call_id),
                    name,
                    payload,
                },
            ]);
            Some(events)
        }
        "web_search_call" => Some(vec![ModelStreamEvent::WebSearchCompleted {
            item_id,
            action: web_search_action(item.get("action")),
            results: item.get("results").and_then(Value::as_array).cloned(),
        }]),
        _ => None,
    }
}

pub(super) fn output_item_native_context(item: &Value) -> Option<ModelStreamEvent> {
    ResponsesContextItem::from_wire(item.clone())
        .map(|item| ModelStreamEvent::ResponsesContextItem { item })
}

fn tool_caller_event(item: &Value, item_id: &str) -> Option<ModelStreamEvent> {
    let caller = item.get("caller")?.clone();
    let caller = serde_json::from_value::<ToolCallCaller>(caller).ok()?;
    Some(ModelStreamEvent::ToolCallCaller {
        item_id: item_id.to_string(),
        caller,
    })
}

pub(super) fn web_search_lifecycle_event(
    event: &super::SseStreamEvent,
) -> Option<ModelStreamEvent> {
    let item_id = event.item_id.clone().unwrap_or_default();
    match event.kind.as_str() {
        "response.web_search_call.in_progress" | "response.web_search_call.searching" => {
            Some(ModelStreamEvent::WebSearchStarted {
                item_id,
                action: WebSearchAction::Other,
            })
        }
        "response.web_search_call.completed" => Some(ModelStreamEvent::WebSearchCompleted {
            item_id,
            action: WebSearchAction::Other,
            results: None,
        }),
        _ => None,
    }
}

fn web_search_action(value: Option<&Value>) -> WebSearchAction {
    let Some(value) = value else {
        return WebSearchAction::Other;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("search") => WebSearchAction::Search {
            query: value
                .get("query")
                .and_then(Value::as_str)
                .map(str::to_string),
            queries: value
                .get("queries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        },
        Some("open_page") | Some("openPage") => WebSearchAction::OpenPage {
            url: value.get("url").and_then(Value::as_str).map(str::to_string),
        },
        Some("find_in_page") | Some("findInPage") => WebSearchAction::FindInPage {
            url: value.get("url").and_then(Value::as_str).map(str::to_string),
            pattern: value
                .get("pattern")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        _ => WebSearchAction::Other,
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

pub(super) fn assistant_message_parts(item: &Value) -> Vec<(u32, String)> {
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, part)| {
            if part.get("type").and_then(Value::as_str) != Some("output_text") {
                return None;
            }
            Some((
                u32::try_from(index).ok()?,
                part.get("text")?.as_str()?.to_owned(),
            ))
        })
        .collect()
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

pub(super) fn presentation_item(
    item: &Value,
    output_index: Option<u32>,
) -> Option<CompletionPresentationItem> {
    let provider_item_id = item.get("id")?.as_str()?.to_owned();
    let kind = match item.get("type")?.as_str()? {
        "message" => {
            CompletionPresentationItemKind::Text(assistant_message_identity(Some(item))?.1)
        }
        "reasoning" => CompletionPresentationItemKind::Reasoning,
        _ => return None,
    };
    let mut parts = Vec::new();
    for (field, part_kind) in [
        ("content", CompletionPresentationPartKind::ReasoningText),
        ("summary", CompletionPresentationPartKind::SummaryText),
    ] {
        if let Some(content) = item.get(field).and_then(Value::as_array) {
            for (index, part) in content.iter().enumerate() {
                let kind = match part.get("type").and_then(Value::as_str) {
                    Some("output_text") if field == "content" => {
                        CompletionPresentationPartKind::OutputText
                    }
                    Some("reasoning_text") if field == "content" => {
                        CompletionPresentationPartKind::ReasoningText
                    }
                    Some("summary_text" | "reasoning_summary_text") if field == "summary" => {
                        part_kind
                    }
                    _ => continue,
                };
                if let (Ok(content_index), Some(text)) = (
                    u32::try_from(index),
                    part.get("text").and_then(Value::as_str),
                ) {
                    parts.push(CompletionPresentationPart {
                        content_index,
                        provider_part_id: part.get("id").and_then(Value::as_str).map(str::to_owned),
                        kind,
                        text: text.to_owned(),
                    });
                }
            }
        }
    }
    Some(CompletionPresentationItem {
        provider_item_id,
        output_index,
        kind,
        parts,
    })
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
