use pl_protocol::{AgentStatus, PureError};
use pl_trace::AgentEvent;

use crate::agent::AgentSpawnInput;
use crate::tool::recoverable::{
    is_recoverable_subagent_capacity_error, recoverable_subagent_tool_output,
};

use super::super::events::emit_agent_record;
use super::super::runner::run_agent_turn;
use super::super::types::{SpawnAgentArgs, SpawnAgentResult, SpawnAgentTool};
use super::super::{
    BoxFuture, ForkTurns, Tool, ToolContext, ToolInput, ToolOutput, child_agent_options,
    current_agent_path, fork_session, invalid_spawn_input, json_output, role_key, unix_seconds,
};

impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Spawn a managed sub-agent for an independent task. Use this when \
         the user asks for subagents, per-crate exploration, parallel work, \
         or isolated context. The spawned agent runs asynchronously; use \
         wait_agent to observe completion."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskName": {
                    "type": "string",
                    "description": "Stable lowercase task name using letters, digits, and underscores."
                },
                "message": {
                    "type": "string",
                    "description": "Initial task message for the spawned agent."
                },
                "agentType": {
                    "type": "string",
                    "enum": ["explorer", "planner", "executor", "reviewer"],
                    "description": "Agent role. Defaults to executor."
                },
                "model": {
                    "type": "string",
                    "description": "Reserved model override; omitted to inherit parent model."
                },
                "reasoningEffort": {
                    "type": "string",
                    "description": "Reserved reasoning override."
                },
                "forkTurns": {
                    "type": "string",
                    "description": "Parent history to inherit: none, all, or a positive integer string. Defaults to none. Inherited history is filtered to remove tool calls/results and reasoning."
                }
            },
            "required": ["taskName", "message"]
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: SpawnAgentArgs =
                serde_json::from_value(input.arguments).map_err(invalid_spawn_input)?;
            let role = role_key(args.agent_type.as_deref())?;
            let fork_turns = ForkTurns::parse(args.fork_turns.as_deref())?;
            let parent_path = current_agent_path(&context);
            let prompt = args.message.clone();
            let _ = context.event_tx.send(AgentEvent::CollabAgentSpawnBegin {
                call_id: input.tool_id.clone(),
                started_at: unix_seconds(),
                sender_path: parent_path.clone(),
                task_name: args.task_name.clone(),
                prompt: prompt.clone(),
                role: role.clone(),
                model: args.model.clone(),
                reasoning_effort: args.reasoning_effort.clone(),
            });
            let spawn_result = context
                .agent_control
                .spawn_agent(AgentSpawnInput {
                    task_name: args.task_name.clone(),
                    message: prompt.clone(),
                    role: role.clone(),
                    parent_path: Some(parent_path.clone()),
                })
                .await;
            let handle = match spawn_result {
                Ok(handle) => {
                    let _ = context.event_tx.send(AgentEvent::CollabAgentSpawnEnd {
                        call_id: input.tool_id.clone(),
                        completed_at: unix_seconds(),
                        sender_path: parent_path,
                        agent_id: Some(handle.id.clone()),
                        path: Some(handle.path.clone()),
                        role: Some(role.clone()),
                        status: AgentStatus::Queued,
                        prompt: prompt.clone(),
                        error: None,
                    });
                    handle
                }
                Err(error) => {
                    let error_message = error.to_string();
                    let _ = context.event_tx.send(AgentEvent::CollabAgentSpawnEnd {
                        call_id: input.tool_id,
                        completed_at: unix_seconds(),
                        sender_path: parent_path,
                        agent_id: None,
                        path: None,
                        role: Some(role),
                        status: AgentStatus::NotFound,
                        prompt,
                        error: Some(error_message.clone()),
                    });
                    if is_recoverable_subagent_capacity_error(&error_message) {
                        return Ok(recoverable_subagent_tool_output(
                            &args.message,
                            &error_message,
                        ));
                    }
                    return Err(error);
                }
            };
            let record = context
                .agent_control
                .record(&handle.id)
                .await
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "spawn_agent".to_string(),
                    error: "spawned agent disappeared".to_string(),
                })?;
            emit_agent_record(&context.event_tx, &record);

            let child_session = fork_session(&context.parent_session, fork_turns);
            context
                .agent_control
                .store_session(&handle.id, child_session)
                .await;

            let options = child_agent_options(&context.options);
            if let Some(token) = options.cancellation_token.clone() {
                context
                    .agent_control
                    .attach_cancellation_token(&handle.id, token)
                    .await;
            }
            let run = self
                .runtime
                .run_config(&context, &record, options, role, prompt);
            tokio::spawn(run_agent_turn(run));

            json_output(SpawnAgentResult {
                agent_id: handle.id,
                task_name: args.task_name,
                path: handle.path,
                status: AgentStatus::Queued,
            })
        })
    }
}
