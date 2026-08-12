use std::fmt;

use pl_protocol::PureError;

use crate::tool::shell::shell_quote_word;

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

pub(crate) fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}
