use std::{future::Future, sync::Arc};

use pl_protocol::PureError;
use serde_json::{Value, json};

use super::*;
use crate::tool::{DynTool, StaticTool, ToolPolicy};
use crate::{AgentRoleId, ToolCallContext, ToolEffect, ToolInput, ToolResult, ToolSessionRuntime};

const TOOL_SPAWN_AGENT: &str = "spawn_agent";
const TOOL_LIST_AGENT_PROFILES: &str = "list_agent_profiles";
const TOOL_REPORT_PROGRESS: &str = "report_progress";
const TOOL_SEND_MESSAGE: &str = "send_message";
const TOOL_INTERRUPT_AGENT: &str = "interrupt_agent";
const TOOL_LIST_AGENTS: &str = "list_agents";
const TOOL_READ_AGENT_SESSION: &str = "read_agent_session";
const TOOL_READ_AGENT_SUBMISSIONS: &str = "read_agent_submissions";
const TOOL_CLOSE_AGENT: &str = "close_agent";
const DEFAULT_SESSION_LIMIT: usize = 20;
const MAX_SESSION_LIMIT: usize = 50;
const MAX_SESSION_OUTPUT_BYTES: usize = 256 * 1024;
const DEFAULT_SUBMISSION_OFFSET: usize = 0;
const DEFAULT_SUBMISSION_LIMIT: usize = 20;
const MAX_SUBMISSION_LIMIT: usize = 50;
/// read_agent_submissions 单页硬字节上限：覆盖默认 12KB 安全阈值，保证 detail 全文返回。
const MAX_SUBMISSION_OUTPUT_BYTES: usize = 64 * 1024;

mod args;
mod event_source;
mod session;
mod summary;
pub(super) mod support;

use crate::tool::tool_error;
use args::*;
use summary::*;
use support::{
    close_schema, filter_visible, fork_session, json_output, json_output_with_budget,
    object_schema, parse_agent_id, parse_input, progress_schema, resolve_profile_writable_paths,
    send_message_schema, session_schema, session_target_visible, spawn_schema, submissions_schema,
    target_schema,
};

/// 为一次 turn 构造由 `AgentRuntimeHandle` 驱动的协作工具。
#[derive(Debug, Clone)]
pub struct AgentCollaborationTools {
    runtime: AgentRuntimeHandle,
    caller: ThreadId,
    policy: AgentAccessPolicy,
    session_runtime: ToolSessionRuntime,
    workspace_root: std::path::PathBuf,
    profiles: Arc<Vec<pl_protocol::AgentProfileSnapshot>>,
}

#[derive(Debug, Clone)]
pub struct AgentCollaborationToolConfig {
    pub policy: AgentAccessPolicy,
    pub session_runtime: ToolSessionRuntime,
    pub workspace_root: std::path::PathBuf,
    /// 本 turn 可用的已启用 Profile；创建子 Agent 时会冻结其中的完整快照。
    pub profiles: Vec<pl_protocol::AgentProfileSnapshot>,
}

impl AgentCollaborationTools {
    /// Registers collaboration wakeups without exposing a second model waiting tool.
    pub fn event_source(&self) -> Option<impl crate::session_runtime::SessionEventSource + use<>> {
        (!matches!(self.policy.list_targets, AgentTargetSelector::None)).then(|| {
            event_source::AgentEventSource {
                runtime: self.runtime.clone(),
                caller: self.caller.clone(),
                selector: self.policy.list_targets.clone(),
            }
        })
    }
    pub fn new(
        runtime: AgentRuntimeHandle,
        caller: ThreadId,
        config: AgentCollaborationToolConfig,
    ) -> Self {
        Self {
            runtime,
            caller,
            policy: config.policy,
            session_runtime: config.session_runtime,
            workspace_root: config.workspace_root,
            profiles: Arc::new(config.profiles),
        }
    }

    /// 返回可直接注册到 `TurnEngine` 的协作工具。
    ///
    /// 所有 Agent Profile 共享同一套基础能力：send_message 仅允许
    /// parent→direct-child 调度，子代理向主代理的报告改由 durable 阶段提交
    /// 与 read_agent_submissions 查询承载。
    pub fn tools(&self) -> Vec<DynTool> {
        let controller = !self.policy.spawn_roles.is_empty()
            || !matches!(self.policy.list_targets, AgentTargetSelector::None)
            || !matches!(self.policy.message_targets, AgentTargetSelector::None)
            || !matches!(self.policy.close_targets, AgentTargetSelector::None);
        CollaborationToolKind::ALL
            .into_iter()
            .filter(|kind| match kind {
                CollaborationToolKind::ReportProgress => !controller,
                CollaborationToolKind::Spawn
                | CollaborationToolKind::ListProfiles
                | CollaborationToolKind::SendMessage
                | CollaborationToolKind::Interrupt
                | CollaborationToolKind::List
                | CollaborationToolKind::ReadSession
                | CollaborationToolKind::ReadSubmissions
                | CollaborationToolKind::Close => controller,
            })
            .map(|kind| {
                CollaborationTool {
                    kind,
                    runtime: self.runtime.clone(),
                    caller: self.caller.clone(),
                    policy: self.policy.clone(),
                    session_runtime: self.session_runtime.clone(),
                    workspace_root: self.workspace_root.clone(),
                    profiles: self.profiles.clone(),
                }
                .into()
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum CollaborationToolKind {
    Spawn,
    ListProfiles,
    ReportProgress,
    SendMessage,
    Interrupt,
    List,
    ReadSession,
    ReadSubmissions,
    Close,
}

impl CollaborationToolKind {
    const ALL: [Self; 9] = [
        Self::Spawn,
        Self::ListProfiles,
        Self::ReportProgress,
        Self::SendMessage,
        Self::Interrupt,
        Self::List,
        Self::ReadSession,
        Self::ReadSubmissions,
        Self::Close,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Spawn => TOOL_SPAWN_AGENT,
            Self::ListProfiles => TOOL_LIST_AGENT_PROFILES,
            Self::ReportProgress => TOOL_REPORT_PROGRESS,
            Self::SendMessage => TOOL_SEND_MESSAGE,
            Self::Interrupt => TOOL_INTERRUPT_AGENT,
            Self::List => TOOL_LIST_AGENTS,
            Self::ReadSession => TOOL_READ_AGENT_SESSION,
            Self::ReadSubmissions => TOOL_READ_AGENT_SUBMISSIONS,
            Self::Close => TOOL_CLOSE_AGENT,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Spawn => {
                "Spawn a child Agent from an enabled Agent Profile. The Profile instructions, model route, and workspace assignment are frozen into the new session. Directory writablePaths constrain only Pure built-in file mutation tools; shell, Git, and MCP can bypass them."
            }
            Self::ListProfiles => {
                "List enabled Agent Profiles available to spawn, including their intended use and model selection."
            }
            Self::ReportProgress => {
                "Record the caller's current execution stage, concise summary, next step, and optional detailed report. Appends a durable submission the orchestrator can read later; never creates a completion or review authorization."
            }
            Self::SendMessage => {
                "Insert a steering message into a direct child agent's session without interrupting its active turn, and refresh the child's current turn budget. Only parent-to-direct-child is allowed."
            }
            Self::Interrupt => "Interrupt an accessible agent's current turn.",
            Self::List => {
                "List full compact canonical snapshots for visible agents when discovering targets, reconciling after restart, or diagnosing stalled work."
            }
            Self::ReadSession => {
                "Read a complete durable agent Timeline with stable keyset pagination. Defaults to the newest 20 text items; use order and detail to inspect the full execution history. This diagnostic does not replace durable submissions."
            }
            Self::ReadSubmissions => {
                "Read the durable stage submission history for an agent (full content, paginated, not truncated; works after the target has closed)."
            }
            Self::Close => {
                "Begin closing an accessible child agent and return its canonical state. Closing is not completed cleanup: use wait for the agent's closed event or inspect a cleanup error. Worktrees are preserved by default; workspaceDisposition=cleanup requires completed integration."
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CollaborationTool {
    kind: CollaborationToolKind,
    runtime: AgentRuntimeHandle,
    caller: ThreadId,
    policy: AgentAccessPolicy,
    session_runtime: ToolSessionRuntime,
    workspace_root: std::path::PathBuf,
    profiles: Arc<Vec<pl_protocol::AgentProfileSnapshot>>,
}

impl StaticTool for CollaborationTool {
    type Input = Value;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(self.kind.name()),
            self.kind.description(),
        )
    }

    fn input_schema(&self) -> Value {
        match self.kind {
            CollaborationToolKind::Spawn => spawn_schema(&self.policy, &self.profiles),
            CollaborationToolKind::ListProfiles => object_schema(Vec::new()),
            CollaborationToolKind::ReportProgress => progress_schema(),
            CollaborationToolKind::SendMessage => send_message_schema(),
            CollaborationToolKind::Interrupt => target_schema(
                &self.policy.message_targets,
                "Agent id whose current turn should be interrupted.",
            ),
            CollaborationToolKind::List => object_schema(Vec::new()),
            CollaborationToolKind::ReadSession => session_schema(&self.policy.list_targets),
            CollaborationToolKind::ReadSubmissions => submissions_schema(&self.policy.list_targets),
            CollaborationToolKind::Close => close_schema(&self.policy.close_targets),
        }
    }

    fn policy(&self) -> ToolPolicy {
        let policy = ToolPolicy::control().with_effect(ToolEffect::AgentControl);
        if matches!(
            self.kind,
            CollaborationToolKind::SendMessage
                | CollaborationToolKind::Interrupt
                | CollaborationToolKind::List
                | CollaborationToolKind::ListProfiles
                | CollaborationToolKind::ReadSession
                | CollaborationToolKind::ReadSubmissions
        ) {
            policy.with_parallel_tool_calls()
        } else {
            policy
        }
    }

    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let input = ToolInput { arguments: input };
            match self.kind {
                CollaborationToolKind::Spawn => self.spawn(input, context).await,
                CollaborationToolKind::ListProfiles => self.list_profiles(input),
                CollaborationToolKind::ReportProgress => self.report_progress(input).await,
                CollaborationToolKind::SendMessage => self.send_message(input).await,
                CollaborationToolKind::Interrupt => self.interrupt(input).await,
                CollaborationToolKind::List => self.list(input).await,
                CollaborationToolKind::ReadSession => self.read_session(input).await,
                CollaborationToolKind::ReadSubmissions => self.read_submissions(input).await,
                CollaborationToolKind::Close => self.close(input).await,
            }
        }
    }
}

impl CollaborationTool {
    async fn spawn(
        &self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> Result<ToolResult, PureError> {
        let args: SpawnArgs = parse_input(TOOL_SPAWN_AGENT, input.arguments)?;
        let profile = self
            .profiles
            .iter()
            .find(|profile| profile.profile_id == args.profile_id)
            .cloned()
            .ok_or_else(|| {
                tool_error(
                    TOOL_SPAWN_AGENT,
                    format!(
                        "agent profile `{}` is not enabled or available",
                        args.profile_id
                    ),
                )
            })?;
        let role = AgentRoleId::new(profile.profile_id.clone())
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        if !self.policy.spawn_roles.contains(&role) {
            return Err(tool_error(
                TOOL_SPAWN_AGENT,
                format!("agent profile `{role}` is not allowed for this turn"),
            ));
        }
        let writable_paths =
            resolve_profile_writable_paths(&self.workspace_root, &profile, args.writable_paths)?;
        let thread_id = ThreadId::generate();
        let mut child_session =
            fork_session(&self.session_runtime.parent_session(), args.fork_turns)?;
        child_session.replace_agent_profile(Some(profile.clone()));
        let session = ThreadContextState {
            session: child_session,
            ..ThreadContextState::empty()
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
            Value::String(context.identity().item_id.clone()),
        );
        metadata.insert(
            "workspaceRoot".to_string(),
            Value::String(self.workspace_root.to_string_lossy().to_string()),
        );
        metadata.insert(
            "profileId".to_string(),
            Value::String(profile.profile_id.clone()),
        );
        metadata.insert(
            "workspaceMode".to_string(),
            serde_json::to_value(profile.workspace_mode)
                .expect("Agent workspace mode must serialize"),
        );
        metadata.insert(
            "writablePaths".to_string(),
            serde_json::to_value(&writable_paths).expect("writable paths must serialize"),
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
            "profileId": profile.profile_id,
            "workspace": result.workspace_assignment,
        }))
    }

    fn list_profiles(&self, input: ToolInput) -> Result<ToolResult, PureError> {
        let _: EmptyArgs = parse_input(TOOL_LIST_AGENT_PROFILES, input.arguments)?;
        let profiles = self
            .profiles
            .iter()
            .filter_map(|profile| {
                let role = AgentRoleId::new(profile.profile_id.clone()).ok()?;
                self.policy.spawn_roles.contains(&role).then(|| {
                    json!({
                        "profileId": profile.profile_id,
                        "displayName": profile.display_name,
                        "description": profile.description,
                        "whenToUse": profile.when_to_use,
                        "providerId": profile.provider_id,
                        "model": profile.model,
                        "effort": profile.effort,
                        "system": profile.system,
                        "workspaceMode": profile.workspace_mode,
                    })
                })
            })
            .collect::<Vec<_>>();
        json_output(json!({ "profiles": profiles }))
    }

    async fn report_progress(&self, input: ToolInput) -> Result<ToolResult, PureError> {
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

    async fn send_message(&self, input: ToolInput) -> Result<ToolResult, PureError> {
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
                super::AgentCurrentSessionSubmitRequest::parent_agent(args.message)
                    .with_budget_action(super::MailboxBudgetAction::Refresh),
            )
            .await
            .map_err(|error| tool_error(TOOL_SEND_MESSAGE, error.to_string()))?;
        json_output(json!({ "target": target, "turnId": turn_id }))
    }

    async fn interrupt(&self, input: ToolInput) -> Result<ToolResult, PureError> {
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
        let turn_id = snapshot.active_turn_id().cloned().ok_or_else(|| {
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
                "state": snapshot.state,
                "lastTurnOutcome": snapshot.last_turn,
            }
        }))
    }

    async fn list(&self, input: ToolInput) -> Result<ToolResult, PureError> {
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

    async fn read_session(&self, input: ToolInput) -> Result<ToolResult, PureError> {
        let args: SessionArgs = parse_input(TOOL_READ_AGENT_SESSION, input.arguments)?;
        let target = parse_agent_id(TOOL_READ_AGENT_SESSION, args.target)?;
        let limit = args.limit.unwrap_or(DEFAULT_SESSION_LIMIT);
        if !(1..=MAX_SESSION_LIMIT).contains(&limit) {
            return Err(tool_error(
                TOOL_READ_AGENT_SESSION,
                format!("limit must be between 1 and {MAX_SESSION_LIMIT}"),
            ));
        }
        let query = session::query(
            target.clone(),
            args.order,
            args.detail,
            limit,
            args.cursor.as_deref(),
        )?;
        let repository = self
            .runtime
            .read_agent_session(query.clone())
            .await
            .map_err(|error| tool_error(TOOL_READ_AGENT_SESSION, error.to_string()))?;
        let caller_root =
            super::directory::root_agent_id_for(&self.runtime.directory_snapshot(), &self.caller)
                .map_err(|error| tool_error(TOOL_READ_AGENT_SESSION, error.to_string()))?;
        let allowed = session_target_visible(
            &self.policy.list_targets,
            &caller_root,
            &target,
            &repository.path,
        );
        if !allowed {
            return Err(tool_error(
                TOOL_READ_AGENT_SESSION,
                format!("agent `{target}` is not accessible for this turn"),
            ));
        }
        let page = session::page_with_budget(&query, repository, MAX_SESSION_OUTPUT_BYTES)?;
        json_output_with_budget(json!(page), MAX_SESSION_OUTPUT_BYTES)
    }

    async fn read_submissions(&self, input: ToolInput) -> Result<ToolResult, PureError> {
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

    async fn close(&self, input: ToolInput) -> Result<ToolResult, PureError> {
        let args: CloseArgs = parse_input(TOOL_CLOSE_AGENT, input.arguments)?;
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
            .close_with_disposition(target, args.workspace_disposition)
            .await
            .map_err(|error| tool_error(TOOL_CLOSE_AGENT, error.to_string()))?;
        json_output(json!({ "snapshot": snapshot }))
    }

    async fn authorize(
        &self,
        selector: &AgentTargetSelector,
        target: &ThreadId,
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
    fn read_session_schema_defaults_to_latest_text_page() {
        let schema = session_schema(&AgentTargetSelector::Tree);
        assert_eq!(schema["properties"]["limit"]["default"], 20);
        assert_eq!(schema["properties"]["order"]["default"], "descending");
        assert_eq!(schema["properties"]["detail"]["default"], "text");
        assert!(
            CollaborationToolKind::ReadSession
                .description()
                .contains("complete durable agent Timeline")
        );
    }
}
