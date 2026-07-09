use std::fmt;

use pl_protocol::{PureError, Result};
use serde_json::{Value, json};

use crate::tool::shell::shell_quote_word;

pub(crate) fn object_schema(fields: Vec<(&str, Value, bool)>) -> Value {
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

pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    arguments: Value,
    tool: &str,
) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| tool_error(tool, format!("invalid input: {error}")))
}

pub(crate) fn shell_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shell_quote(value: &str) -> String {
    shell_quote_word(value)
}

pub(crate) fn preview_error(stderr: &str, stdout: &str) -> String {
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

pub(crate) fn bounded_text(
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

pub(crate) fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{ShellCommandTimeout, shell_command_with_timeout};

    #[test]
    fn shell_command_timeout_helper_wraps_only_positive_timeout() {
        assert_eq!(
            shell_command_with_timeout("sleep 1000", ShellCommandTimeout::Disabled),
            "sleep 1000"
        );
        assert_eq!(
            shell_command_with_timeout("sleep 1000", ShellCommandTimeout::Seconds(0)),
            "sleep 1000"
        );
        assert_eq!(
            shell_command_with_timeout("sleep 1000", ShellCommandTimeout::Seconds(5)),
            "timeout --preserve-status 5s /bin/sh -lc 'sleep 1000'"
        );
    }
}
