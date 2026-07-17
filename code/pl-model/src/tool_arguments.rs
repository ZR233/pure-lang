use crate::request::ToolCall;

pub(crate) fn function_tool_call_from_raw(
    id: String,
    tool_name: String,
    arguments: String,
    call_id: Option<String>,
) -> ToolCall {
    match serde_json::from_str(&arguments) {
        Ok(arguments) => ToolCall::function(id, tool_name, arguments, call_id),
        Err(error) => {
            ToolCall::invalid_function(id, tool_name, arguments, error.to_string(), call_id)
        }
    }
}
