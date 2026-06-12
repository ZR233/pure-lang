use std::collections::HashMap;
use std::path::PathBuf;

use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
#[cfg(test)]
use pl_protocol::{AgentEvent, ErrorSeverity, PureError, TimelineItemStatus, TraceEvent};
use pl_protocol::{AgentEventSender, Message, MessageContent, MessageRole, Result};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::permission::parse_reviewer_decision;
use crate::session::CoreSession;
#[cfg(test)]
use crate::tool::WorkspaceAccess;
use crate::tool::{
    ApplyPatchTool, AskUserTool, CloseAgentTool, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    FollowupTaskTool, ListAgentsTool, ListFilesTool, MovePathTool, ReadFileTool, SearchFilesTool,
    SendMessageTool, SkillManageTool, SkillViewTool, SkillsListTool, SpawnAgentTool, StatPathTool,
    SubagentContext, ToolContext, ToolRegistry, WaitAgentTool, WriteFileTool, command_tool_pair,
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
    mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
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
            mcp_runtime: None,
            lsp_runtime: None,
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
            mcp_runtime: None,
            lsp_runtime: None,
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
            mcp_runtime: None,
            lsp_runtime: None,
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

    pub fn with_mcp_runtime(mut self, registry: crate::mcp::McpRuntimeRegistry) -> Self {
        self.mcp_runtime = Some(registry);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    /// 注册一个工具。
    pub fn register_tool(&mut self, tool: impl crate::tool::Tool + 'static) {
        self.tools.register(tool);
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.tools.get(name).is_some()
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 shell、异步 agent 协作工具和 workspace 文件工具。调用方应通过 `TurnOptions` 控制审批策略。
    pub async fn register_default_tools(
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
        let (bash_tool, write_stdin_tool) = command_tool_pair(workspace_root.clone());
        self.register_tool(bash_tool);
        self.register_tool(write_stdin_tool);
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
        if let Some(registry) = self.lsp_runtime.clone() {
            self.tools.register_lsp_languages(&registry).await;
        }
        self.register_tool(SpawnAgentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            self.mcp_runtime.clone(),
            self.lsp_runtime.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(WaitAgentTool);
        self.register_tool(ListAgentsTool);
        self.register_tool(SendMessageTool);
        self.register_tool(FollowupTaskTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            self.mcp_runtime.clone(),
            self.lsp_runtime.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(CloseAgentTool);
        self.register_tool(AskUserTool);
    }

    pub async fn register_available_mcp_tools(&mut self) -> Result<()> {
        let Some(registry) = self.mcp_runtime.clone() else {
            return Ok(());
        };
        registry.register_available_tools(self).await
    }

    pub async fn register_configured_mcp_tools(&mut self) -> Result<()> {
        self.register_available_mcp_tools().await
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
    failed_turn_result, looks_like_unexecuted_tool_call_text, prompt_requires_subagent_dispatch,
    provider_error_severity, tool_allowed_in_mode,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolInput, ToolOutput};
    use crate::turn::{CompileMode, PermissionMode, ToolApprovalPolicy};
    use crate::{ConfigStore, ModelRole};
    use pl_model::ToolCall;
    use pl_protocol::{
        InteractionPayload, InteractionResolution, TimelineItemKind, TimelineTextRole,
        ToolApprovalResolution, TraceEventKind,
    };
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn test_tool_context(event_tx: AgentEventSender) -> ToolContext {
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            lsp_runtime: None,
            parent_session: std::sync::Arc::new(CoreSession::new()),
        }
    }

    fn terminal_tool_event_count(events: &[TraceEvent]) -> usize {
        events
            .iter()
            .filter(|event| match &event.kind {
                TraceEventKind::TimelineItemCompleted { item } => {
                    item.kind == pl_protocol::TimelineItemKind::Tool
                }
                TraceEventKind::TimelineItemFailed { item, .. } => {
                    item.kind == pl_protocol::TimelineItemKind::Tool
                }
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => false,
            })
            .count()
    }

    async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
                if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&buffer[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    break (header_end, content_length);
                }
            };

            while buffer.len() < header_end + 4 + content_length {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });

        (format!("http://{addr}"), handle)
    }

    fn timeline_started_kinds(events: &[TraceEvent]) -> Vec<TimelineItemKind> {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TimelineItemStarted { item } => Some(item.kind),
                TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::TimelineItemFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect()
    }

    fn record_enabled_tools_for_core(
        core: &PureCore,
        session_id: &str,
        turn_id: &str,
        mode: CompileMode,
    ) -> Vec<TraceEvent> {
        let tool_schemas = core
            .tools
            .schemas()
            .into_iter()
            .filter(|schema| tool_allowed_in_mode(mode, schema.name()))
            .collect::<Vec<_>>();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, 0);

        super::turn_loop::record_enabled_tools(&mut recorder, turn_id, mode, &tool_schemas);

        recorder.drain()
    }

    fn enabled_tools_event(events: &[TraceEvent]) -> &pl_protocol::EnabledToolsEvent {
        events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::EnabledToolsRecorded { event } => Some(event),
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::TimelineItemFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. } => None,
            })
            .expect("enabled tools event")
    }

    #[derive(Debug)]
    struct SleepingTool;

    impl Tool for SleepingTool {
        fn name(&self) -> &str {
            "sleeping_tool"
        }

        fn description(&self) -> &str {
            "Sleeps until the turn is cancelled"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn supports_parallel_tool_calls(&self) -> bool {
            true
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
            _context: ToolContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(ToolOutput {
                    description: "done".to_string(),
                    truncated: crate::tool::OutputTruncation::empty(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            })
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
                id: "item-1".to_string(),
                call_id: Some("call-1".to_string()),
                name: "spawn_agent".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: retry locally".to_string(),
                display_result: "recoverableSubagentProvider429: retry locally".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            },
            ToolExecutionRecord {
                id: "item-2".to_string(),
                call_id: Some("call-2".to_string()),
                name: "bash".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: unrelated text".to_string(),
                display_result: "recoverableSubagentProvider429: unrelated text".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
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
        assert!(tool_allowed_in_mode(plan, "lsp_query_rust"));
        assert!(tool_allowed_in_mode(plan, "mcp__github__search_issues"));
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
        assert!(options.interaction_callback.is_none());
    }

    #[tokio::test]
    async fn manual_tool_approval_can_approve_through_interaction() {
        let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen_interaction_for_callback = seen_interaction.clone();
        let options = TurnOptions::new(ToolApprovalPolicy::Manual).with_interaction_callback(
            std::sync::Arc::new(move |interaction| {
                let seen_interaction = seen_interaction_for_callback.clone();
                Box::pin(async move {
                    assert_eq!(interaction.kind, pl_protocol::InteractionKind::ToolApproval);
                    match &interaction.payload {
                        InteractionPayload::ToolApproval { name, .. } => assert_eq!(name, "bash"),
                        other => panic!("unexpected payload: {other:?}"),
                    }
                    *seen_interaction.lock().unwrap() = Some(interaction);
                    InteractionResolution::ToolApproval {
                        decision: ToolApprovalResolution::Approved,
                        reason: None,
                    }
                })
            }),
        );
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, &context).await;

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(event_rx.try_recv().is_err());
        let interaction = seen_interaction.lock().unwrap().clone().unwrap();
        assert_eq!(interaction.interaction_id, "call-1");
        assert_eq!(interaction.status, pl_protocol::InteractionStatus::Pending);
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

        let decision = approve_tool_call(&options, &request, &context).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "manual approval required but no interaction runtime is configured"
                    .to_string()
            }
        );
        assert!(event_rx.try_recv().is_err());
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

        let decision = approve_tool_call(&options, &request, &context).await;

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

        let decision = approve_tool_call(&options, &request, &context).await;

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
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();

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
        let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen_interaction_for_callback = seen_interaction.clone();
        let options = TurnOptions::default().with_interaction_callback(std::sync::Arc::new(
            move |interaction| {
                let seen_interaction = seen_interaction_for_callback.clone();
                Box::pin(async move {
                    match &interaction.payload {
                        InteractionPayload::ToolApproval { name, .. } => {
                            assert_eq!(name, "read_file")
                        }
                        other => panic!("unexpected payload: {other:?}"),
                    }
                    *seen_interaction.lock().unwrap() = Some(interaction);
                    InteractionResolution::ToolApproval {
                        decision: ToolApprovalResolution::Approved,
                        reason: None,
                    }
                })
            },
        ));
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
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Completed);
        assert!(records[0].result.contains("external ok"));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { .. }
        ));
        assert!(seen_interaction.lock().unwrap().is_some());
        while event_rx.try_recv().is_ok() {}
        let events = recorder.drain();
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert!(!events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.kind == pl_protocol::TimelineItemKind::Tool
                    && item.status == TimelineItemStatus::Approved
        )));
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
        let _ = tokio::fs::remove_dir_all(outside_root).await;
    }

    #[tokio::test]
    async fn unknown_tool_records_one_terminal_event_and_tool_result() {
        let core = PureCore::default_provider().unwrap();
        let tool_call = ToolCall::function(
            "provider-item-1",
            "missing_tool",
            serde_json::json!({"value": 1}),
            Some("call-1".to_string()),
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &TurnOptions::default(),
                mode: crate::turn::CompileMode::Auto,
                session_id: "turn-1",
                workspace_root: &std::env::temp_dir(),
                workspace_instructions: None,
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Failed);
        assert_eq!(records[0].id, "provider-item-1");
        assert_eq!(records[0].call_id.as_deref(), Some("call-1"));
        assert!(records[0].result.contains("Unknown tool: missing_tool"));
        assert_eq!(terminal_tool_event_count(&events), 1);
    }

    #[tokio::test]
    async fn plan_disabled_tool_records_one_terminal_event_and_tool_result() {
        let mut core = PureCore::default_provider().unwrap();
        core.register_tool(WriteFileTool);
        let tool_call = ToolCall::function(
            "provider-item-1",
            "write_file",
            serde_json::json!({"path": "note.txt", "content": "nope"}),
            Some("call-1".to_string()),
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &TurnOptions::default(),
                mode: crate::turn::CompileMode::Plan,
                session_id: "turn-1",
                workspace_root: &std::env::temp_dir(),
                workspace_instructions: None,
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Denied);
        assert!(
            records[0]
                .result
                .contains("Tool disabled in plan mode: write_file")
        );
        assert_eq!(terminal_tool_event_count(&events), 1);
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemCompleted { item } => Some(item),
                TraceEventKind::TimelineItemFailed { item, .. } => Some(item),
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("terminal tool item");
        assert_eq!(
            terminal
                .tool
                .as_ref()
                .and_then(|tool| tool.denial_reason.as_deref()),
            Some("Tool disabled in plan mode: write_file")
        );
    }

    #[tokio::test]
    async fn policy_denied_tool_records_one_terminal_event_and_tool_result() {
        let mut core = PureCore::default_provider().unwrap();
        core.register_tool(ReadFileTool::new());
        let tool_call = ToolCall::function(
            "provider-item-1",
            "read_file",
            serde_json::json!({"path": "note.txt"}),
            Some("call-1".to_string()),
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &TurnOptions::deny_all(),
                mode: crate::turn::CompileMode::Auto,
                session_id: "turn-1",
                workspace_root: &std::env::temp_dir(),
                workspace_instructions: None,
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Denied);
        assert!(
            records[0]
                .result
                .contains("Tool execution denied: tool execution denied by policy")
        );
        assert_eq!(terminal_tool_event_count(&events), 1);
    }

    #[tokio::test]
    async fn cancelling_running_tool_records_interrupted_terminal_event() {
        let mut core = PureCore::default_provider().unwrap();
        core.register_tool(SleepingTool);
        let tool_call = ToolCall::function(
            "provider-item-1",
            "sleeping_tool",
            serde_json::json!({}),
            Some("call-1".to_string()),
        );
        let token = tokio_util::sync::CancellationToken::new();
        let options = TurnOptions::default().with_cancellation(token.clone());
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            token.cancel();
        });

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &options,
                mode: crate::turn::CompileMode::Auto,
                session_id: "turn-1",
                workspace_root: &std::env::temp_dir(),
                workspace_instructions: None,
                instruction_snapshot: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
                parent_session: std::sync::Arc::new(CoreSession::new()),
            },
        )
        .await
        .unwrap();
        cancel_task.await.unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Interrupted);
        assert_eq!(records[0].result, "Tool execution interrupted");
        assert_eq!(terminal_tool_event_count(&events), 1);
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemFailed { item, .. } => Some(item),
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("interrupted tool item");
        assert_eq!(terminal.status, TimelineItemStatus::Interrupted);
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

        let decision = approve_tool_call(&options, &request, &context).await;

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

    #[tokio::test]
    async fn default_tools_register_bash_and_agent_tools() {
        let mut core = PureCore::default_provider().unwrap();

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await;

        assert!(core.tools.get("bash").is_some());
        assert!(core.tools.get("write_stdin").is_some());
        assert!(core.tools.get("spawn_agent").is_some());
        assert!(core.tools.get("wait_agent").is_some());
        assert!(core.tools.get("list_agents").is_some());
        assert!(core.tools.get("request_user_input").is_some());
        assert!(core.tools.get("subagent").is_none());
        assert!(core.tools.get("read_file").is_some());
        assert!(core.tools.get("apply_patch").is_some());
        assert!(core.tools.get("lsp_query").is_none());
    }

    #[tokio::test]
    async fn default_tools_register_lsp_query_when_runtime_is_shared() {
        let registry = pl_lsp::LspRuntimeRegistry::new();
        let mut core = PureCore::default_provider()
            .unwrap()
            .with_lsp_runtime(registry.clone());

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await;

        // 空注册表没有可用语言，不应注册任何 LSP 工具。
        assert!(core.tools.get("lsp_query_rust").is_none());
        assert!(
            core.tools
                .names()
                .iter()
                .all(|name| !name.starts_with("lsp_query_"))
        );
    }

    #[tokio::test]
    async fn enabled_tools_snapshot_records_mode_filtered_tools() {
        let mut core = PureCore::default_provider().unwrap();
        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await;

        let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Plan);
        let event = enabled_tools_event(&events);

        assert_eq!(event.turn_id, "turn-1");
        assert_eq!(event.mode, "plan");
        assert!(event.tools.contains(&"bash".to_string()));
        assert!(event.tools.contains(&"read_file".to_string()));
        assert!(!event.tools.contains(&"write_file".to_string()));
        assert!(!event.tools.contains(&"apply_patch".to_string()));
    }

    #[tokio::test]
    async fn enabled_tools_snapshot_includes_lsp_query_when_runtime_is_shared() {
        let registry = pl_lsp::LspRuntimeRegistry::new();
        let mut core = PureCore::default_provider()
            .unwrap()
            .with_lsp_runtime(registry);
        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await;

        let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
        let event = enabled_tools_event(&events);

        // 空注册表没有可用语言，不应出现任何 LSP 工具。
        assert!(event.tools.iter().all(|t| !t.starts_with("lsp_query_")));
    }

    #[tokio::test]
    async fn run_turn_records_user_timeline_before_internal_items() {
        let sse_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let mut provider = ProviderInfo::openai(Some(base_url));
        provider.bearer_token = Some("test-token".to_string());
        provider.default_model = "local-responses".to_string();
        let core = PureCore::from_provider_info(provider).unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut session = CoreSession::new();

        let result = core
            .run_turn_with_trace(
                &mut session,
                TurnRequest {
                    prompt: "Build the thing".to_string(),
                    mode: CompileMode::Auto,
                    budget: crate::turn::TurnBudget::new(60_000),
                    instruction_snapshot: None,
                    workspace_instructions: None,
                },
                &mut recorder,
                TurnOptions::default(),
            )
            .await
            .unwrap();
        handle.await.unwrap();

        assert_eq!(result.status, TurnResultStatus::Completed);
        let events = &result.timeline_events;
        let started_kinds = timeline_started_kinds(&events);
        assert_eq!(started_kinds[0], TimelineItemKind::Text);
        assert_eq!(started_kinds[1], TimelineItemKind::Turn);
        assert_eq!(started_kinds[2], TimelineItemKind::Inference);

        let user_item = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemStarted { item }
                    if item.kind == TimelineItemKind::Text
                        && item.role == Some(TimelineTextRole::User) =>
                {
                    Some(item)
                }
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::TimelineItemFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("user timeline item");
        assert_eq!(user_item.sequence, 0);
        assert_eq!(user_item.content, "Build the thing");
    }

    #[tokio::test]
    async fn enabled_tools_snapshot_persists_to_sqlite_timeline() {
        let mut core = PureCore::default_provider().unwrap();
        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await;
        let store = crate::StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(std::env::temp_dir()).await.unwrap();
        let session = store
            .create_session(&project.id, "Tool log", CompileMode::Auto)
            .await
            .unwrap();
        let events = record_enabled_tools_for_core(&core, &session.id, "turn-1", CompileMode::Auto);

        store.append_timeline_events(&events).await.unwrap();
        let records = store
            .load_timeline_events(&session.id, None, None)
            .await
            .unwrap();
        let kind: TraceEventKind = serde_json::from_str(&records[0].payload_json).unwrap();

        assert_eq!(records[0].kind, "EnabledToolsRecorded");
        let TraceEventKind::EnabledToolsRecorded { event } = kind else {
            panic!("expected enabled tools event");
        };
        assert_eq!(event.turn_id, "turn-1");
        assert!(event.tools.contains(&"read_file".to_string()));
    }
}
