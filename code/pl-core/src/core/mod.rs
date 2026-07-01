use std::collections::HashMap;
use std::path::PathBuf;

use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
#[cfg(test)]
use pl_protocol::{ErrorSeverity, PureError};
use pl_protocol::{Message, MessageContent, MessageRole, Result};
use pl_trace::AgentEventSender;
#[cfg(test)]
use pl_trace::{AgentEvent, TraceEvent, TracePartStatus};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::permission::parse_reviewer_decision;
use crate::session::CoreSession;
#[cfg(test)]
use crate::tool::WorkspaceAccess;
use crate::tool::{
    ApplyPatchTool, AskUserTool, CloseAgentTool, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    FollowupTaskTool, ListAgentsTool, ListFilesTool, MovePathTool, PlanExitTool, ReadFileTool,
    SearchFilesTool, SendMessageTool, SkillManageTool, SkillViewTool, SkillsListTool,
    SpawnAgentTool, StatPathTool, SubagentContext, ToolContext, ToolRegistry, WaitAgentTool,
    WriteFileTool, command_tool_pair,
};
use crate::trace::TraceRecorder;
#[cfg(test)]
use crate::turn::{BudgetTracker, TurnResultStatus};
use crate::turn::{
    ToolApprovalDecision, ToolApprovalRequest, TurnOptions, TurnRequest, TurnResult,
};

mod permission;
pub(crate) mod progress;
mod tool_dispatch;
mod turn_loop;
mod turn_result;

pub(crate) use turn_result::compact_text;
/// 生成唯一的 turn ID（毫秒时间戳 + 序列号），用于隔离每个 turn 的 trace part id。
fn generate_turn_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:x}")
}

const SUBAGENT_DISPATCH_CONSTRAINT: &str = "\n\n# 子代理调度约束\n用户明确要求使用 subagent/子代理分工时，必须先调度 `spawn_agent` 工具；不要只用 `bash` 或文件工具替代。若尚未知道 crate 列表，可以先用只读工具定位 workspace，再为每个 crate 创建 explorer agent，最后由父会话汇总。如果子代理创建返回结构化容量错误，表示 provider 并发/容量或 agent 数量限制导致子代理不可用；此时停止继续创建或重试子代理，由当前父 agent 自己完成剩余工作。";

const SUBAGENT_FORCE_DISPATCH_INSTRUCTION: &str = "# 当前轮强制要求\n前面已进行了必要定位但尚未创建 agent。本轮必须调用 `spawn_agent`，不要继续调用文件、shell 或搜索工具，也不要输出最终回答。若子代理创建返回结构化容量错误，后续不再重试创建子代理，改由当前 agent 自己完成任务。";

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
    agent_supervisor: crate::AgentSupervisor,
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
            agent_supervisor: crate::AgentSupervisor::default(),
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
            agent_supervisor: crate::AgentSupervisor::default(),
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
            agent_supervisor: crate::AgentSupervisor::default(),
            tools: ToolRegistry::new(),
        })
    }

    pub(crate) fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub(crate) fn with_agent_supervisor(
        mut self,
        agent_supervisor: crate::AgentSupervisor,
    ) -> Self {
        self.agent_supervisor = agent_supervisor;
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
        self.register_tool(PlanExitTool);
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
            trace: None,
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
use pl_model::TokenUsage;
#[cfg(test)]
use tool_dispatch::{ToolExecutionContext, execute_tool_calls, namespaced_tool_trace_part_id};
#[cfg(test)]
use turn_result::{
    failed_turn_result, looks_like_unexecuted_tool_call_text, normalize_provider_error,
    prompt_requires_subagent_dispatch, provider_error_severity, tool_allowed_in_mode,
};

#[cfg(test)]
mod tests;
