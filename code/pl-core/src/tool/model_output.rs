use serde_json::{Value, json};

const TOKEN_ESTIMATE_BYTES: usize = 4;

pub const DEFAULT_MODEL_TOOL_OUTPUT_TOKENS: usize = 3_000;
pub const MAX_MODEL_TOOL_OUTPUT_BYTES: usize = 12 * 1024;

pub fn model_visible_tool_output(output: &str) -> String {
    model_visible_tool_output_with_tokens(output, DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
}

pub fn model_visible_tool_output_with_tokens(output: &str, max_output_tokens: usize) -> String {
    let max_bytes = max_output_tokens
        .saturating_mul(TOKEN_ESTIMATE_BYTES)
        .clamp(1, MAX_MODEL_TOOL_OUTPUT_BYTES);
    if output.len() <= max_bytes {
        return output.to_string();
    }
    let projected = if let Ok(value) = serde_json::from_str::<Value>(output) {
        bounded_json_tool_output(value, max_bytes).to_string()
    } else {
        let (text, truncated, bytes_omitted, next_offset) = bounded_text(output, max_bytes, 0);
        json!({
            "truncated": truncated,
            "bytesReturned": text.len(),
            "bytesOmitted": bytes_omitted,
            "nextOffset": next_offset,
            "text": text,
        })
        .to_string()
    };
    enforce_model_output_limit(&projected, MAX_MODEL_TOOL_OUTPUT_BYTES)
}

/// 对所有工具输出执行最终字节预算，任何工具或产品 adapter 都不能绕过。
pub fn enforce_model_output_limit(output: &str, requested_max_bytes: usize) -> String {
    let max_bytes = requested_max_bytes.clamp(1, MAX_MODEL_TOOL_OUTPUT_BYTES);
    if output.len() <= max_bytes {
        return output.to_string();
    }
    if serde_json::from_str::<Value>(output).is_ok() {
        let mut preview_bytes = max_bytes.saturating_sub(192).max(1);
        loop {
            let head_budget = preview_bytes / 3;
            let tail_budget = preview_bytes - head_budget;
            let preview = format!(
                "{}\n… truncated …\n{}",
                utf8_prefix(output, head_budget),
                utf8_suffix(output, tail_budget)
            );
            let candidate = json!({
                "truncated": true,
                "bytesReturned": preview.len(),
                "bytesOmitted": output.len().saturating_sub(preview.len()),
                "nextOffset": preview.len(),
                "jsonPreview": preview,
            })
            .to_string();
            if candidate.len() <= max_bytes {
                return candidate;
            }
            if preview_bytes <= 1 {
                return "{}".to_string();
            }
            preview_bytes = (preview_bytes / 2).max(1);
        }
    }
    const MARKER: &str = "\n… output truncated by pl-core …\n";
    if max_bytes <= MARKER.len() {
        return utf8_prefix(MARKER, max_bytes).to_string();
    }
    let available = max_bytes - MARKER.len();
    let head_budget = available / 3;
    let tail_budget = available - head_budget;
    let head = utf8_prefix(output, head_budget);
    let tail = utf8_suffix(output, tail_budget);
    format!("{head}{MARKER}{tail}")
}

fn bounded_json_tool_output(mut value: Value, max_bytes: usize) -> Value {
    match &mut value {
        Value::Object(map) => {
            for key in [
                "stdout",
                "stderr",
                "body",
                "text",
                "tarBase64",
                "contentBase64",
            ] {
                if let Some(Value::String(text)) = map.get_mut(key) {
                    let (bounded, truncated, bytes_omitted, next_offset) =
                        bounded_text(text, max_bytes, 0);
                    if truncated {
                        let bytes_returned = bounded.len();
                        *text = bounded;
                        map.insert("truncated".to_string(), Value::Bool(true));
                        map.insert("bytesReturned".to_string(), json!(bytes_returned));
                        map.insert("bytesOmitted".to_string(), json!(bytes_omitted));
                        map.insert("nextOffset".to_string(), json!(next_offset));
                        break;
                    }
                }
            }
            if serde_json::to_string(&*map).map_or(0, |value| value.len()) > max_bytes {
                let array_count = map.values().filter(|value| value.is_array()).count().max(1);
                let array_budget = max_bytes.saturating_sub(256) / array_count;
                for value in map.values_mut() {
                    let Value::Array(items) = value else {
                        continue;
                    };
                    if serde_json::to_string(&*items).map_or(0, |value| value.len()) > array_budget
                    {
                        *value = bounded_json_array(std::mem::take(items), array_budget);
                    }
                }
            }
            if value.to_string().len() > max_bytes {
                json_preview(value, max_bytes)
            } else {
                value
            }
        }
        Value::Array(items) => bounded_json_array(std::mem::take(items), max_bytes),
        Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null => {
            let serialized = value.to_string();
            if serialized.len() <= max_bytes {
                value
            } else {
                json_preview(Value::String(serialized), max_bytes)
            }
        }
    }
}

fn bounded_json_array(items: Vec<Value>, max_bytes: usize) -> Value {
    let total = items.len();
    let item_budget = max_bytes.saturating_sub(192).max(1);
    let mut retained = Vec::new();
    let mut used = 0usize;
    for item in items {
        let mut serialized = item.to_string();
        let item = if serialized.len() > item_budget {
            let preview = json_preview(item, item_budget);
            serialized = preview.to_string();
            preview
        } else {
            item
        };
        if !retained.is_empty() && used.saturating_add(serialized.len()) > item_budget {
            break;
        }
        used = used.saturating_add(serialized.len());
        retained.push(item);
    }
    let returned = retained.len();
    json!({
        "truncated": returned < total,
        "itemsReturned": returned,
        "itemsOmitted": total.saturating_sub(returned),
        "items": retained,
    })
}

fn json_preview(value: Value, max_bytes: usize) -> Value {
    let serialized = match value {
        Value::String(text) => text,
        other => other.to_string(),
    };
    let (text, _, bytes_omitted, next_offset) = bounded_text(&serialized, max_bytes, 0);
    json!({
        "truncated": true,
        "bytesReturned": text.len(),
        "bytesOmitted": bytes_omitted,
        "nextOffset": next_offset,
        "jsonPreview": text,
    })
}

fn bounded_text(
    value: &str,
    max_bytes: usize,
    offset: usize,
) -> (String, bool, usize, Option<usize>) {
    if value.len() <= max_bytes {
        return (value.to_string(), false, 0, None);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let text = value[..end].to_string();
    let omitted = value.len().saturating_sub(end);
    (text, true, omitted, Some(offset.saturating_add(end)))
}

fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

fn utf8_suffix(value: &str, max_bytes: usize) -> &str {
    let mut start = value.len().saturating_sub(max_bytes);
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}
