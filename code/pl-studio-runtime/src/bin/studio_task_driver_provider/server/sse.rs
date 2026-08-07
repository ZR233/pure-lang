pub(super) fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> String {
    let item_id = format!("fc_{id}");
    let call_id = format!("call_{id}");
    let arguments = arguments.to_string();
    events([
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name
            }
        }),
        serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "item_id": item_id,
            "call_id": call_id,
            "delta": arguments
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": arguments
            }
        }),
        completed(id),
    ])
}

pub(super) fn final_text(id: &str, content: &str) -> String {
    let item_id = format!("msg_{id}");
    events([
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer"
            }
        }),
        serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "delta": content
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [{"type": "output_text", "text": content}]
            }
        }),
        completed(id),
    ])
}

fn completed(id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": format!("response_{id}"),
            "usage": {
                "input_tokens": 200,
                "input_tokens_details": {"cached_tokens": 100},
                "output_tokens": 12,
                "total_tokens": 212
            }
        }
    })
}

fn events<const N: usize>(events: [serde_json::Value; N]) -> String {
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect()
}
