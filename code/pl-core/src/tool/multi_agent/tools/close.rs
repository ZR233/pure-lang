use pl_protocol::PureError;

use super::super::types::{CloseAgentArgs, CloseAgentTool, MessageResult};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, current_agent_path, json_output,
};

impl Tool for CloseAgentTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn description(&self) -> &str {
        "Close an existing managed sub-agent. The root agent cannot be closed."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Agent id, relative path, or canonical path."
                }
            },
            "required": ["target"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: CloseAgentArgs =
                serde_json::from_value(input.arguments).map_err(|error| {
                    PureError::ToolExecutionFailed {
                        tool: "close_agent".to_string(),
                        error: format!("invalid input: {error}"),
                    }
                })?;
            let sender_path = current_agent_path(&context);
            let record = context
                .agent_supervisor
                .close_agent(
                    &sender_path,
                    &args.target,
                    "closed by close_agent",
                    &context.event_tx,
                    input.tool_id,
                )
                .await?;
            json_output(MessageResult {
                target: record.path,
                status: record.status,
            })
        })
    }
}
