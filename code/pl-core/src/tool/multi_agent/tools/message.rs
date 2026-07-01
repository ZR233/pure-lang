use pl_protocol::PureError;

use crate::agent::{AgentMessageMode, AgentMessageRequest};

use super::super::types::{AgentMessageArgs, FollowupTaskTool, MessageResult, SendMessageTool};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, child_agent_options, current_agent_path,
    json_output, message_schema,
};

impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Queue a message for an existing agent without starting a new turn."
    }

    fn input_schema(&self) -> serde_json::Value {
        message_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            handle_message_tool(input, context, AgentMessageMode::QueueOnly, None).await
        })
    }
}

impl Tool for FollowupTaskTool {
    fn name(&self) -> &str {
        "followup_task"
    }

    fn description(&self) -> &str {
        "Send a follow-up task to an existing non-root agent and trigger a new turn."
    }

    fn input_schema(&self) -> serde_json::Value {
        message_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let run_spec = self.runtime.run_config(
                &context,
                child_agent_options(&context.options),
                input.tool_id.clone(),
                String::new(),
                crate::CoreSession::new(),
            );
            handle_message_tool(
                input,
                context,
                AgentMessageMode::TriggerTurn,
                Some(run_spec),
            )
            .await
        })
    }
}

async fn handle_message_tool(
    input: ToolInput,
    context: ToolContext,
    mode: AgentMessageMode,
    run_spec: Option<crate::AgentRunSpec>,
) -> Result<ToolOutput, PureError> {
    let tool = if mode == AgentMessageMode::TriggerTurn {
        "followup_task"
    } else {
        "send_message"
    };
    let args: AgentMessageArgs = serde_json::from_value(input.arguments).map_err(|error| {
        PureError::ToolExecutionFailed {
            tool: tool.to_string(),
            error: format!("invalid input: {error}"),
        }
    })?;
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
            call_id: input.tool_id,
        })
        .await?;
    json_output(MessageResult {
        target: record.path,
        status: record.status,
    })
}
