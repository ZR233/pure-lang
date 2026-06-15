use pl_protocol::{AgentStatus, PureError};
use pl_trace::AgentEvent;

use super::events::emit_agent_record;
use super::runner::run_agent_turn;
use super::types::{
    AgentMessageArgs, AgentRunConfig, CloseAgentArgs, FollowupTaskTool, ListAgentsArgs,
    ListAgentsResult, ListAgentsTool, MessageResult, SendMessageTool, SpawnAgentArgs,
    SpawnAgentResult, SpawnAgentTool, WaitAgentArgs, WaitAgentResult, WaitAgentTool,
};
use super::{
    CloseAgentTool, ForkTurns, Tool, ToolContext, ToolInput, ToolOutput, agent_tool_records,
    child_agent_options, current_agent_path, fork_session, invalid_spawn_input, json_output,
    message_schema, unix_seconds,
};
use crate::agent::{AgentSpawnInput, MessageDeliveryMode};
use crate::tool::recoverable::{
    is_recoverable_subagent_capacity_error, recoverable_subagent_failures,
    recoverable_subagent_failures_message, recoverable_subagent_tool_output,
};
use crate::turn::TurnBudget;

impl SpawnAgentTool {
    pub fn new(
        provider: pl_model::SharedModelProvider,
        reasoning_effort: Option<crate::config::ReasoningEffort>,
        config: Option<crate::config::PureConfig>,
        mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
        lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            mcp_runtime,
            lsp_runtime,
            workspace_instructions,
        }
    }
}

impl FollowupTaskTool {
    pub fn new(
        provider: pl_model::SharedModelProvider,
        reasoning_effort: Option<crate::config::ReasoningEffort>,
        config: Option<crate::config::PureConfig>,
        mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
        lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            mcp_runtime,
            lsp_runtime,
            workspace_instructions,
        }
    }
}

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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: SpawnAgentArgs =
                serde_json::from_value(input.arguments).map_err(invalid_spawn_input)?;
            let role = super::role_key(args.agent_type.as_deref())?;
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
            let run = AgentRunConfig {
                provider: self.provider.clone(),
                reasoning_effort: self.reasoning_effort.clone(),
                config: self.config.clone(),
                mcp_runtime: self.mcp_runtime.clone(),
                lsp_runtime: self.lsp_runtime.clone(),
                workspace_instructions: context
                    .workspace_instructions
                    .clone()
                    .or_else(|| self.workspace_instructions.clone()),
                instruction_snapshot: context.instruction_snapshot.clone(),
                workspace_root: context.workspace_root.clone(),
                options,
                agent_control: context.agent_control.clone(),
                event_tx: context.event_tx.clone(),
                agent_id: handle.id.clone(),
                agent_path: handle.path.clone(),
                role,
                message: prompt,
                mode: context.mode,
                budget: TurnBudget::child_default(),
            };
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

impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait for managed sub-agent activity or completion. Use this after spawning agents."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeoutMs": {
                    "type": "integer",
                    "description": "Wait timeout in milliseconds. Defaults to 30000."
                },
                "includeDetails": {
                    "type": "boolean",
                    "description": "Return full AgentRecord entries instead of compact summaries. Defaults to false."
                }
            }
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: WaitAgentArgs =
                serde_json::from_value(input.arguments).unwrap_or(WaitAgentArgs {
                    timeout_ms: None,
                    include_details: false,
                });
            let sender_path = current_agent_path(&context);
            let _ = context.event_tx.send(AgentEvent::CollabWaitingBegin {
                call_id: input.tool_id.clone(),
                started_at: unix_seconds(),
                sender_path: sender_path.clone(),
            });
            let outcome = context
                .agent_control
                .wait_for_activity(
                    args.timeout_ms
                        .unwrap_or(super::types::DEFAULT_WAIT_TIMEOUT_MS),
                )
                .await;
            let _ = context.event_tx.send(AgentEvent::CollabWaitingEnd {
                call_id: input.tool_id,
                completed_at: unix_seconds(),
                sender_path,
                timed_out: outcome.timed_out,
            });
            let message = if outcome.timed_out {
                "wait_agent timed out before new agent activity.".to_string()
            } else {
                "wait_agent observed agent activity.".to_string()
            };
            let recoverable_failures = recoverable_subagent_failures(&outcome.agents);
            let message = if recoverable_failures.is_empty() {
                message
            } else {
                recoverable_subagent_failures_message(recoverable_failures.len())
            };
            json_output(WaitAgentResult {
                message,
                timed_out: outcome.timed_out,
                agents: agent_tool_records(&outcome.agents, args.include_details),
                recoverable_failures,
            })
        })
    }
}

impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List known managed sub-agents in the current collaboration tree."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pathPrefix": {
                    "type": "string",
                    "description": "Optional canonical path prefix, such as /root/research."
                },
                "includeDetails": {
                    "type": "boolean",
                    "description": "Return full AgentRecord entries instead of compact summaries. Defaults to false."
                }
            }
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: ListAgentsArgs =
                serde_json::from_value(input.arguments).unwrap_or(ListAgentsArgs {
                    path_prefix: None,
                    include_details: false,
                });
            let agents = context
                .agent_control
                .list_agents(args.path_prefix.as_deref())
                .await;
            let recoverable_failures = recoverable_subagent_failures(&agents);
            json_output(ListAgentsResult {
                agents: agent_tool_records(&agents, args.include_details),
                recoverable_failures,
            })
        })
    }
}

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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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
            if let Some(message) = context.agent_control.take_trigger_message(&agent_id).await
                && let Some(record) = context.agent_control.record(&agent_id).await
            {
                let run = AgentRunConfig {
                    provider: self.provider.clone(),
                    reasoning_effort: self.reasoning_effort.clone(),
                    config: self.config.clone(),
                    mcp_runtime: self.mcp_runtime.clone(),
                    lsp_runtime: self.lsp_runtime.clone(),
                    workspace_instructions: context
                        .workspace_instructions
                        .clone()
                        .or_else(|| self.workspace_instructions.clone()),
                    instruction_snapshot: context.instruction_snapshot.clone(),
                    workspace_root: context.workspace_root.clone(),
                    options: child_agent_options(&context.options),
                    agent_control: context.agent_control.clone(),
                    event_tx: context.event_tx.clone(),
                    agent_id: agent_id.clone(),
                    agent_path: record.path,
                    role: record.role,
                    message,
                    mode: context.mode,
                    budget: TurnBudget::child_default(),
                };
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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
                status: previous.status,
                error: None,
            });
            json_output(MessageResult {
                target: previous.path,
                status: previous.status,
            })
        })
    }
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
