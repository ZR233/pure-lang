pub(super) fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> String {
    tool_calls(id, [(id.to_string(), name.to_string(), arguments)])
}

fn tool_calls(
    response_id: &str,
    calls: impl IntoIterator<Item = (String, String, serde_json::Value)>,
) -> String {
    let mut events = Vec::new();
    for (id, name, arguments) in calls {
        let item_id = format!("fc_{id}");
        let call_id = format!("call_{id}");
        let arguments = arguments.to_string();
        events.extend([
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
        ]);
    }
    events.push(serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": format!("response_{response_id}"),
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
        }
    }));
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect()
}
