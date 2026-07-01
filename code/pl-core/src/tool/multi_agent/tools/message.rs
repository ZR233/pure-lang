use pl_protocol::{AgentStatus, PureError};
use pl_trace::AgentEvent;

use crate::agent::{AgentMailboxMessage, MessageDeliveryMode};

use super::super::events::emit_agent_record;
use super::super::runner::run_agent_turn;
use super::super::types::{AgentMessageArgs, FollowupTaskTool, MessageResult, SendMessageTool};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, child_agent_options, current_agent_path,
    json_output, message_schema, unix_seconds,
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
            handle_message_tool(
                input,
                context,
                MessageDeliveryMode::QueueOnly,
                "send_message",
            )
            .await
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
            let output = handle_message_tool(
                input,
                context.clone(),
                MessageDeliveryMode::TriggerTurn,
                "followup_task",
            )
            .await?;
            let MessageResult { target, .. } =
                serde_json::from_str(&output.description).map_err(|error| {
                    PureError::ToolExecutionFailed {
                        tool: "followup_task".to_string(),
                        error: format!("invalid followup result: {error}"),
                    }
                })?;
            let agent_id = context
                .agent_control
                .resolve_agent(&current_agent_path(&context), &target)
                .await?;
            if let Some(messages) = context.agent_control.take_turn_messages(&agent_id).await
                && let Some(record) = context.agent_control.record(&agent_id).await
            {
                let run = self.runtime.run_config(
                    &context,
                    &record,
                    child_agent_options(&context.options),
                    record.role.clone(),
                    followup_prompt(messages),
                );
                if let Some(token) = run.options.cancellation_token.clone() {
                    context
                        .agent_control
                        .attach_cancellation_token(&agent_id, token)
                        .await;
                }
                tokio::spawn(run_agent_turn(run));
            }
            Ok(output)
        })
    }
}

pub(in crate::tool::multi_agent) fn followup_prompt(messages: Vec<AgentMailboxMessage>) -> String {
    let multiple = messages.len() > 1;
    let mut prompt = String::new();
    for (index, message) in messages.into_iter().enumerate() {
        if index > 0 {
            prompt.push_str("\n\n");
        }
        if multiple {
            if message.trigger_turn {
                prompt.push_str("Follow-up task");
            } else {
                prompt.push_str("Queued message from ");
                prompt.push_str(&message.sender_path);
            }
            prompt.push_str(":\n");
        }
        prompt.push_str(&message.message);
    }
    prompt
}

async fn handle_message_tool(
    input: ToolInput,
    context: ToolContext,
    mode: MessageDeliveryMode,
    tool: &str,
) -> Result<ToolOutput, PureError> {
    let args: AgentMessageArgs = serde_json::from_value(input.arguments).map_err(|error| {
        PureError::ToolExecutionFailed {
            tool: tool.to_string(),
            error: format!("invalid input: {error}"),
        }
    })?;
    let sender_path = current_agent_path(&context);
    let _ = context
        .event_tx
        .send(AgentEvent::CollabAgentInteractionBegin {
            call_id: input.tool_id.clone(),
            started_at: unix_seconds(),
            sender_path: sender_path.clone(),
            receiver_path: args.target.clone(),
            prompt: args.message.clone(),
        });
    let record = context
        .agent_control
        .append_message(&sender_path, &args.target, args.message.clone(), mode)
        .await;
    let record = match record {
        Ok(record) => {
            let _ = context
                .event_tx
                .send(AgentEvent::CollabAgentInteractionEnd {
                    call_id: input.tool_id,
                    completed_at: unix_seconds(),
                    sender_path,
                    receiver_path: record.path.clone(),
                    status: record.status,
                    prompt: args.message,
                    error: None,
                });
            record
        }
        Err(error) => {
            let _ = context
                .event_tx
                .send(AgentEvent::CollabAgentInteractionEnd {
                    call_id: input.tool_id,
                    completed_at: unix_seconds(),
                    sender_path,
                    receiver_path: args.target,
                    status: AgentStatus::NotFound,
                    prompt: args.message,
                    error: Some(error.to_string()),
                });
            return Err(error);
        }
    };
    emit_agent_record(&context.event_tx, &record);
    json_output(MessageResult {
        target: record.path,
        status: record.status,
    })
}
