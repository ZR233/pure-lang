use std::fmt;

use pl_protocol::{PureError, Result};
use serde_json::{Value, json};

const TOKEN_ESTIMATE_BYTES: usize = 4;

pub(super) fn object_schema(fields: Vec<(&str, Value, bool)>) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in fields {
        properties.insert(name.to_string(), schema);
        if is_required {
            required.push(Value::String(name.to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(super) fn parse_input<T: serde::de::DeserializeOwned>(
    arguments: Value,
    tool: &str,
) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| tool_error(tool, format!("invalid input: {error}")))
}

pub(super) fn shell_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(super) fn preview_error(stderr: &str, stdout: &str) -> String {
    preview(format!("{stderr}\n{stdout}").trim(), 500)
}

fn preview(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

pub(super) fn bounded_text(
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

pub(super) fn bounded_model_tool_output_with_tokens(output: &str, max_tokens: usize) -> String {
    let max_bytes = max_tokens.saturating_mul(TOKEN_ESTIMATE_BYTES).max(1);
    if output.len() <= max_bytes {
        return output.to_string();
    }
    let mut end = max_bytes;
    while !output.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!(
        "{}\n...[truncated {} bytes]",
        &output[..end],
        output.len().saturating_sub(end)
    )
}

pub(super) fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}
