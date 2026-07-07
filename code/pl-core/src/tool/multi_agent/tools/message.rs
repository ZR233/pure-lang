use pl_protocol::PureError;

use crate::agent::{AgentMessageMode, AgentMessageRequest};

use super::super::schema::AgentControlToolKind;
use super::super::types::{AgentMessageArgs, SendInputResult, SendInputTool};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, child_agent_options, current_agent_path,
    json_output,
};

impl Tool for SendInputTool {
    fn name(&self) -> &str {
        "send_input"
    }

    fn description(&self) -> &str {
        "Send input to an existing agent. Defaults to queueing the input; set triggerTurn=true to start a new turn for a waiting child agent."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::SendInput.input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: AgentMessageArgs =
                serde_json::from_value(input.arguments).map_err(|error| {
                    PureError::ToolExecutionFailed {
                        tool: "send_input".to_string(),
                        error: format!("invalid input: {error}"),
                    }
                })?;
            let mode = if args.trigger_turn {
                AgentMessageMode::TriggerTurn
            } else {
                AgentMessageMode::QueueOnly
            };
            let run_spec = if args.trigger_turn {
                Some(self.runtime.run_config(
                    &context,
                    child_agent_options(&context.options),
                    input.tool_id.clone(),
                    String::new(),
                    crate::CoreSession::new(),
                ))
            } else {
                None
            };
            handle_message_tool(input.tool_id, context, args, mode, run_spec).await
        })
    }
}

async fn handle_message_tool(
    tool_id: String,
    context: ToolContext,
    args: AgentMessageArgs,
    mode: AgentMessageMode,
    run_spec: Option<crate::AgentRunSpec>,
) -> Result<ToolOutput, PureError> {
    let sender_path = current_agent_path(&context);
    let record = context
        .agent_supervisor
        .send_message(AgentMessageRequest {
            current_path: &sender_path,
            target: &args.target,
            message: args.message,
            mode,
            run_spec,
            event_tx: &context.event_tx,
            call_id: tool_id,
        })
        .await?;
    json_output(SendInputResult {
        target: record.path,
        status: record.status,
        interrupt: args.interrupt,
    })
}
