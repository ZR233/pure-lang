use pl_protocol::{PureError, Result};

pub(crate) fn parse_function_tool_arguments(
    arguments: &str,
    tool_name: &str,
) -> Result<serde_json::Value> {
    serde_json::from_str(arguments).map_err(|error| {
        PureError::LlmError(format!(
            "provider emitted invalid JSON arguments for function tool {tool_name}: {error}"
        ))
    })
}
