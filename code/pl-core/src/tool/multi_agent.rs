use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::{AgentEvent, AgentStatus, PureError};
use serde::{Deserialize, Serialize};

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{SubagentContext, Tool, ToolContext, ToolInput, ToolOutput};
use crate::agent::{AgentRecord, AgentSpawnInput, MessageDeliveryMode};
use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::core::compact_text;
use crate::session::CoreSession;
use crate::turn::{CompileMode, TurnResultStatus};
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
    timed_out: bool,
    agents: Vec<AgentRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListAgentsResult {
    agents: Vec<AgentRecord>,
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
            let handle = context
                .agent_control
                .spawn_agent(AgentSpawnInput {
                    task_name: args.task_name.clone(),
                    message: args.message.clone(),
                    role: role.clone(),
                    parent_path: Some(parent_path),
                })
                .await?;
            let record = context
                .agent_control
                .record(&handle.id)
                .await
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "spawn_agent".to_string(),
                    error: "spawned agent disappeared".to_string(),
                })?;
            emit_agent_record(&context.event_tx, &record);

            let run = AgentRunConfig {
                provider: self.provider.clone(),
                reasoning_effort: self.reasoning_effort.clone(),
                config: self.config.clone(),
                workspace_instructions: context
                    .workspace_instructions
                    .clone()
                    .or_else(|| self.workspace_instructions.clone()),
                workspace_root: context.workspace_root.clone(),
                options: context.options.clone(),
                agent_control: context.agent_control.clone(),
                event_tx: context.event_tx.clone(),
                agent_id: handle.id.clone(),
                agent_path: handle.path.clone(),
                role,
                message: args.message,
                max_tool_iterations: None,
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

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: WaitAgentArgs = serde_json::from_value(input.arguments)
                .unwrap_or(WaitAgentArgs { timeout_ms: None });
            let outcome = context
                .agent_control
                .wait_for_activity(args.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS))
                .await;
            json_output(WaitAgentResult {
                timed_out: outcome.timed_out,
                agents: outcome.agents,
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
            json_output(ListAgentsResult { agents })
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
                    options: context.options.clone(),
                    agent_control: context.agent_control.clone(),
                    event_tx: context.event_tx.clone(),
                    agent_id,
                    agent_path: record.path,
                    role: record.role,
                    message,
                    max_tool_iterations: None,
                };
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
            let record = context
                .agent_control
                .close_agent(&current_agent_path(&context), &args.target)
                .await?;
            emit_agent_record(&context.event_tx, &record);
            json_output(MessageResult {
                target: record.path,
                status: record.status,
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
    let record = context
        .agent_control
        .append_message(
            &current_agent_path(&context),
            &args.target,
            args.message,
            mode,
        )
        .await?;
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
    pub max_tool_iterations: Option<usize>,
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
    let mut request = crate::turn::TurnRequest::new(config.message.clone(), CompileMode::Auto);
    if let Some(max) = config.max_tool_iterations {
        request = request.with_max_tool_iterations(max);
    }
    if let Some(instructions) = config.workspace_instructions.clone() {
        request = request.with_workspace_instructions(instructions);
    }
    let result = core
        .run_turn_with_options(
            &mut session,
            request,
            config.event_tx.clone(),
            config.options.clone(),
        )
        .await;
    config
        .agent_control
        .store_session(&config.agent_id, session)
        .await;

    match result {
        Ok(result) => {
            let status = match result.status {
                TurnResultStatus::Completed => AgentStatus::Completed,
                TurnResultStatus::Interrupted => AgentStatus::Interrupted,
                TurnResultStatus::Failed => AgentStatus::Failed,
            };
            let summary = result.content.trim().to_string();
            if let Some(record) = config
                .agent_control
                .update_status(
                    &config.agent_id,
                    status,
                    (!summary.is_empty()).then_some(summary.clone()),
                    None,
                )
                .await
            {
                emit_agent_record(&config.event_tx, &record);
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
        .update_status(&config.agent_id, AgentStatus::Failed, None, Some(error))
        .await
    {
        emit_agent_record(&config.event_tx, &record);
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
