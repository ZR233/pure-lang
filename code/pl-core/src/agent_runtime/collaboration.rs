use std::sync::Arc;

use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::state::unix_timestamp;
use super::{
    AgentAccessPolicy, AgentActivityState, AgentId, AgentLifecycleState, AgentProgressStage,
    AgentRuntimeHandle, AgentSessionState, AgentSnapshot, AgentSpawnRequest, AgentTargetSelector,
    SessionId,
};
use crate::{AgentRoleId, Tool, ToolContext, ToolEffect, ToolInput, ToolOutput};

const TOOL_SPAWN_AGENT: &str = "spawn_agent";
const TOOL_REPORT_PROGRESS: &str = "report_progress";
const TOOL_SEND_MESSAGE: &str = "send_message";
const TOOL_INTERRUPT_AGENT: &str = "interrupt_agent";
const TOOL_LIST_AGENTS: &str = "list_agents";
const TOOL_WAIT_AGENTS: &str = "wait_agents";
const TOOL_READ_AGENT_SESSION: &str = "read_agent_session";
const TOOL_CLOSE_AGENT: &str = "close_agent";
const SESSION_READ_MIN_AGE_SECONDS: i64 = 300;

mod support;
use support::{
    agent_path, filter_visible, fork_session, json_output, object_schema, parse_agent_id,
    parse_input, progress_schema, send_message_schema, spawn_schema, target_schema, tool_error,
    wait_schema,
};

/// 为一次 turn 构造由 `AgentRuntimeHandle` 驱动的协作工具。
#[derive(Debug, Clone)]
pub struct AgentCollaborationTools {
    runtime: AgentRuntimeHandle,
    caller: AgentId,
    policy: AgentAccessPolicy,
}

impl AgentCollaborationTools {
    pub fn new(runtime: AgentRuntimeHandle, caller: AgentId, policy: AgentAccessPolicy) -> Self {
        Self {
            runtime,
            caller,
            policy,
        }
    }

    /// 返回可直接注册到 `AgentKernelBuilder` 的协作工具。
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools_matching(|_| true)
    }

    /// 返回由产品层接管 `send_message` 准入时使用的其余协作工具。
    pub fn tools_without_send_message(&self) -> Vec<Arc<dyn Tool>> {
        self.tools_matching(|kind| !matches!(kind, CollaborationToolKind::SendMessage))
    }

    fn tools_matching(
        &self,
        include: impl Fn(CollaborationToolKind) -> bool,
    ) -> Vec<Arc<dyn Tool>> {
        CollaborationToolKind::ALL
            .into_iter()
            .filter(|kind| include(*kind))
            .map(|kind| {
                Arc::new(CollaborationTool {
                    kind,
                    runtime: self.runtime.clone(),
                    caller: self.caller.clone(),
                    policy: self.policy.clone(),
                }) as Arc<dyn Tool>
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum CollaborationToolKind {
    Spawn,
    ReportProgress,
    SendMessage,
    Interrupt,
    List,
    Wait,
    ReadSession,
    Close,
}

impl CollaborationToolKind {
    const ALL: [Self; 8] = [
        Self::Spawn,
        Self::ReportProgress,
        Self::SendMessage,
        Self::Interrupt,
        Self::List,
        Self::Wait,
        Self::ReadSession,
        Self::Close,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Spawn => TOOL_SPAWN_AGENT,
            Self::ReportProgress => TOOL_REPORT_PROGRESS,
            Self::SendMessage => TOOL_SEND_MESSAGE,
            Self::Interrupt => TOOL_INTERRUPT_AGENT,
            Self::List => TOOL_LIST_AGENTS,
            Self::Wait => TOOL_WAIT_AGENTS,
            Self::ReadSession => TOOL_READ_AGENT_SESSION,
            Self::Close => TOOL_CLOSE_AGENT,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Spawn => "Spawn a child agent using one of the roles allowed for this turn.",
            Self::ReportProgress => {
                "Record the caller's current execution stage, concise summary, and next step."
            }
            Self::SendMessage => {
                "Send a message to an accessible agent without interrupting its active turn."
            }
            Self::Interrupt => "Interrupt an accessible agent's current turn.",
            Self::List => "List compact canonical snapshots for visible agents.",
            Self::Wait => {
                "Wait until a target reports progress, requests interaction, or finishes a turn."
            }
            Self::ReadSession => {
                "Read a bounded filtered digest for a terminal or potentially stuck agent."
            }
            Self::Close => "Close an accessible child agent and its product resources.",
        }
    }
}

#[derive(Debug, Clone)]
struct CollaborationTool {
    kind: CollaborationToolKind,
    runtime: AgentRuntimeHandle,
    caller: AgentId,
    policy: AgentAccessPolicy,
}

impl Tool for CollaborationTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        match self.kind {
            CollaborationToolKind::Spawn => spawn_schema(&self.policy),
            CollaborationToolKind::ReportProgress => progress_schema(),
            CollaborationToolKind::SendMessage => send_message_schema(&self.policy.message_targets),
            CollaborationToolKind::Interrupt => target_schema(
                &self.policy.message_targets,
                "Agent id whose current turn should be interrupted.",
            ),
            CollaborationToolKind::List => object_schema(Vec::new()),
            CollaborationToolKind::Wait => wait_schema(&self.policy.list_targets),
            CollaborationToolKind::ReadSession => target_schema(
                &self.policy.list_targets,
                "Agent id whose bounded session digest should be read.",
            ),
            CollaborationToolKind::Close => {
                target_schema(&self.policy.close_targets, "Agent id to close.")
            }
        }
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        matches!(
            self.kind,
            CollaborationToolKind::SendMessage
                | CollaborationToolKind::Interrupt
                | CollaborationToolKind::List
                | CollaborationToolKind::ReadSession
        )
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::AgentControl)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutput, PureError>> + Send + 'a>,
    > {
        Box::pin(async move {
            match self.kind {
                CollaborationToolKind::Spawn => self.spawn(input, context).await,
                CollaborationToolKind::ReportProgress => self.report_progress(input).await,
                CollaborationToolKind::SendMessage => self.send_message(input).await,
                CollaborationToolKind::Interrupt => self.interrupt(input).await,
                CollaborationToolKind::List => self.list(input).await,
                CollaborationToolKind::Wait => self.wait(input, context).await,
                CollaborationToolKind::ReadSession => self.read_session(input).await,
                CollaborationToolKind::Close => self.close(input).await,
            }
        })
    }
}

impl CollaborationTool {
    async fn spawn(&self, input: ToolInput, context: ToolContext) -> Result<ToolOutput, PureError> {
        let args: SpawnArgs = parse_input(TOOL_SPAWN_AGENT, input.arguments)?;
        let role = AgentRoleId::new(args.role)
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        if !self.policy.spawn_roles.contains(&role) {
            return Err(tool_error(
                TOOL_SPAWN_AGENT,
                format!("role `{role}` is not allowed for this turn"),
            ));
        }
        let session_id = SessionId::generate();
        let session = AgentSessionState {
            id: session_id.clone(),
            metadata: serde_json::Value::Null,
            session: fork_session(&context.parent_session, args.fork_turns)?,
            usage: pl_model::TokenUsage::default(),
            last_context_tokens: None,
            trace_sequence: 0,
            session_event_sequence: 0,
        };
        let mut metadata = match args.metadata {
            Value::Object(metadata) => metadata,
            Value::Null => serde_json::Map::new(),
            _ => {
                return Err(tool_error(
                    TOOL_SPAWN_AGENT,
                    "metadata must be an object".to_string(),
                ));
            }
        };
        metadata.insert(
            "requestingToolCallId".to_string(),
            Value::String(input.tool_id),
        );
        metadata.insert(
            "workspaceRoot".to_string(),
            Value::String(context.workspace_root.to_string_lossy().to_string()),
        );
        let result = self
            .runtime
            .spawn(AgentSpawnRequest {
                parent_id: self.caller.clone(),
                role,
                session,
                initial_message: Some(args.message),
                metadata: Value::Object(metadata),
            })
            .await
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        json_output(json!({
            "agentId": result.snapshot.identity.id,
            "sessionId": session_id,
            "turnId": result.initial_turn_id,
        }))
    }

    async fn report_progress(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: ProgressArgs = parse_input(TOOL_REPORT_PROGRESS, input.arguments)?;
        let checkpoint = self
            .runtime
            .report_progress(
                self.caller.clone(),
                args.stage.into(),
                args.summary,
                args.next_step,
            )
            .await
            .map_err(|error| tool_error(TOOL_REPORT_PROGRESS, error.to_string()))?;
        json_output(json!(checkpoint))
    }

    async fn send_message(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: SendMessageArgs = parse_input(TOOL_SEND_MESSAGE, input.arguments)?;
        let target = parse_agent_id(TOOL_SEND_MESSAGE, args.target)?;
        self.authorize(&self.policy.message_targets, &target)
            .await?;
        let turn_id = self
            .runtime
            .submit_current_session(
                target.clone(),
                super::AgentCurrentSessionSubmitRequest::start(args.message)
                    .with_presentation(super::MailboxPresentation::Hidden),
            )
            .await
            .map_err(|error| tool_error(TOOL_SEND_MESSAGE, error.to_string()))?;
        json_output(json!({ "target": target, "turnId": turn_id }))
    }

    async fn interrupt(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: TargetArgs = parse_input(TOOL_INTERRUPT_AGENT, input.arguments)?;
        let target = parse_agent_id(TOOL_INTERRUPT_AGENT, args.target)?;
        if target == self.caller {
            return Err(tool_error(
                TOOL_INTERRUPT_AGENT,
                "an agent cannot interrupt itself".to_string(),
            ));
        }
        self.authorize(&self.policy.message_targets, &target)
            .await?;
        let snapshot = self
            .runtime
            .snapshot(target.clone())
            .await
            .map_err(|error| tool_error(TOOL_INTERRUPT_AGENT, error.to_string()))?;
        let turn_id = snapshot.active_turn_id.clone().ok_or_else(|| {
            tool_error(
                TOOL_INTERRUPT_AGENT,
                format!("agent `{target}` has no active turn"),
            )
        })?;
        self.runtime
            .cancel_turn(target.clone(), turn_id)
            .await
            .map_err(|error| tool_error(TOOL_INTERRUPT_AGENT, error.to_string()))?;
        json_output(json!({
            "target": target,
            "previousStatus": {
                "lifecycle": snapshot.lifecycle,
                "activity": snapshot.activity,
                "lastTurnOutcome": snapshot.last_turn,
            }
        }))
    }

    async fn list(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let _: EmptyArgs = parse_input(TOOL_LIST_AGENTS, input.arguments)?;
        let snapshots = self
            .runtime
            .list()
            .await
            .map_err(|error| tool_error(TOOL_LIST_AGENTS, error.to_string()))?;
        let visible = filter_visible(&snapshots, &self.caller, &self.policy.list_targets);
        let agents = visible
            .iter()
            .map(|snapshot| compact_agent(snapshot, &snapshots))
            .collect::<Vec<_>>();
        json_output(json!({ "agents": agents }))
    }

    async fn wait(&self, input: ToolInput, context: ToolContext) -> Result<ToolOutput, PureError> {
        let args: WaitArgs = parse_input(TOOL_WAIT_AGENTS, input.arguments)?;
        let snapshots = self
            .runtime
            .list()
            .await
            .map_err(|error| tool_error(TOOL_WAIT_AGENTS, error.to_string()))?;
        let visible = filter_visible(&snapshots, &self.caller, &self.policy.list_targets);
        let targets = match args.targets {
            Some(targets) => {
                let targets = targets
                    .into_iter()
                    .map(|target| parse_agent_id(TOOL_WAIT_AGENTS, target))
                    .collect::<Result<Vec<_>, _>>()?;
                for target in &targets {
                    if !visible
                        .iter()
                        .any(|snapshot| &snapshot.identity.id == target)
                    {
                        return Err(tool_error(
                            TOOL_WAIT_AGENTS,
                            format!("agent `{target}` is not accessible for this turn"),
                        ));
                    }
                }
                targets
            }
            None => visible
                .iter()
                .filter(|snapshot| snapshot.identity.parent_id.as_ref() == Some(&self.caller))
                .map(|snapshot| snapshot.identity.id.clone())
                .collect(),
        };
        if targets.is_empty() {
            return Err(tool_error(
                TOOL_WAIT_AGENTS,
                "no visible target agents to wait for".to_string(),
            ));
        }

        let wait = self.runtime.wait_agents(targets);
        let result = match context.options.cancellation_token.clone() {
            Some(token) => {
                tokio::select! {
                    result = wait => result,
                    _ = token.cancelled() => {
                        return Err(tool_error(
                            TOOL_WAIT_AGENTS,
                            "wait cancelled with the current turn".to_string(),
                        ));
                    }
                }
            }
            None => wait.await,
        }
        .map_err(|error| tool_error(TOOL_WAIT_AGENTS, error.to_string()))?;
        let agents = result
            .agents
            .iter()
            .map(|snapshot| compact_agent(snapshot, &snapshots))
            .collect::<Vec<_>>();
        json_output(json!({ "reason": result.reason, "agents": agents }))
    }

    async fn read_session(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: TargetArgs = parse_input(TOOL_READ_AGENT_SESSION, input.arguments)?;
        let target = parse_agent_id(TOOL_READ_AGENT_SESSION, args.target)?;
        self.authorize(&self.policy.list_targets, &target).await?;
        let snapshot = self
            .runtime
            .snapshot(target.clone())
            .await
            .map_err(|error| tool_error(TOOL_READ_AGENT_SESSION, error.to_string()))?;
        let age = summary_age_seconds(&snapshot);
        if session_read_requires_age_gate(snapshot.lifecycle, snapshot.activity)
            && age < SESSION_READ_MIN_AGE_SECONDS
        {
            return Err(tool_error(
                TOOL_READ_AGENT_SESSION,
                format!(
                    "agent `{target}` has active work and its latest summary is {age}s old; reading is available at {SESSION_READ_MIN_AGE_SECONDS}s"
                ),
            ));
        }
        let digest = self
            .runtime
            .read_agent_session(target)
            .await
            .map_err(|error| tool_error(TOOL_READ_AGENT_SESSION, error.to_string()))?;
        json_output(json!(digest))
    }

    async fn close(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: TargetArgs = parse_input(TOOL_CLOSE_AGENT, input.arguments)?;
        let target = parse_agent_id(TOOL_CLOSE_AGENT, args.target)?;
        if target == self.caller {
            return Err(tool_error(
                TOOL_CLOSE_AGENT,
                "an agent cannot close itself".to_string(),
            ));
        }
        self.authorize(&self.policy.close_targets, &target).await?;
        let snapshot = self
            .runtime
            .close(target)
            .await
            .map_err(|error| tool_error(TOOL_CLOSE_AGENT, error.to_string()))?;
        json_output(json!({ "snapshot": snapshot }))
    }

    async fn authorize(
        &self,
        selector: &AgentTargetSelector,
        target: &AgentId,
    ) -> Result<(), PureError> {
        let snapshots = self
            .runtime
            .list()
            .await
            .map_err(|error| tool_error(self.kind.name(), error.to_string()))?;
        let allowed = filter_visible(&snapshots, &self.caller, selector)
            .iter()
            .any(|snapshot| &snapshot.identity.id == target);
        if allowed {
            Ok(())
        } else {
            Err(tool_error(
                self.kind.name(),
                format!("agent `{target}` is not accessible for this turn"),
            ))
        }
    }
}

fn compact_agent(snapshot: &AgentSnapshot, all: &[AgentSnapshot]) -> Value {
    json!({
        "identity": snapshot.identity.id,
        "path": agent_path(&snapshot.identity.id, all),
        "role": snapshot.identity.role,
        "lifecycle": snapshot.lifecycle,
        "activity": snapshot.activity,
        "lastTurnOutcome": snapshot.last_turn,
        "progress": snapshot.progress,
        "updatedAt": snapshot.updated_at,
        "summaryAgeSeconds": summary_age_seconds(snapshot),
    })
}

fn summary_age_seconds(snapshot: &AgentSnapshot) -> i64 {
    unix_timestamp()
        .saturating_sub(
            snapshot
                .progress
                .as_ref()
                .map_or(snapshot.updated_at, |progress| progress.updated_at),
        )
        .max(0)
}

fn session_read_requires_age_gate(
    lifecycle: AgentLifecycleState,
    activity: AgentActivityState,
) -> bool {
    lifecycle == AgentLifecycleState::Active && activity != AgentActivityState::Idle
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpawnArgs {
    message: String,
    role: String,
    #[serde(default)]
    fork_turns: ForkTurns,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgressArgs {
    stage: ProgressStage,
    summary: String,
    next_step: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForReview,
}

impl From<ProgressStage> for AgentProgressStage {
    fn from(value: ProgressStage) -> Self {
        match value {
            ProgressStage::Exploring => Self::Exploring,
            ProgressStage::Implementing => Self::Implementing,
            ProgressStage::Verifying => Self::Verifying,
            ProgressStage::Blocked => Self::Blocked,
            ProgressStage::ReadyForReview => Self::ReadyForReview,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendMessageArgs {
    target: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetArgs {
    target: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitArgs {
    targets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ForkTurns {
    #[default]
    None,
    All,
    Last(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_session_age_gate_only_applies_while_agent_has_active_work() {
        assert!(session_read_requires_age_gate(
            AgentLifecycleState::Active,
            AgentActivityState::Running,
        ));
        assert!(session_read_requires_age_gate(
            AgentLifecycleState::Active,
            AgentActivityState::WaitingTool,
        ));
        assert!(!session_read_requires_age_gate(
            AgentLifecycleState::Active,
            AgentActivityState::Idle,
        ));
        assert!(!session_read_requires_age_gate(
            AgentLifecycleState::Closed,
            AgentActivityState::Idle,
        ));
    }
}
