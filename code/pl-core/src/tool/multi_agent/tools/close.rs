use pl_protocol::PureError;

use crate::agent::worktree::CloseDisposition;

use super::super::schema::AgentControlToolKind;
use super::super::types::{CloseAgentArgs, CloseAgentTool, MessageResult, ResumeAgentTool};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, current_agent_path, json_output,
};

impl Tool for CloseAgentTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn description(&self) -> &str {
        "Close an existing managed sub-agent. The root agent cannot be closed. \
         Set merge=true to merge the sub-agent's worktree branch back into the main \
         workspace; otherwise its changes are discarded."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::CloseAgent.input_schema()
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
            let disposition = if args.merge {
                CloseDisposition::Merge {
                    target_branch: None,
                }
            } else {
                CloseDisposition::Discard
            };
            let record = context
                .agent_supervisor
                .close_agent(
                    &sender_path,
                    &args.target,
                    "closed by close_agent",
                    &context.event_tx,
                    input.tool_id,
                    disposition,
                )
                .await?;
            json_output(MessageResult {
                target: record.path,
                status: record.status,
            })
        })
    }
}

impl Tool for ResumeAgentTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn description(&self) -> &str {
        "Resume a closed managed sub-agent."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::ResumeAgent.input_schema()
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
                        tool: "resume_agent".to_string(),
                        error: format!("invalid input: {error}"),
                    }
                })?;
            let sender_path = current_agent_path(&context);
            let record = context
                .agent_supervisor
                .resume_agent(&sender_path, &args.target, &context.event_tx)
                .await?;
            json_output(MessageResult {
                target: record.path,
                status: record.status,
            })
        })
    }
}
