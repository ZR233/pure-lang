use std::sync::Arc;

use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::state::unix_timestamp;
use super::*;
use crate::tool::ToolBudgetTiming;
use crate::{AgentRoleId, Tool, ToolContext, ToolEffect, ToolInput, ToolOutput};

const TOOL_SPAWN_AGENT: &str = "spawn_agent";
const TOOL_REPORT_PROGRESS: &str = "report_progress";
const TOOL_SEND_MESSAGE: &str = "send_message";
const TOOL_INTERRUPT_AGENT: &str = "interrupt_agent";
const TOOL_LIST_AGENTS: &str = "list_agents";
const TOOL_WAIT_AGENTS: &str = "wait_agents";
const TOOL_READ_AGENT_SESSION: &str = "read_agent_session";
const TOOL_READ_AGENT_SUBMISSIONS: &str = "read_agent_submissions";
const TOOL_CLOSE_AGENT: &str = "close_agent";
const SESSION_READ_MIN_AGE_SECONDS: i64 = 300;
const DEFAULT_SUBMISSION_OFFSET: usize = 0;
const DEFAULT_SUBMISSION_LIMIT: usize = 20;
const MAX_SUBMISSION_LIMIT: usize = 50;
/// read_agent_submissions 单页硬字节上限：覆盖默认 12KB 安全阈值，保证 detail 全文返回。
const MAX_SUBMISSION_OUTPUT_BYTES: usize = 64 * 1024;

mod support;
use support::{
    agent_path, filter_visible, fork_session, json_output, json_output_with_budget, object_schema,
    parse_agent_id, parse_input, progress_schema, send_message_schema, spawn_schema,
    submissions_schema, target_schema, tool_error, wait_schema,
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

    /// 返回可直接注册到 `TurnEngine` 的协作工具。
    ///
    /// 所有 agent（含 Task planner）共享同一套基础能力：send_message 仅允许
    /// parent→direct-child 调度，子代理向主代理的报告改由 durable 阶段提交
    /// 与 read_agent_submissions 查询承载。
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        CollaborationToolKind::ALL
            .into_iter()
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
    ReadSubmissions,
    Close,
}

impl CollaborationToolKind {
    const ALL: [Self; 9] = [
        Self::Spawn,
        Self::ReportProgress,
        Self::SendMessage,
        Self::Interrupt,
        Self::List,
        Self::Wait,
        Self::ReadSession,
        Self::ReadSubmissions,
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
            Self::ReadSubmissions => TOOL_READ_AGENT_SUBMISSIONS,
            Self::Close => TOOL_CLOSE_AGENT,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Spawn => "Spawn a child agent using one of the roles allowed for this turn.",
            Self::ReportProgress => {
                "Record the caller's current execution stage, concise summary, next step, and optional detailed report. Appends a durable submission the orchestrator can read later; never creates a completion or review authorization."
            }
            Self::SendMessage => {
                "Insert a steering message into a direct child agent's session without interrupting its active turn. Only parent-to-direct-child is allowed."
            }
            Self::Interrupt => "Interrupt an accessible agent's current turn.",
            Self::List => {
                "List full compact canonical snapshots for visible agents when discovering targets, reconciling after restart, or diagnosing stalled work."
            }
            Self::Wait => {
                "Wait until a target reports progress, requests interaction, or finishes a turn, then return only the latest changed agent messages. Consume this delta directly instead of calling list_agents to refresh."
            }
            Self::ReadSession => {
                "Read a bounded filtered digest for a terminal or potentially stuck agent."
            }
            Self::ReadSubmissions => {
                "Read the durable stage submission history for an agent (full content, paginated, not truncated; works after the target has closed)."
            }
            Self::Close => "Close an accessible child agent and its product resources.",
        }
    }

    fn budget_timing(self) -> ToolBudgetTiming {
        match self {
            Self::Wait => ToolBudgetTiming::PauseWhenOnlyScheduledTool,
            Self::Spawn
            | Self::ReportProgress
            | Self::SendMessage
            | Self::Interrupt
            | Self::List
            | Self::ReadSession
            | Self::ReadSubmissions
            | Self::Close => ToolBudgetTiming::Count,
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
            CollaborationToolKind::SendMessage => send_message_schema(),
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
            CollaborationToolKind::ReadSubmissions => submissions_schema(&self.policy.list_targets),
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
                | CollaborationToolKind::ReadSubmissions
        )
    }

    fn budget_timing(&self) -> ToolBudgetTiming {
        self.kind.budget_timing()
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
                CollaborationToolKind::ReadSubmissions => self.read_submissions(input).await,
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
        let thread_id = ThreadId::generate();
        let session = ThreadContextState {
            metadata: serde_json::Value::Null,
            session: fork_session(&context.parent_session, args.fork_turns)?,
            usage: pl_model::TokenUsage::default(),
            billing_by_turn: std::collections::BTreeMap::new(),
            last_context_tokens: None,
            trace_sequence: 0,
            thread_revision: 0,
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
            Value::String(context.workspace.root().to_string_lossy().to_string()),
        );
        let result = self
            .runtime
            .spawn(AgentSpawnRequest {
                thread_id: thread_id.clone(),
                parent_id: self.caller.clone(),
                role,
                session,
                initial_turn_id: None,
                initial_message: Some(args.message),
                metadata: Value::Object(metadata),
            })
            .await
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        json_output(json!({
            "agentId": result.snapshot.identity.id,
            "threadId": thread_id,
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
                args.detail,
            )
            .await
            .map_err(|error| tool_error(TOOL_REPORT_PROGRESS, error.to_string()))?;
        json_output(json!(checkpoint))
    }

    async fn send_message(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: SendMessageArgs = parse_input(TOOL_SEND_MESSAGE, input.arguments)?;
        let target = parse_agent_id(TOOL_SEND_MESSAGE, args.target)?;
        // 单一消息插入原语：仅允许父代理向其直接子代理插入调度消息。
        // 子代理不得向父代理或 peer push；子代理向主代理的报告改由 durable
        // 阶段提交 + 主代理主动查询（read_agent_submissions）承载。
        let snapshot = self
            .runtime
            .snapshot(target.clone())
            .await
            .map_err(|error| tool_error(TOOL_SEND_MESSAGE, error.to_string()))?;
        if snapshot.identity.parent_id.as_ref() != Some(&self.caller) {
            return Err(tool_error(
                TOOL_SEND_MESSAGE,
                format!(
                    "agent `{target}` is not a direct child of `{}`; send_message only steers direct children",
                    self.caller
                ),
            ));
        }
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
        let messages = result
            .messages
            .iter()
            .map(|message| compact_wait_message(message, &snapshots))
            .collect::<Vec<_>>();
        json_output(json!({ "reason": result.reason, "messages": messages }))
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

    async fn read_submissions(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: SubmissionsArgs = parse_input(TOOL_READ_AGENT_SUBMISSIONS, input.arguments)?;
        let target = parse_agent_id(TOOL_READ_AGENT_SUBMISSIONS, args.target)?;
        self.authorize(&self.policy.list_targets, &target).await?;
        let offset = args.offset.unwrap_or(DEFAULT_SUBMISSION_OFFSET);
        let limit = args
            .limit
            .unwrap_or(DEFAULT_SUBMISSION_LIMIT)
            .clamp(1, MAX_SUBMISSION_LIMIT);
        let page = self
            .runtime
            .read_submissions(target, offset, limit)
            .await
            .map_err(|error| tool_error(TOOL_READ_AGENT_SUBMISSIONS, error.to_string()))?;
        json_output_with_budget(json!(page), MAX_SUBMISSION_OUTPUT_BYTES)
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

fn compact_wait_message(message: &AgentDirectoryWaitMessage, all: &[AgentSnapshot]) -> Value {
    let progress = message.message.as_ref().map(|progress| {
        json!({
            "stage": progress.report.stage,
            "summary": progress.report.summary,
            "nextStep": progress.report.next_step,
        })
    });
    json!({
        "agentId": message.identity.id,
        "path": agent_path(&message.identity.id, all),
        "role": message.identity.role,
        "message": progress,
        "state": {
            "lifecycle": message.lifecycle,
            "activity": message.activity,
            "turnOutcome": message.turn_outcome,
        },
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
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForCompletion,
}

impl From<ProgressStage> for AgentProgressStage {
    fn from(value: ProgressStage) -> Self {
        match value {
            ProgressStage::Exploring => Self::Exploring,
            ProgressStage::Implementing => Self::Implementing,
            ProgressStage::Verifying => Self::Verifying,
            ProgressStage::Blocked => Self::Blocked,
            ProgressStage::ReadyForCompletion => Self::ReadyForCompletion,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionsArgs {
    target: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
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
    fn report_progress_exposes_pre_completion_stage_without_review_authorization() {
        let schema = progress_schema();
        let stages = schema["properties"]["stage"]["enum"]
            .as_array()
            .expect("progress stages must be an array");

        assert!(stages.iter().any(|stage| stage == "readyForCompletion"));
        assert!(!stages.iter().any(|stage| stage == "readyForReview"));
        assert!(
            CollaborationToolKind::ReportProgress
                .description()
                .contains("never creates a completion")
        );
    }

    #[test]
    fn wait_message_projection_contains_only_latest_delta() {
        let agent_id = AgentId::new("executor").unwrap();
        let message = AgentDirectoryWaitMessage {
            identity: super::super::AgentIdentity {
                id: agent_id,
                parent_id: None,
                role: AgentRoleId::new("executor").unwrap(),
                depth: 0,
            },
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Active(ActiveKind::Running),
            message: Some(super::super::AgentProgressCheckpoint {
                report: super::super::AgentProgressReport {
                    stage: AgentProgressStage::Verifying,
                    summary: "验证完成".to_string(),
                    next_step: "等待审查".to_string(),
                    revision: 3,
                },
                updated_at: 123,
            }),
            turn_outcome: None,
        };

        let output = compact_wait_message(&message, &[]);

        assert_eq!(output["agentId"], "executor");
        assert_eq!(output["path"], serde_json::json!(["executor"]));
        assert_eq!(output["message"]["stage"], "verifying");
        assert_eq!(output["message"]["summary"], "验证完成");
        assert!(output["message"].get("revision").is_none());
        assert!(output.get("agents").is_none());
        assert!(output["state"]["turnOutcome"].is_null());

        let terminal = AgentDirectoryWaitMessage {
            identity: message.identity,
            lifecycle: AgentLifecycleState::Closed,
            activity: AgentActivityState::Idle,
            message: None,
            turn_outcome: Some(super::super::TurnOutcomeKind::Failed),
        };
        let terminal_output = compact_wait_message(&terminal, &[]);
        assert!(terminal_output["message"].is_null());
        assert_eq!(terminal_output["state"]["turnOutcome"], "failed");
    }

    #[test]
    fn read_session_age_gate_only_applies_while_agent_has_active_work() {
        assert!(session_read_requires_age_gate(
            AgentLifecycleState::Active,
            AgentActivityState::Active(ActiveKind::Running),
        ));
        assert!(session_read_requires_age_gate(
            AgentLifecycleState::Active,
            AgentActivityState::Active(ActiveKind::WaitingTool),
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

    #[test]
    fn only_wait_agents_pauses_active_wall_clock() {
        for kind in CollaborationToolKind::ALL {
            let expected = if matches!(kind, CollaborationToolKind::Wait) {
                ToolBudgetTiming::PauseWhenOnlyScheduledTool
            } else {
                ToolBudgetTiming::Count
            };
            assert_eq!(kind.budget_timing(), expected);
        }
        assert!(
            CollaborationToolKind::Wait
                .description()
                .contains("latest changed agent messages")
        );
        assert!(
            CollaborationToolKind::List
                .description()
                .contains("discovering targets")
        );
    }
}
