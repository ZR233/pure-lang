use pl_protocol::{AgentStatus, PureError};
use pl_trace::AgentEvent;

use super::super::events::emit_agent_record;
use super::super::types::{CloseAgentArgs, CloseAgentTool, MessageResult};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, current_agent_path, json_output,
    unix_seconds,
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
            let _ = context.event_tx.send(AgentEvent::CollabCloseBegin {
                call_id: input.tool_id.clone(),
                started_at: unix_seconds(),
                sender_path: sender_path.clone(),
                receiver_path: args.target.clone(),
            });
            let previous = context
                .agent_control
                .close_agent(&sender_path, &args.target)
                .await;
            let previous = match previous {
                Ok(previous) => previous,
                Err(error) => {
                    let _ = context.event_tx.send(AgentEvent::CollabCloseEnd {
                        call_id: input.tool_id,
                        completed_at: unix_seconds(),
                        sender_path,
                        receiver_path: args.target,
                        status: AgentStatus::NotFound,
                        error: Some(error.to_string()),
                    });
                    return Err(error);
                }
            };
            let shutdown = context
                .agent_control
                .record(&previous.id)
                .await
                .unwrap_or_else(|| previous.clone());
            emit_agent_record(&context.event_tx, &shutdown);
            let _ = context.event_tx.send(AgentEvent::CollabCloseEnd {
                call_id: input.tool_id,
                completed_at: unix_seconds(),
                sender_path,
                receiver_path: previous.path.clone(),
                status: shutdown.status,
                error: None,
            });
            json_output(MessageResult {
                target: previous.path,
                status: shutdown.status,
            })
        })
    }
}
