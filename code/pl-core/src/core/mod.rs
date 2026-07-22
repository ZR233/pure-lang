use std::collections::HashMap;
use std::path::PathBuf;

use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider,
};
#[cfg(test)]
use pl_protocol::{ErrorSeverity, PureError};
use pl_protocol::{Message, MessageContent, MessageRole, Result};
use pl_trace::AgentEventSender;
#[cfg(test)]
use pl_trace::{AgentEvent, TraceEvent, TracePartStatus};

use crate::config::{ReasoningEffort, SkillsConfig, ToolCapabilityConfig};
use crate::context_compaction::{
    CompactionOutcome, CompactionTrigger, ContextCompactionConfig, ContextCompactionPhase,
    ContextCompactionRequest, ContextCompactionSnapshot, ManualContextCompactionRequest,
    maybe_compact_session,
};
use crate::instruction::{InstructionAssembler, InstructionAssemblyRequest};
use crate::permission::parse_reviewer_decision;
use crate::session::AgentSession;
use crate::tool::{
    ExecutionBackend, GitCredentialProvider, GitTool, GitToolKind, GitWorkspaceConfig,
    SkillManageTool, SkillViewTool, SkillsListTool, SubagentContext, ToolContext, ToolRegistry,
};
#[cfg(test)]
use crate::tool::{LocalWorkspaceFileTool, WorkspaceAccess, WorkspaceFileToolKind, WriteFileTool};
use crate::trace::TraceRecorder;
#[cfg(test)]
use crate::turn::{BudgetTracker, TurnResultStatus};
use crate::turn::{
    ToolApprovalDecision, ToolApprovalRequest, TurnOptions, TurnRequest, TurnResult,
};
use progress::{ProgressEmitter, ProgressVerbosity};

mod kernel;
mod model_turn;
mod permission;
mod profile;
pub(crate) mod progress;
mod tool_dispatch;
mod tool_set;
mod turn_loop;
mod turn_result;

pub use kernel::{
    AgentKernel, AgentKernelBuilder, AgentKernelToolRequest, AgentKernelToolSet, CoreAgentProfile,
    NoAgentKernelToolSet,
};
pub use model_turn::{
    CoreModelTurnClient, CoreModelTurnOptions, CoreModelTurnRequest,
    stream_history_completion_message_text, stream_session_completion_message_text,
    stream_session_completion_response,
};
pub use profile::{
    CoreRuntimeOptions, CoreRuntimeProfile, ToolProfile, TurnEngineBuilder, WorkspaceProfile,
};
pub use tool_set::{
    SharedToolSchemaOptions, ToolSetBuilder, ToolVisibilitySet, shared_tool_names,
    shared_tool_schemas,
};
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

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider、工具注册表和单轮编译请求。
/// 工具能力由调用方显式注册，并通过 `TurnOptions` 控制审批策略。
#[derive(Debug)]
pub struct TurnEngine {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    skills: Option<SkillsConfig>,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    workspace_root: Option<PathBuf>,
    workspace_instructions: Option<String>,
    instruction_profile: Option<crate::instruction::InstructionProfile>,
    tool_profile: ToolProfile,
    tool_capabilities: ToolCapabilityConfig,
    runtime_options: CoreRuntimeOptions,
    context_compaction: ContextCompactionConfig,
    active_subagent: Option<SubagentContext>,
    tools: ToolRegistry,
}

impl TurnEngine {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            skills: None,
            lsp_runtime: None,
            workspace_root: None,
            workspace_instructions: None,
            instruction_profile: None,
            tool_profile: ToolProfile::Minimal,
            tool_capabilities: ToolCapabilityConfig::default(),
            runtime_options: CoreRuntimeOptions::default(),
            context_compaction: ContextCompactionConfig::default(),
            active_subagent: None,
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
            skills: None,
            lsp_runtime: None,
            workspace_root: None,
            workspace_instructions: None,
            instruction_profile: None,
            tool_profile: ToolProfile::Minimal,
            tool_capabilities: ToolCapabilityConfig::default(),
            runtime_options: CoreRuntimeOptions::default(),
            context_compaction: ContextCompactionConfig::default(),
            active_subagent: None,
            tools: ToolRegistry::new(),
        }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn default_provider() -> Result<Self> {
        Self::from_provider_info(ProviderInfo::default_provider())
    }

    pub fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    pub async fn register_profile_tools(&mut self) {
        match self.tool_profile {
            ToolProfile::LocalWorkspace => {
                let workspace_root = self.workspace_root.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                self.register_default_tools(workspace_root, self.workspace_instructions.clone())
                    .await;
            }
            ToolProfile::HostProvided | ToolProfile::Minimal => {}
        }
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
        self.tool_profile = ToolProfile::LocalWorkspace;
        self.register_tools_with_capabilities(
            workspace_root,
            workspace_instructions,
            self.tool_capabilities.clone(),
        )
        .await;
    }

    /// 按显式 capability 注册共享工具集合。
    pub async fn register_tools_with_capabilities(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
        capabilities: ToolCapabilityConfig,
    ) {
        self.tool_capabilities = capabilities.clone();
        ToolSetBuilder::from_capabilities(capabilities)
            .register(self, workspace_root, workspace_instructions)
            .await;
    }

    /// 注册 pl-core 提供的通用 git 工具集合。
    pub fn register_git_tools<B, P>(
        &mut self,
        config: GitWorkspaceConfig,
        backend: std::sync::Arc<B>,
        credential_provider: std::sync::Arc<P>,
    ) where
        B: ExecutionBackend + 'static,
        P: GitCredentialProvider + 'static,
    {
        for kind in GitToolKind::all() {
            self.register_tool(GitTool::new(
                *kind,
                config.clone(),
                backend.clone(),
                credential_provider.clone(),
            ));
        }
    }

    pub(crate) fn mcp_tools_enabled(&self) -> bool {
        self.tool_capabilities.mcp
    }

    pub fn register_skill_tools(
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
        let Some(config) = self.skills.clone() else {
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
        let provider = self.provider.clone();
        let reasoning_effort = self.reasoning_effort.clone();
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
        let completion_request = CompletionRequest::builder(provider.default_model())
            .instructions(include_str!("../../prompts/permission_review.md"))
            .messages(vec![message])
            .tool_choice("none")
            .temperature(Some(0.0))
            .max_tokens(512)
            .store(Some(false))
            .reasoning(reasoning)
            .stream(false)
            .build();
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
        session: &'a mut AgentSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.run_turn_with_options(
            session,
            request,
            event_tx,
            self.runtime_options.default_turn_options.clone(),
        )
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut AgentSession,
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
        session: &mut AgentSession,
        request: TurnRequest,
        recorder: &mut TraceRecorder,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        turn_loop::run_turn_with_trace(self, session, request, recorder, options).await
    }

    /// 立即压缩当前会话，不检查自动压缩阈值。
    ///
    /// 空历史或只有远程 checkpoint 时返回 `Ok(None)`。成功压缩会原子替换
    /// 会话上下文并重置 Responses continuation。
    ///
    /// # Errors
    ///
    /// instruction 组装失败、provider 压缩失败或远程结果校验失败时返回错误；
    /// 失败不会安装部分压缩历史。
    pub async fn compact_session(
        &self,
        session: &mut AgentSession,
        request: ManualContextCompactionRequest,
        event_tx: AgentEventSender,
    ) -> Result<Option<ContextCompactionSnapshot>> {
        let mut recorder = TraceRecorder::disabled(event_tx);
        self.compact_session_with_trace(session, request, &mut recorder)
            .await
    }

    /// 立即压缩当前会话，并把进展写入给定 trace recorder。
    ///
    /// # Errors
    ///
    /// 与 [`TurnEngine::compact_session`] 相同。
    pub async fn compact_session_with_trace(
        &self,
        session: &mut AgentSession,
        request: ManualContextCompactionRequest,
        recorder: &mut TraceRecorder,
    ) -> Result<Option<ContextCompactionSnapshot>> {
        let model = self.provider.default_model().to_string();
        let model_info = self.provider.model_info(&model);
        let workspace_root = self
            .workspace_root
            .clone()
            .unwrap_or_else(turn_result::default_workspace_root);
        let snapshot = match request.instruction_snapshot {
            Some(snapshot) => snapshot,
            None => {
                let assembly_request = InstructionAssemblyRequest {
                    instructions: None,
                    skills: self.skills.as_ref(),
                    execution_profile: None,
                    model: &model_info,
                    workspace_root: &workspace_root,
                    current_dir: &workspace_root,
                    workspace_instructions: request.workspace_instructions.as_deref(),
                    subagent_constraint: None,
                };
                match self.instruction_profile.as_ref() {
                    Some(profile) => {
                        InstructionAssembler::assemble_with_profile(assembly_request, profile)?
                    }
                    None => InstructionAssembler::assemble(assembly_request)?,
                }
            }
        };
        let bundle = snapshot.to_bundle();
        let tools = request.execution_policy.as_ref().map_or_else(
            || self.tools.schemas(),
            |policy| self.tools.schemas_for_policy(policy),
        );
        let capabilities = self.provider.effective_model_capabilities(&model);
        let parallel_tool_calls = capabilities.supports_parallel_tool_calls();
        let reasoning = self
            .reasoning_effort
            .as_ref()
            .map(|effort| ReasoningConfig {
                effort: Some(effort.as_str().to_string()),
                summary: Some(if effort.is_none() {
                    ReasoningSummary::Disabled
                } else {
                    ReasoningSummary::Enabled
                }),
            });
        let turn_id = request.turn_id.unwrap_or_else(generate_turn_id);
        let mut progress = ProgressEmitter::new(
            recorder.sender().clone(),
            turn_id,
            ProgressVerbosity::from_env(),
        );
        let outcome = maybe_compact_session(
            session,
            ContextCompactionRequest {
                provider: self.provider.as_ref(),
                model: &model,
                config: &self.context_compaction,
                request_instructions: &bundle.instructions,
                request_messages: &bundle.prelude_messages,
                tools: &tools,
                parallel_tool_calls,
                reasoning,
                prompt_cache_key: session.prompt_cache_key().map(ToString::to_string),
                trigger: CompactionTrigger::Manual,
                phase: ContextCompactionPhase::Standalone,
                event_tx: recorder.sender().clone(),
                progress: Some(&mut progress),
            },
        )
        .await?;
        Ok(match outcome {
            CompactionOutcome::Skipped => None,
            CompactionOutcome::Compacted { snapshot, .. } => Some(snapshot),
        })
    }
}

// Re-export for tests
#[cfg(test)]
use permission::{approval_request, approve_tool_call};
#[cfg(test)]
use pl_model::TokenUsage;
#[cfg(test)]
use tool_dispatch::{ToolExecutionContext, execute_tool_calls};
#[cfg(test)]
use turn_result::{
    failed_turn_result, looks_like_unexecuted_tool_call_text, normalize_provider_error,
    provider_error_severity,
};

#[cfg(test)]
mod tests;
