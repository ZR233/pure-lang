use serde_json::Value;

use crate::WebSearchAction;
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
    let (item_id, call_id) = responses_tool_identity(item);
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
        "web_search_call" => Some(ModelStreamEvent::WebSearchStarted {
            item_id,
            action: web_search_action(item.get("action")),
        }),
        _ => None,
    }
}

pub(super) fn output_item_tool_completed(item: &Value) -> Option<Vec<ModelStreamEvent>> {
    let kind = item.get("type")?.as_str()?;
    let (item_id, call_id) = responses_tool_identity(item);
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
        "web_search_call" => Some(vec![ModelStreamEvent::WebSearchCompleted {
            item_id,
            action: web_search_action(item.get("action")),
            results: item.get("results").and_then(Value::as_array).cloned(),
        }]),
        _ => None,
    }
}

fn responses_tool_identity(item: &Value) -> (String, Option<String>) {
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            item.get("call_id")
                .and_then(Value::as_str)
                .filter(|call_id| !call_id.is_empty())
        })
        .unwrap_or_default()
        .to_string();
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|call_id| !call_id.is_empty())
        .map(String::from)
        .or_else(|| (!item_id.is_empty()).then(|| item_id.clone()));
    (item_id, call_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_items_decode_search_open_find_and_unknown_actions() {
        let cases = [
            (
                serde_json::json!({"type": "search", "query": "rust"}),
                WebSearchAction::Search {
                    query: Some("rust".to_string()),
                    queries: Vec::new(),
                },
            ),
            (
                serde_json::json!({
                    "type": "open_page",
                    "url": "https://example.com"
                }),
                WebSearchAction::OpenPage {
                    url: Some("https://example.com".to_string()),
                },
            ),
            (
                serde_json::json!({
                    "type": "find_in_page",
                    "url": "https://example.com",
                    "pattern": "needle"
                }),
                WebSearchAction::FindInPage {
                    url: Some("https://example.com".to_string()),
                    pattern: Some("needle".to_string()),
                },
            ),
            (
                serde_json::json!({"type": "future_action"}),
                WebSearchAction::Other,
            ),
        ];

        for (action, expected) in cases {
            let started = output_item_tool_started(&serde_json::json!({
                "type": "web_search_call",
                "id": "ws_1",
                "action": action
            }))
            .expect("started event");
            assert!(matches!(
                started,
                ModelStreamEvent::WebSearchStarted { item_id, action }
                    if item_id == "ws_1" && action == expected
            ));
        }
    }

    #[test]
    fn completed_web_search_item_preserves_opaque_results() {
        let events = output_item_tool_completed(&serde_json::json!({
            "type": "web_search_call",
            "id": "ws_1",
            "action": {"type": "search", "query": "rust"},
            "results": [{"url": "https://example.com", "future": {"rank": 1}}]
        }))
        .expect("completed events");

        assert!(matches!(
            &events[0],
            ModelStreamEvent::WebSearchCompleted {
                item_id,
                results: Some(results),
                ..
            } if item_id == "ws_1" && results[0]["future"]["rank"] == 1
        ));
    }
}
