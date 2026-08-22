//! 模型请求中工具与 function call 输出的解析 helper。

pub(super) fn tool_name(tool: &serde_json::Value) -> Option<&str> {
    tool.get("name")
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(serde_json::Value::as_str)
}

pub(super) fn function_call_outputs(request: &serde_json::Value) -> impl Iterator<Item = &str> {
    let responses_outputs = request
        .get("input")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("function_call_output")
        })
        .filter_map(|item| item.get("output").and_then(serde_json::Value::as_str));
    let chat_completions_outputs = request
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .filter(|message| message.get("role").and_then(serde_json::Value::as_str) == Some("tool"))
        .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str));

    responses_outputs.chain(chat_completions_outputs)
}

pub(super) fn parse_output(output: &str) -> Option<serde_json::Value> {
    serde_json::from_str(output).ok().or_else(|| {
        let start = output.find('{')?;
        let end = output.rfind('}')?;
        serde_json::from_str(&output[start..=end]).ok()
    })
}

pub(super) fn find_string_field(value: &serde_json::Value, field: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => object
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                object
                    .values()
                    .find_map(|value| find_string_field(value, field))
            }),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| find_string_field(value, field)),
        _ => None,
    }
}
