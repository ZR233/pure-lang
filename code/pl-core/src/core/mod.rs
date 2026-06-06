use std::collections::HashMap;
use std::path::PathBuf;

use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
#[cfg(test)]
use pl_protocol::{AgentEvent, ErrorSeverity, TimelineItemStatus};
use pl_protocol::{AgentEventSender, Message, MessageContent, MessageRole, Result};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::permission::parse_reviewer_decision;
use crate::session::CoreSession;
#[cfg(test)]
use crate::tool::WorkspaceAccess;
use crate::tool::{
    ApplyPatchTool, AskUserTool, BashTool, CloseAgentTool, CopyPathTool, CreateDirectoryTool,
    DeletePathTool, FollowupTaskTool, ListAgentsTool, ListFilesTool, MovePathTool, ReadFileTool,
    SearchFilesTool, SendMessageTool, SkillManageTool, SkillViewTool, SkillsListTool,
    SpawnAgentTool, StatPathTool, SubagentContext, ToolContext, ToolRegistry, WaitAgentTool,
    WriteFileTool,
};
use crate::trace::TraceRecorder;
#[cfg(test)]
use crate::turn::{BudgetTracker, TurnResultStatus};
use crate::turn::{
    ToolApprovalDecision, ToolApprovalRequest, TurnOptions, TurnRequest, TurnResult,
};

mod permission;
mod tool_dispatch;
mod turn_loop;
mod turn_result;

pub(crate) use turn_result::compact_text;
/// 生成唯一的会话 ID（毫秒时间戳 + 序列号）。
fn generate_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:x}")
}

const SUBAGENT_DISPATCH_CONSTRAINT: &str = "\n\n# 子代理调度约束\n用户明确要求使用 subagent/子代理分工时，必须先调度 `spawn_agent` 工具；不要只用 `bash` 或文件工具替代。若尚未知道 crate 列表，可以先用只读工具定位 workspace，再为每个 crate 创建 explorer agent，最后由父会话汇总。如果 `wait_agent` 或 `list_agents` 返回 `recoverableSubagentProvider429` 或 `recoverableFailures`，表示 provider 429 并发/容量上限导致子代理不可用；此时停止继续创建或重试子代理，由当前父 agent 自己完成剩余工作。";

const SUBAGENT_FORCE_DISPATCH_INSTRUCTION: &str = "# 当前轮强制要求\n前面已进行了必要定位但尚未创建 agent。本轮必须调用 `spawn_agent`，不要继续调用文件、shell 或搜索工具，也不要输出最终回答。若工具结果提示 `recoverableSubagentProvider429`，后续不再重试创建子代理，改由当前 agent 自己完成任务。";

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider、工具注册表和单轮编译请求。
/// 工具能力由调用方显式注册，并通过 `TurnOptions` 控制审批策略。
#[derive(Debug)]
pub struct PureCore {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_root: Option<PathBuf>,
    workspace_instructions: Option<String>,
    active_subagent: Option<SubagentContext>,
    agent_control: crate::AgentControl,
    tools: ToolRegistry,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        }
    }

    pub fn with_reasoning_effort(
        provider: SharedModelProvider,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            provider,
            reasoning_effort: Some(reasoning_effort),
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn default_provider() -> Result<Self> {
        Self::from_provider_info(ProviderInfo::default_provider())
    }

    pub fn from_config(config: &PureConfig, role: ModelRole) -> Result<Self> {
        let resolved = config.resolve_role(role)?;
        let provider = create_provider_with_models(resolved.provider_info, resolved.models)?;
        Ok(Self {
            provider,
            reasoning_effort: Some(resolved.role_config.effort),
            config: Some(config.clone()),
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        })
    }

    pub(crate) fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub(crate) fn with_agent_control(mut self, agent_control: crate::AgentControl) -> Self {
        self.agent_control = agent_control;
        self
    }

    /// 注册一个工具。
    pub fn register_tool(&mut self, tool: impl crate::tool::Tool + 'static) {
        self.tools.register(tool);
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 shell、异步 agent 协作工具和 workspace 文件工具。调用方应通过 `TurnOptions` 控制审批策略。
    pub fn register_default_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace_root.into();
        self.workspace_root = Some(workspace_root.clone());
        self.workspace_instructions = workspace_instructions.clone();
        self.register_skill_tools_for_workspace(
            workspace_root.clone(),
            workspace_instructions.clone(),
        );
        self.register_tool(BashTool::new(workspace_root));
        self.register_tool(ReadFileTool::new());
        self.register_tool(WriteFileTool);
        self.register_tool(ListFilesTool);
        self.register_tool(SearchFilesTool);
        self.register_tool(StatPathTool);
        self.register_tool(CreateDirectoryTool);
        self.register_tool(DeletePathTool);
        self.register_tool(CopyPathTool);
        self.register_tool(MovePathTool);
        self.register_tool(ApplyPatchTool);
        self.register_tool(SpawnAgentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(WaitAgentTool);
        self.register_tool(ListAgentsTool);
        self.register_tool(SendMessageTool);
        self.register_tool(FollowupTaskTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(CloseAgentTool);
        self.register_tool(AskUserTool);
    }

    pub(crate) fn register_skill_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        self.register_skill_tools_for_workspace(workspace_root.into(), workspace_instructions);
    }

    fn register_skill_tools_for_workspace(
        &mut self,
        workspace_root: std::path::PathBuf,
        workspace_instructions: Option<String>,
    ) {
        self.workspace_root = Some(workspace_root);
        self.workspace_instructions = workspace_instructions;
        let Some(config) = self.config.as_ref().map(|config| config.skills.clone()) else {
            return;
        };
        if !config.enabled {
            return;
        }
        self.register_tool(SkillsListTool::new(config.clone()));
        self.register_tool(SkillViewTool::new(config.clone()));
        self.register_tool(SkillManageTool::new(config));
    }

    async fn review_tool_call_with_ai(
        &self,
        request: &ToolApprovalRequest,
        context: &ToolContext,
    ) -> ToolApprovalDecision {
        let (provider, reasoning_effort) = match &self.config {
            Some(config) => match PureCore::from_config(config, ModelRole::Reviewer) {
                Ok(core) => (core.provider, core.reasoning_effort),
                Err(error) => {
                    return ToolApprovalDecision::Denied {
                        reason: format!("AI reviewer is unavailable: {error}"),
                    };
                }
            },
            None => (self.provider.clone(), self.reasoning_effort.clone()),
        };
        let reasoning = reasoning_effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(ReasoningSummary::Enabled),
        });
        let payload = serde_json::json!({
            "toolName": &request.name,
            "arguments": &request.arguments,
            "workingDirectory": &request.working_directory,
            "parentAgentId": &request.parent_agent_id,
            "permissionMode": context.options.permission_mode.label(),
            "workspaceAccess": format!("{:?}", context.workspace_access),
            "compileMode": context.mode.label(),
            "workspaceRoot": context.workspace_root.display().to_string(),
            "riskSummary": permission::permission_risk_summary(&request.name),
        });
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
            ),
            reasoning_content: None,
            metadata: HashMap::new(),
        };
        let completion_request = CompletionRequest {
            model: provider.default_model().to_string(),
            instructions: Some(include_str!("../../prompts/permission_review.md").to_string()),
            messages: vec![message],
            tools: Vec::new(),
            tool_choice: "none".to_string(),
            parallel_tool_calls: false,
            temperature: Some(0.0),
            max_tokens: Some(512),
            reasoning,
            stream: false,
            timeline: None,
        };
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
        match provider.stream_complete(completion_request, event_tx).await {
            Ok(response) => {
                let content = response
                    .raw_content
                    .or(response.content)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    return ToolApprovalDecision::Denied {
                        reason: "AI reviewer returned an empty decision".to_string(),
                    };
                }
                match parse_reviewer_decision(&content) {
                    Ok(decision) => decision,
                    Err(error) => ToolApprovalDecision::Denied { reason: error },
                }
            }
            Err(error) => ToolApprovalDecision::Denied {
                reason: format!("AI reviewer failed: {error}"),
            },
        }
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.run_turn_with_options(session, request, event_tx, TurnOptions::default())
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        let mut recorder = TraceRecorder::disabled(event_tx);
        self.run_turn_with_trace(session, request, &mut recorder, options)
            .await
    }

    pub async fn run_turn_with_trace(
        &self,
        session: &mut CoreSession,
        request: TurnRequest,
        recorder: &mut TraceRecorder,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        turn_loop::run_turn_with_trace(self, session, request, recorder, options).await
    }
}

// Re-export for tests
#[cfg(test)]
use permission::{approval_request, approve_tool_call};
#[cfg(test)]
use pl_model::{TokenUsage, ToolCallKind};
#[cfg(test)]
use tool_dispatch::{
    ToolExecutionContext, ToolExecutionRecord, execute_tool_calls,
    namespaced_tool_timeline_item_id, tool_results_include_recoverable_subagent_capacity,
};
#[cfg(test)]
use turn_result::{
    failed_turn_result, format_instructions, looks_like_unexecuted_tool_call_text,
    prompt_requires_subagent_dispatch, provider_error_severity, tool_allowed_in_mode,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{PermissionMode, ToolApprovalPolicy};
    use crate::{ConfigStore, ModelRole};
    use pl_model::ToolCall;
    use pl_protocol::TimelineTextRole;
    use pretty_assertions::assert_eq;

    fn test_tool_context(event_tx: AgentEventSender) -> ToolContext {
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
        }
    }

    #[test]
    fn config_core_uses_planner_role_model_and_effort() {
        let config = ConfigStore::new(crate::ConfigPaths::from_home("unused"))
            .load_or_default()
            .unwrap();
        let core = PureCore::from_config(&config, ModelRole::Planner).unwrap();

        assert_eq!(core.provider.default_model(), "deepseek-v4-flash");
        assert_eq!(core.reasoning_effort.unwrap().as_str(), "high");
    }

    #[test]
    fn format_instructions_without_workspace() {
        assert_eq!(format_instructions("base", None, None), "base");
    }

    #[test]
    fn format_instructions_with_workspace() {
        assert_eq!(
            format_instructions("base", None, Some("project rules")),
            "base\n\n# 项目记忆\nproject rules"
        );
    }

    #[test]
    fn format_instructions_injects_skills_before_workspace() {
        assert_eq!(
            format_instructions("base", Some("# Skills\n- rust"), Some("project rules")),
            "base\n\n# Skills\n- rust\n\n# 项目记忆\nproject rules"
        );
    }

    #[test]
    fn format_instructions_ignores_empty_workspace() {
        assert_eq!(format_instructions("base", None, Some("")), "base");
        assert_eq!(format_instructions("base", None, Some("   ")), "base");
    }

    #[test]
    fn detects_explicit_subagent_partition_requests() {
        assert!(prompt_requires_subagent_dispatch(
            "每个 crate 分一个 subagent 探索，然后介绍整个项目"
        ));
        assert!(prompt_requires_subagent_dispatch(
            "请分别用子代理探索前端和后端"
        ));
        assert!(!prompt_requires_subagent_dispatch("介绍整个项目"));
        assert!(!prompt_requires_subagent_dispatch(
            "用 bash 看一下每个 crate"
        ));
        assert!(!prompt_requires_subagent_dispatch(
            "读取 src/tool/subagent.rs，并总结每个模块的职责"
        ));
    }

    #[test]
    fn subagent_dispatch_instructions_describe_recoverable_429() {
        assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("429"));
        assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("recoverableSubagentProvider429"));
        assert!(SUBAGENT_FORCE_DISPATCH_INSTRUCTION.contains("429"));
    }

    #[test]
    fn detects_recoverable_subagent_tool_result_marker() {
        let records = vec![
            ToolExecutionRecord {
                call_id: "call-1".to_string(),
                name: "spawn_agent".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: retry locally".to_string(),
                display_result: "recoverableSubagentProvider429: retry locally".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
            },
            ToolExecutionRecord {
                call_id: "call-2".to_string(),
                name: "bash".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: unrelated text".to_string(),
                display_result: "recoverableSubagentProvider429: unrelated text".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
            },
        ];

        assert!(tool_results_include_recoverable_subagent_capacity(&records));
        assert!(!tool_results_include_recoverable_subagent_capacity(
            &records[1..]
        ));
    }

    #[test]
    fn plan_mode_tool_allowlist_exposes_only_read_and_agent_tools() {
        let auto = crate::turn::CompileMode::Auto;
        let plan = crate::turn::CompileMode::Plan;

        assert!(tool_allowed_in_mode(auto, "write_file"));
        assert!(tool_allowed_in_mode(plan, "read_file"));
        assert!(tool_allowed_in_mode(plan, "search_files"));
        assert!(tool_allowed_in_mode(plan, "skills_list"));
        assert!(tool_allowed_in_mode(plan, "skill_view"));
        assert!(tool_allowed_in_mode(plan, "spawn_agent"));
        assert!(tool_allowed_in_mode(plan, "followup_task"));
        assert!(tool_allowed_in_mode(plan, "request_user_input"));
        assert!(tool_allowed_in_mode(plan, "bash"));
        assert!(!tool_allowed_in_mode(plan, "subagent"));
        assert!(!tool_allowed_in_mode(plan, "write_file"));
        assert!(!tool_allowed_in_mode(plan, "apply_patch"));
        assert!(!tool_allowed_in_mode(plan, "delete_path"));
        assert!(!tool_allowed_in_mode(plan, "skill_manage"));
    }

    #[test]
    fn tool_timeline_item_ids_are_scoped_to_turn() {
        assert_eq!(
            namespaced_tool_timeline_item_id("turn-1", "call_0"),
            "turn-1-call_0"
        );
        assert_eq!(
            namespaced_tool_timeline_item_id("turn-1", "turn-1-call_0"),
            "turn-1-call_0"
        );
    }

    #[test]
    fn root_provider_429_is_transient_but_subagent_provider_429_stays_recoverable() {
        assert!(matches!(
            provider_error_severity(None, "API error 429 Too Many Requests"),
            ErrorSeverity::Transient
        ));

        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: None,
            agent_path: Some("/root/worker".to_string()),
            role: "executor".to_string(),
            task: "inspect worker".to_string(),
            depth: 1,
        };
        assert!(matches!(
            provider_error_severity(Some(&subagent), "API error 429 Too Many Requests"),
            ErrorSeverity::Recoverable
        ));
        assert!(matches!(
            provider_error_severity(None, "API error 500"),
            ErrorSeverity::Recoverable
        ));
    }

    #[test]
    fn detects_unexecuted_tool_call_text() {
        assert!(looks_like_unexecuted_tool_call_text(
            "<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"spawn_agent\">"
        ));
        assert!(looks_like_unexecuted_tool_call_text(
            r#"{"tool_calls":[{"name":"spawn_agent"}]}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "源码中有 tool_calls 字段、name 字段和 subagent.rs 文件。"
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            r#"{"summary":"tool_calls and name are discussed in docs"}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "已完成探索，没有工具调用文本。"
        ));
    }

    #[test]
    fn failed_turn_result_preserves_error_message() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);

        let result = failed_turn_result(
            &mut recorder,
            "turn-1",
            crate::turn::CompileMode::Auto,
            "partial summary".to_string(),
            None,
            "model-a".to_string(),
            TokenUsage::default(),
            3,
            "provider rejected request".to_string(),
            ErrorSeverity::Transient,
        );

        assert_eq!(result.status, TurnResultStatus::Errored);
        assert_eq!(
            result.abort_reason,
            Some(crate::turn::TurnAbortReason::ProviderError),
        );
        assert_eq!(result.content, "partial summary");
        assert_eq!(result.error.as_deref(), Some("provider rejected request"));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { item }
                if item.item_id == "turn-1-assistant"
                    && item.role == Some(TimelineTextRole::Assistant)
                    && item.content == "partial summary"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemCompleted { item, .. }
                if item.item_id == "turn-1-assistant"
                    && item.role == Some(TimelineTextRole::Assistant)
                    && item.content == "partial summary"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemFailed { item, .. } if item.item_id == "turn-1-turn"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::Error {
                severity: ErrorSeverity::Transient,
                ..
            }
        ));
        assert!(matches!(event_rx.try_recv().unwrap(), AgentEvent::Done));
    }

    #[test]
    fn default_turn_options_auto_allow_tools() {
        let options = TurnOptions::default();

        assert_eq!(options.tool_approval_policy, ToolApprovalPolicy::AutoAllow);
        assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
        assert!(options.tool_approval_callback.is_none());
    }

    #[tokio::test]
    async fn manual_tool_approval_can_approve() {
        let options = TurnOptions::manual(std::sync::Arc::new(|request| {
            Box::pin(async move {
                assert_eq!(request.name, "bash");
                ToolApprovalDecision::Approved
            })
        }));
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;
        let event = event_rx.recv().await.unwrap();

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(matches!(
            event,
            AgentEvent::ToolApprovalRequested {
                id,
                name,
                ..
            } if id == "call-1" && name == "bash"
        ));
    }

    #[tokio::test]
    async fn plan_mode_bash_requires_manual_approval_even_when_auto_allowed() {
        let options = TurnOptions::default();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "pwd"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx.clone());
        context.mode = crate::turn::CompileMode::Plan;

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "manual approval required but no approver is configured".to_string()
            }
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::ToolApprovalRequested {
                id,
                name,
                ..
            } if id == "call-1" && name == "bash"
        ));
    }

    #[tokio::test]
    async fn full_access_plan_bash_does_not_request_manual_approval() {
        let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "pwd"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx.clone());
        context.mode = crate::turn::CompileMode::Plan;

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn plan_mode_read_tool_still_uses_auto_allow() {
        let options = TurnOptions::default();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "Cargo.toml"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx.clone());
        context.mode = crate::turn::CompileMode::Plan;

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn plan_mode_denies_disallowed_tool_before_execution_even_with_full_access() {
        let core = PureCore::default_provider().unwrap();
        let tool_call = ToolCall::function(
            "call-1",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "oops"}),
            None,
        );
        let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
        let workspace_root = std::env::temp_dir();

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &options,
                mode: crate::turn::CompileMode::Plan,
                session_id: "turn-1",
                workspace_root: &workspace_root,
                workspace_instructions: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
            },
        )
        .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Denied);
        assert_eq!(records[0].name, "write_file");
        assert_eq!(records[0].result, "Tool disabled in plan mode: write_file");
    }

    #[tokio::test]
    async fn request_approval_allows_external_path_after_user_approval() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_root =
            std::env::temp_dir().join(format!("pure-permission-workspace-{unique}"));
        let outside_root = std::env::temp_dir().join(format!("pure-permission-outside-{unique}"));
        tokio::fs::create_dir_all(&workspace_root).await.unwrap();
        tokio::fs::create_dir_all(&outside_root).await.unwrap();
        let outside_file = outside_root.join("note.txt");
        tokio::fs::write(&outside_file, "external ok")
            .await
            .unwrap();
        let mut core = PureCore::default_provider().unwrap();
        core.register_tool(ReadFileTool::new());
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({"path": outside_file.to_string_lossy()}),
            None,
        );
        let options = TurnOptions {
            tool_approval_callback: Some(std::sync::Arc::new(|request| {
                Box::pin(async move {
                    assert_eq!(request.name, "read_file");
                    ToolApprovalDecision::Approved
                })
            })),
            ..Default::default()
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &options,
                mode: crate::turn::CompileMode::Auto,
                session_id: "turn-1",
                workspace_root: &workspace_root,
                workspace_instructions: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
            },
        )
        .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Completed);
        assert!(records[0].result.contains("external ok"));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::ToolApprovalRequested { name, .. } if name == "read_file"
        ));
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
        let _ = tokio::fs::remove_dir_all(outside_root).await;
    }

    #[tokio::test]
    async fn deny_all_tool_approval_denies_without_request_event() {
        let options = TurnOptions::deny_all();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "tool execution denied by policy".to_string()
            }
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn approval_request_extracts_working_directory() {
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({
                "command": "pwd",
                "workingDirectory": "C:/work"
            }),
            None,
        );

        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let request = approval_request(&call, &test_tool_context(event_tx));

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn approval_request_marks_parent_agent() {
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({"command": "pwd"}),
            None,
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx);
        context.active_subagent = Some(SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            agent_path: None,
            role: "executor".to_string(),
            task: "inspect".to_string(),
            depth: 1,
        });

        let request = approval_request(&call, &context);

        assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
    }

    #[test]
    fn default_tools_register_bash_and_agent_tools() {
        let mut core = PureCore::default_provider().unwrap();

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()));

        assert!(core.tools.get("bash").is_some());
        assert!(core.tools.get("spawn_agent").is_some());
        assert!(core.tools.get("wait_agent").is_some());
        assert!(core.tools.get("list_agents").is_some());
        assert!(core.tools.get("request_user_input").is_some());
        assert!(core.tools.get("subagent").is_none());
        assert!(core.tools.get("read_file").is_some());
        assert!(core.tools.get("apply_patch").is_some());
    }
}
