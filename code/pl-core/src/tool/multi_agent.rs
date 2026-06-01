use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::{AgentEvent, AgentStatus, PureError};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::recoverable::{
    RecoverableSubagentFailure, is_recoverable_subagent_capacity_error,
    recoverable_subagent_failures, recoverable_subagent_failures_message,
    recoverable_subagent_tool_output,
};
use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{SubagentContext, Tool, ToolContext, ToolInput, ToolOutput};
use crate::agent::{AgentRecord, AgentSpawnInput, AgentStatusUpdate, MessageDeliveryMode};
use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::core::compact_text;
use crate::session::CoreSession;
use crate::turn::{CompileMode, TurnAbortReason, TurnBudget, TurnResultStatus};
use crate::{AgentControl, AgentPath, PureCore};

const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone)]
pub struct SpawnAgentTool {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_instructions: Option<String>,
}

impl SpawnAgentTool {
    pub fn new(
        provider: SharedModelProvider,
        reasoning_effort: Option<ReasoningEffort>,
        config: Option<PureConfig>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            workspace_instructions,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WaitAgentTool;

#[derive(Debug, Clone)]
pub struct ListAgentsTool;

#[derive(Debug, Clone)]
pub struct SendMessageTool;

#[derive(Debug, Clone)]
pub struct FollowupTaskTool {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_instructions: Option<String>,
}

impl FollowupTaskTool {
    pub fn new(
        provider: SharedModelProvider,
        reasoning_effort: Option<ReasoningEffort>,
        config: Option<PureConfig>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            workspace_instructions,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloseAgentTool;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct SpawnAgentArgs {
    task_name: String,
    message: String,
    agent_type: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    fork_turns: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitAgentArgs {
    timeout_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAgentsArgs {
    path_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentMessageArgs {
    target: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseAgentArgs {
    target: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpawnAgentResult {
    agent_id: String,
    task_name: String,
    path: String,
    status: AgentStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WaitAgentResult {
    message: String,
    timed_out: bool,
    recoverable_failures: Vec<RecoverableSubagentFailure>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListAgentsResult {
    agents: Vec<AgentRecord>,
    recoverable_failures: Vec<RecoverableSubagentFailure>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageResult {
    target: String,
    status: AgentStatus,
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
                    "description": "Reserved history fork mode; current implementation starts with the provided message."
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
            let role = role_key(args.agent_type.as_deref())?;
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
                workspace_instructions: context
                    .workspace_instructions
                    .clone()
                    .or_else(|| self.workspace_instructions.clone()),
                workspace_root: context.workspace_root.clone(),
                options,
                agent_control: context.agent_control.clone(),
                event_tx: context.event_tx.clone(),
                agent_id: handle.id.clone(),
                agent_path: handle.path.clone(),
                role,
                message: prompt,
                budget: crate::turn::TurnBudget::child_default(),
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
            let args: WaitAgentArgs = serde_json::from_value(input.arguments)
                .unwrap_or(WaitAgentArgs { timeout_ms: None });
            let sender_path = current_agent_path(&context);
            let _ = context.event_tx.send(AgentEvent::CollabWaitingBegin {
                call_id: input.tool_id.clone(),
                started_at: unix_seconds(),
                sender_path: sender_path.clone(),
            });
            let outcome = context
                .agent_control
                .wait_for_activity(args.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS))
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
            let args: ListAgentsArgs = serde_json::from_value(input.arguments)
                .unwrap_or(ListAgentsArgs { path_prefix: None });
            let agents = context
                .agent_control
                .list_agents(args.path_prefix.as_deref())
                .await;
            let recoverable_failures = recoverable_subagent_failures(&agents);
            json_output(ListAgentsResult {
                agents,
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
                    workspace_instructions: context
                        .workspace_instructions
                        .clone()
                        .or_else(|| self.workspace_instructions.clone()),
                    workspace_root: context.workspace_root.clone(),
                    options: child_agent_options(&context.options),
                    agent_control: context.agent_control.clone(),
                    event_tx: context.event_tx.clone(),
                    agent_id: agent_id.clone(),
                    agent_path: record.path,
                    role: record.role,
                    message,
                    budget: crate::turn::TurnBudget::child_default(),
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

fn message_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Agent id, relative path, or canonical path."
            },
            "message": {
                "type": "string",
                "description": "Message to send to the target agent."
            }
        },
        "required": ["target", "message"]
    })
}

pub(super) struct AgentRunConfig {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub workspace_instructions: Option<String>,
    pub workspace_root: PathBuf,
    pub options: crate::TurnOptions,
    pub agent_control: AgentControl,
    pub event_tx: pl_protocol::AgentEventSender,
    pub agent_id: String,
    pub agent_path: String,
    pub role: String,
    pub message: String,
    pub budget: TurnBudget,
}

pub(super) async fn run_agent_turn(config: AgentRunConfig) {
    let Some(record) = config
        .agent_control
        .update_status(&config.agent_id, AgentStatus::Running, None, None)
        .await
    else {
        return;
    };
    emit_agent_record(&config.event_tx, &record);

    let role = ModelRole::from_key(&config.role).unwrap_or(ModelRole::Executor);
    let core_result = match &config.config {
        Some(pure_config) => PureCore::from_config(pure_config, role),
        None => Ok(match &config.reasoning_effort {
            Some(effort) => {
                PureCore::with_reasoning_effort(config.provider.clone(), effort.clone())
            }
            None => PureCore::new(config.provider.clone()),
        }),
    };
    let mut core = match core_result {
        Ok(core) => core
            .with_agent_control(config.agent_control.clone())
            .with_subagent_context(SubagentContext {
                id: config.agent_id.clone(),
                parent_id: record.parent_path.clone(),
                agent_path: Some(config.agent_path.clone()),
                role: config.role.clone(),
                task: compact_text(&config.message),
                depth: record.depth,
            }),
        Err(error) => {
            mark_agent_failed(&config, error.to_string()).await;
            return;
        }
    };
    core.register_default_tools(
        config.workspace_root.clone(),
        config.workspace_instructions.clone(),
    );

    let mut session = config
        .agent_control
        .load_session(&config.agent_id)
        .await
        .unwrap_or_else(CoreSession::new);
    let mut request = crate::turn::TurnRequest::new(config.message.clone(), CompileMode::Auto)
        .with_budget(config.budget);
    if let Some(instructions) = config.workspace_instructions.clone() {
        request = request.with_workspace_instructions(instructions);
    }
    let (agent_event_tx, agent_event_rx) = tokio::sync::broadcast::channel(256);
    let forward_task = tokio::spawn(forward_agent_lifecycle_events(
        agent_event_rx,
        config.event_tx.clone(),
    ));
    let result = core
        .run_turn_with_options(
            &mut session,
            request,
            agent_event_tx.clone(),
            config.options.clone(),
        )
        .await;
    drop(agent_event_tx);
    let _ = forward_task.await;
    config
        .agent_control
        .store_session(&config.agent_id, session)
        .await;

    match result {
        Ok(result) => {
            let status = match result.status {
                TurnResultStatus::Completed => AgentStatus::Completed,
                TurnResultStatus::Aborted => AgentStatus::Interrupted,
                TurnResultStatus::Errored => AgentStatus::Errored,
            };
            let summary = result.content.trim().to_string();
            let reason = result
                .abort_reason
                .map(|reason| reason.as_str().to_string());
            let error = match result.status {
                TurnResultStatus::Aborted
                    if matches!(result.abort_reason, Some(TurnAbortReason::BudgetLimited)) =>
                {
                    result
                        .error
                        .clone()
                        .or_else(|| Some("subagent budget limited".to_string()))
                }
                TurnResultStatus::Errored => result
                    .error
                    .clone()
                    .or_else(|| Some("subagent errored".to_string())),
                TurnResultStatus::Completed | TurnResultStatus::Aborted => result.error.clone(),
            };
            if let Some(record) = config
                .agent_control
                .update_status_with(
                    &config.agent_id,
                    AgentStatusUpdate {
                        status,
                        summary: (!summary.is_empty()).then_some(summary.clone()),
                        error,
                        reason,
                        budget_limit_kind: result.budget_limit_kind,
                        budget_usage: result.budget_usage,
                    },
                )
                .await
            {
                emit_agent_record(&config.event_tx, &record);
            }
            if !matches!(result.status, TurnResultStatus::Completed) {
                let reason = result
                    .abort_reason
                    .map(|reason| reason.as_str())
                    .unwrap_or("errored");
                for record in config
                    .agent_control
                    .shutdown_descendants(&config.agent_id, reason)
                    .await
                {
                    emit_agent_record(&config.event_tx, &record);
                }
            }
        }
        Err(error) => {
            mark_agent_failed(&config, error.to_string()).await;
        }
    }
}

async fn mark_agent_failed(config: &AgentRunConfig, error: String) {
    if let Some(record) = config
        .agent_control
        .update_status_with(
            &config.agent_id,
            AgentStatusUpdate {
                status: AgentStatus::Errored,
                summary: None,
                error: Some(error),
                reason: Some("errored".to_string()),
                budget_limit_kind: None,
                budget_usage: None,
            },
        )
        .await
    {
        emit_agent_record(&config.event_tx, &record);
    }
    for record in config
        .agent_control
        .shutdown_descendants(&config.agent_id, "errored")
        .await
    {
        emit_agent_record(&config.event_tx, &record);
    }
}

async fn forward_agent_lifecycle_events(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    parent_event_tx: pl_protocol::AgentEventSender,
) {
    loop {
        match event_rx.recv().await {
            Ok(
                event @ (AgentEvent::AgentStateChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::CollabAgentSpawnBegin { .. }
                | AgentEvent::CollabAgentSpawnEnd { .. }
                | AgentEvent::CollabAgentInteractionBegin { .. }
                | AgentEvent::CollabAgentInteractionEnd { .. }
                | AgentEvent::CollabWaitingBegin { .. }
                | AgentEvent::CollabWaitingEnd { .. }
                | AgentEvent::CollabCloseBegin { .. }
                | AgentEvent::CollabCloseEnd { .. }),
            ) => {
                let _ = parent_event_tx.send(event);
            }
            Ok(AgentEvent::Done) => break,
            Ok(
                AgentEvent::TimelineItemStarted { .. }
                | AgentEvent::TimelineItemDelta { .. }
                | AgentEvent::TimelineItemCompleted { .. }
                | AgentEvent::TimelineItemFailed { .. }
                | AgentEvent::ToolApprovalRequested { .. }
                | AgentEvent::ToolApprovalGranted { .. }
                | AgentEvent::ToolApprovalDenied { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. },
            ) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn role_key(role: Option<&str>) -> Result<String, PureError> {
    let role = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("executor");
    ModelRole::from_key(role)
        .map(|role| role.key().to_string())
        .ok_or_else(|| PureError::ToolExecutionFailed {
            tool: "spawn_agent".to_string(),
            error: format!("unsupported agentType: {role}"),
        })
}

pub(super) fn current_agent_path(context: &ToolContext) -> String {
    context
        .active_subagent
        .as_ref()
        .and_then(|subagent| subagent.agent_path.clone())
        .unwrap_or_else(|| AgentPath::ROOT.to_string())
}

pub(super) fn emit_agent_record(event_tx: &pl_protocol::AgentEventSender, record: &AgentRecord) {
    let _ = event_tx.send(AgentEvent::AgentStateChanged {
        id: record.id.clone(),
        path: record.path.clone(),
        parent_path: record.parent_path.clone(),
        role: record.role.clone(),
        task: record.task.clone(),
        status: record.status,
        summary: record.summary.clone(),
        depth: record.depth,
        error: record.error.clone(),
        reason: record.reason.clone(),
        budget_limit_kind: record.budget_limit_kind,
        budget_usage: record.budget_usage,
        updated_at: record.updated_at,
    });
}

fn invalid_spawn_input(error: serde_json::Error) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: format!("invalid input: {error}"),
    }
}

fn json_output(value: impl Serialize) -> Result<ToolOutput, PureError> {
    let description =
        serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
            tool: "agent".to_string(),
            error: format!("failed to serialize output: {error}"),
        })?;
    Ok(ToolOutput {
        description,
        truncated: OutputTruncation {
            stdout: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
            stderr: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
        },
        output_file: PathBuf::new(),
        exit_code: None,
        timed_out: false,
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn child_agent_options(options: &crate::TurnOptions) -> crate::TurnOptions {
    let token = options
        .cancellation_token
        .as_ref()
        .map(CancellationToken::child_token)
        .unwrap_or_default();
    options.clone().with_cancellation(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn agent_record(id: &str, status: AgentStatus, error: Option<&str>) -> AgentRecord {
        AgentRecord {
            id: id.to_string(),
            path: format!("/root/{id}"),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: format!("inspect {id}"),
            status,
            summary: None,
            error: error.map(str::to_string),
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            depth: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn wait_agent_result_serializes_recoverable_failures() {
        let agents = vec![
            agent_record(
                "agent-1",
                AgentStatus::Errored,
                Some("API error 429 Too Many Requests"),
            ),
            agent_record("agent-2", AgentStatus::Completed, None),
        ];
        let recoverable_failures = recoverable_subagent_failures(&agents);
        let output = json_output(WaitAgentResult {
            message: recoverable_subagent_failures_message(recoverable_failures.len()),
            timed_out: false,
            recoverable_failures,
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

        assert_eq!(value["timedOut"], false);
        assert_eq!(
            value["message"],
            "recoverableSubagentProvider429: 1 subagent(s) are unavailable because the provider returned 429 concurrency/rate-limit capacity. Stop creating or retrying subagents and continue the remaining work in the current agent."
        );
        assert_eq!(value["recoverableFailures"][0]["agentId"], "agent-1");
        assert_eq!(value["recoverableFailures"][0]["path"], "/root/agent-1");
    }

    #[test]
    fn list_agents_result_keeps_agents_and_recoverable_failures() {
        let agents = vec![
            agent_record(
                "agent-1",
                AgentStatus::Interrupted,
                Some("provider returned status 429"),
            ),
            agent_record("agent-2", AgentStatus::Completed, None),
        ];
        let recoverable_failures = recoverable_subagent_failures(&agents);
        let output = json_output(ListAgentsResult {
            agents,
            recoverable_failures,
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

        assert_eq!(value["agents"].as_array().unwrap().len(), 2);
        assert_eq!(value["recoverableFailures"][0]["agentId"], "agent-1");
        assert_eq!(
            value["recoverableFailures"][0]["error"],
            "provider returned status 429"
        );
    }
}
