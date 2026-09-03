#[cfg(test)]
use pl_protocol::ErrorSeverity;
use pl_protocol::{Message, MessageContent, MessageRole, PureError, Result};
#[cfg(test)]
use pl_trace::{AgentEvent, TraceEvent};
use std::collections::HashMap;

use crate::config::{ReasoningEffort, SkillsConfig, ToolCapabilityConfig};
use crate::context_compaction::{
    CompactionOutcome, CompactionTrigger, ContextCompactionConfig, ContextCompactionControl,
    ContextCompactionPhase, ContextCompactionRequest, ContextCompactionSnapshot,
    ContextCompactionTrigger, ManualContextCompactionRequest, maybe_compact_session,
};
use crate::execution_environment::ExecutionEnvironment;
use crate::instruction::{InstructionAssembler, InstructionAssemblyRequest};
use crate::permission::parse_reviewer_decision;
use crate::session::AgentSession;
use crate::tool::{
    AgentToolSet, BeforeModelStepHook, ExecutionBackend, GitCredentialProvider, GitTool,
    GitToolKind, GitWorkspaceConfig, SkillManageTool, SkillToolMode, SkillViewTool, SkillsListTool,
    SubagentContext, ToolGroupId, ToolInstallGroup, ToolPlan, WorkspaceAccess,
};
#[cfg(test)]
use crate::tool::{LocalWorkspaceFileTool, WorkspaceFileToolKind, WriteFileTool};
use crate::trace::TraceRecorder;
#[cfg(test)]
use crate::turn::BudgetTracker;
use crate::turn::{
    ToolApprovalDecision, ToolApprovalRequest, TurnOptions, TurnRequest, TurnResult,
};
use pl_model::completion::{CompletionRequest, ReasoningConfig, ReasoningSummary};
use pl_model::runtime::{ModelInvocationContext, ModelRuntime};
use progress::{ProgressEmitter, ProgressVerbosity};

mod model_turn;
mod permission;
mod profile;
pub(crate) mod progress;
mod tool_dispatch;
mod tool_set;
mod turn_loop;
mod turn_result;

pub use model_turn::*;
pub use profile::*;
pub use tool_set::*;
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
    runtime: ModelRuntime,
    effort: Option<ReasoningEffort>,
    skills: Option<SkillsConfig>,
    skill_catalog: Option<std::sync::Arc<crate::skill::FrozenSkillCatalog>>,
    lsp_runtime: Option<pl_lsp::runtime::LspRuntimeRegistry>,
    workspace: Option<crate::tool::AgentWorkspace>,
    workspace_instructions: Option<String>,
    instruction_profile: Option<crate::instruction::InstructionProfile>,
    tool_profile: ToolProfile,
    tool_capabilities: ToolCapabilityConfig,
    default_turn_options: TurnOptions,
    context_compaction: ContextCompactionConfig,
    attachment_runtime: Option<crate::AttachmentRuntime>,
    active_subagent: Option<SubagentContext>,
    /// 此 agent 持久拥有的工具集合；每个模型 step 都从它冻结新 plan。
    agent_tools: AgentToolSet,
    /// 每个模型 step 冻结前由宿主更新工具集合的窗口。
    before_model_step: Option<BeforeModelStepHook>,
    /// 仅由需要 session/working-set 的具体工具捕获。
    tool_session_runtime: crate::tool::ToolSessionRuntime,
    execution_environment: ExecutionEnvironment,
}

impl TurnEngine {
    pub fn execution_environment(&self) -> &ExecutionEnvironment {
        &self.execution_environment
    }
}

impl TurnEngine {
    pub fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::runtime::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    pub async fn install_profile_tools(&mut self) -> Result<()> {
        match self.tool_profile {
            ToolProfile::LocalWorkspace => {
                let workspace = self.workspace.clone().unwrap_or_else(|| {
                    crate::tool::AgentWorkspace::local(
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                    )
                });
                self.install_agent_workspace_tools(
                    workspace,
                    self.workspace_instructions.clone(),
                    self.tool_capabilities.clone(),
                )
                .await?;
            }
            ToolProfile::Minimal => {}
        }
        Ok(())
    }

    /// 测试专用：向 host 来源追加单个工具，等价于旧 `register_tool`。
    #[cfg(test)]
    pub(crate) fn register_test_tool(&mut self, tool: impl Into<crate::tool::DynTool>) {
        let tool = tool.into();
        let name = tool.definition().name().wire_name().to_string();
        let _ = self.agent_tools.install(ToolInstallGroup::direct(
            ToolGroupId::new(format!("test:{name}")),
            vec![tool],
        ));
    }

    /// 冻结一次模型 step 的不可变工具 plan。
    pub(crate) fn acquire_tool_plan(&self) -> ToolPlan {
        self.agent_tools.freeze()
    }

    pub(crate) fn acquire_tool_plan_for(
        &self,
        discovery: &pl_protocol::ToolDiscoveryState,
    ) -> ToolPlan {
        self.agent_tools.freeze_with_discovery(discovery)
    }

    /// 返回当前 Turn 可见的工具名（本地与共享注册表合并）。
    pub fn tool_names(&self) -> Vec<String> {
        self.agent_tools.tool_names()
    }

    /// 返回该引擎绑定的持久 per-agent 工具集合。
    pub fn agent_tools(&self) -> &AgentToolSet {
        &self.agent_tools
    }

    /// 返回供 session-aware 工具在构造时捕获的 per-agent runtime。
    pub fn tool_session_runtime(&self) -> crate::tool::ToolSessionRuntime {
        self.tool_session_runtime.clone()
    }

    /// 返回供 workspace-aware 工具在构造时捕获的 agent workspace runtime。
    pub fn tool_workspace(&self) -> crate::tool::ToolWorkspace {
        let workspace = self.workspace.clone().unwrap_or_else(|| {
            crate::tool::AgentWorkspace::local(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )
        });
        crate::tool::ToolWorkspace::new(workspace).with_lsp_runtime(self.lsp_runtime.clone())
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 shell、异步 agent 协作工具和 workspace 文件工具。调用方应通过 `TurnOptions` 控制审批策略。
    pub async fn install_default_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) -> Result<()> {
        self.tool_profile = ToolProfile::LocalWorkspace;
        self.install_tools_with_capabilities(
            workspace_root,
            workspace_instructions,
            self.tool_capabilities.clone(),
        )
        .await
    }

    /// 按显式 capability 注册共享工具集合。
    pub async fn install_tools_with_capabilities(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
        capabilities: ToolCapabilityConfig,
    ) -> Result<()> {
        self.install_agent_workspace_tools(
            crate::tool::AgentWorkspace::local(workspace_root),
            workspace_instructions,
            capabilities,
        )
        .await
    }

    pub async fn install_agent_workspace_tools(
        &mut self,
        workspace: crate::tool::AgentWorkspace,
        workspace_instructions: Option<String>,
        capabilities: ToolCapabilityConfig,
    ) -> Result<()> {
        self.tool_capabilities = capabilities.clone();
        BuiltinToolInstaller::from_capabilities(capabilities)
            .install_agent_workspace(self, workspace, workspace_instructions)
            .await?;
        if let Some(attachment_runtime) = self.attachment_runtime.clone()
            && let Some(tool) = crate::tool::ViewImageTool::for_model(
                self.tool_workspace(),
                self.runtime.model(),
                attachment_runtime,
            )
        {
            self.agent_tools.install(ToolInstallGroup::direct(
                ToolGroupId::new("view_image"),
                vec![tool.into()],
            ))?;
        }
        Ok(())
    }

    /// 注册 pl-core 提供的通用 git 工具集合（builtin 来源，git 命名空间）。
    pub fn install_git_tools<B, P>(
        &mut self,
        config: GitWorkspaceConfig,
        backend: std::sync::Arc<B>,
        credential_provider: std::sync::Arc<P>,
    ) -> Result<()>
    where
        B: ExecutionBackend + 'static,
        P: GitCredentialProvider + 'static,
    {
        self.agent_tools.install(ToolInstallGroup::direct(
            ToolGroupId::new("git"),
            git_tools(config, backend, credential_provider),
        ))
    }

    pub fn install_skill_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) -> Result<()> {
        let workspace_root = workspace_root.into();
        self.workspace = Some(crate::tool::AgentWorkspace::local(workspace_root.clone()));
        self.workspace_instructions = workspace_instructions;
        self.register_skill_tools_for_workspace(workspace_root)
    }

    /// 从宿主发现并冻结的目录安装原生 Skill 工具组。
    ///
    /// 该入口同时把目录绑定为本 Turn 的 instruction 事实源；工具组仍由 per-agent
    /// `AgentToolSet` 持有，并在每个模型 step 冻结进唯一 `ToolPlan`。
    pub fn install_skill_tools_from_catalog(
        &mut self,
        catalog: std::sync::Arc<crate::skill::FrozenSkillCatalog>,
        mode: SkillToolMode,
    ) -> Result<()> {
        self.skills
            .get_or_insert_with(crate::config::SkillsConfig::default);
        self.skill_catalog = Some(catalog.clone());
        let workspace = self.workspace.clone().unwrap_or_else(|| {
            crate::tool::AgentWorkspace::local(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )
        });
        let tool_workspace =
            crate::tool::ToolWorkspace::new(workspace).with_lsp_runtime(self.lsp_runtime.clone());
        self.agent_tools.install(ToolInstallGroup::direct(
            ToolGroupId::new("skills"),
            crate::tool::skill_tools_from_catalog(catalog, tool_workspace, mode),
        ))
    }

    fn register_skill_tools_for_workspace(
        &mut self,
        workspace_root: std::path::PathBuf,
    ) -> Result<()> {
        let Some(config) = self.skills.clone() else {
            self.agent_tools.uninstall(&ToolGroupId::new("skills"));
            return Ok(());
        };
        if !config.enabled {
            self.agent_tools.uninstall(&ToolGroupId::new("skills"));
            return Ok(());
        }
        let workspace = self
            .workspace
            .clone()
            .unwrap_or_else(|| crate::tool::AgentWorkspace::local(workspace_root));
        let tool_workspace =
            crate::tool::ToolWorkspace::new(workspace).with_lsp_runtime(self.lsp_runtime.clone());
        let tools = if let Some(catalog) = self.skill_catalog.clone() {
            vec![
                SkillsListTool::from_catalog(catalog.clone(), tool_workspace.clone()).into(),
                SkillViewTool::from_catalog(catalog.clone(), tool_workspace.clone()).into(),
                SkillManageTool::from_catalog(catalog, tool_workspace).into(),
            ]
        } else {
            vec![
                SkillsListTool::new(config.clone(), tool_workspace.clone()).into(),
                SkillViewTool::new(config.clone(), tool_workspace.clone()).into(),
                SkillManageTool::new(config, tool_workspace).into(),
            ]
        };
        self.agent_tools
            .install(ToolInstallGroup::direct(ToolGroupId::new("skills"), tools))
    }

    async fn review_tool_call_with_ai(
        &self,
        request: &ToolApprovalRequest,
        permission_mode: crate::turn::PermissionMode,
        workspace_access: WorkspaceAccess,
        workspace_root: &std::path::Path,
    ) -> ToolApprovalDecision {
        let provider = self.runtime.clone();
        let effort = self.effort.clone();
        let reasoning = effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(ReasoningSummary::Enabled),
        });
        let payload = serde_json::json!({
            "toolName": &request.name,
            "arguments": &request.arguments,
            "workingDirectory": &request.working_directory,
            "parentAgentId": &request.parent_agent_id,
            "permissionMode": permission_mode.label(),
            "workspaceAccess": format!("{workspace_access:?}"),
            "workspaceRoot": workspace_root.display().to_string(),
            "riskSummary": permission::permission_risk_summary(&request.name),
        });
        let message = Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
            ),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        };
        let completion_request = CompletionRequest::builder()
            .instructions(include_str!("../../prompts/permission_review.md"))
            .messages(vec![message])
            .tool_choice("none")
            .temperature(Some(0.0))
            .max_tokens(512)
            .reasoning(reasoning)
            .build();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
        let invocation = ModelInvocationContext::new(Default::default(), event_tx);
        match provider.complete(completion_request, invocation).await {
            Ok(response) => {
                let content = response.content.unwrap_or_default().trim().to_string();
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

    pub async fn run_turn(
        &self,
        session: &mut AgentSession,
        request: TurnRequest,
    ) -> Result<TurnResult> {
        self.run_turn_with_options(session, request, self.default_turn_options.clone())
            .await
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut AgentSession,
        request: TurnRequest,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
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
    ) -> Result<Option<ContextCompactionSnapshot>> {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
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
        self.compact_session_with_trace_control(
            session,
            request,
            recorder,
            ContextCompactionControl::default(),
        )
        .await
    }

    pub(crate) async fn compact_session_with_trace_control(
        &self,
        session: &mut AgentSession,
        request: ManualContextCompactionRequest,
        recorder: &mut TraceRecorder,
        control: ContextCompactionControl,
    ) -> Result<Option<ContextCompactionSnapshot>> {
        let requested_trigger = request.trigger;
        let model_info = self.runtime.model().clone();
        let workspace_root = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().to_path_buf())
            .unwrap_or_else(turn_result::default_workspace_root);
        let snapshot = match request.instruction_snapshot {
            Some(snapshot) => snapshot,
            None => {
                let assembly_request = InstructionAssemblyRequest {
                    instructions: None,
                    skills: self.skills.as_ref(),
                    skill_catalog: self
                        .skill_catalog
                        .as_deref()
                        .map(|catalog| catalog.snapshot()),
                    execution_profile: None,
                    model: &model_info,
                    workspace_root: &workspace_root,
                    current_dir: &workspace_root,
                    workspace_documents: None,
                    workspace_instructions: request.workspace_instructions.as_deref(),
                    subagent_constraint: None,
                    skill_suggestions: None,
                    execution_environment: Some(&self.execution_environment),
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
        let tools = self.acquire_tool_plan().specs().to_vec();
        let capabilities = self.runtime.effective_model_capabilities();
        let parallel_tool_calls = capabilities.supports_parallel_tool_calls();
        let reasoning = self.effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(if effort.is_none() {
                ReasoningSummary::Disabled
            } else {
                ReasoningSummary::Enabled
            }),
        });
        let compaction_trigger = match requested_trigger {
            ContextCompactionTrigger::Manual => CompactionTrigger::Manual,
            ContextCompactionTrigger::WallClockRollover => CompactionTrigger::WallClockRollover,
            ContextCompactionTrigger::EstimatedTokens
            | ContextCompactionTrigger::ProviderPromptTokens => {
                return Err(PureError::ConfigError(
                    "standalone compaction only accepts manual or wall-clock rollover triggers"
                        .to_string(),
                ));
            }
        };
        let turn_id = request.turn_id.unwrap_or_else(generate_turn_id);
        let progress_scope = match requested_trigger {
            ContextCompactionTrigger::Manual => format!("{turn_id}:manual-compaction:progress"),
            ContextCompactionTrigger::WallClockRollover => {
                format!("{turn_id}:rollover-compaction:progress")
            }
            ContextCompactionTrigger::EstimatedTokens
            | ContextCompactionTrigger::ProviderPromptTokens => {
                unreachable!("standalone compaction trigger was validated above")
            }
        };
        let mut progress =
            ProgressEmitter::new_scoped(turn_id, progress_scope, ProgressVerbosity::from_env());
        let outcome = maybe_compact_session(
            session,
            ContextCompactionRequest {
                runtime: &self.runtime,
                config: &self.context_compaction,
                request_instructions: &bundle.instructions,
                request_messages: &bundle.prelude_messages,
                working_context_tail: crate::context_assembler::ContextAssembler::assemble(
                    &bundle.instructions,
                    &bundle.prelude_messages,
                    session.items(),
                    &session.working_context_snapshot(),
                )?
                .working_context_tail,
                tools: &tools,
                parallel_tool_calls,
                reasoning,
                prompt_cache_key: session.prompt_cache_key().map(ToString::to_string),
                trigger: compaction_trigger,
                phase: ContextCompactionPhase::Standalone,
                event_tx: recorder.sender().clone(),
                recorder,
                progress: Some(&mut progress),
                control,
            },
        )
        .await?;
        Ok(match outcome {
            CompactionOutcome::Skipped => None,
            CompactionOutcome::Compacted { snapshot, .. } => Some(snapshot),
        })
    }
}

fn git_tools<B, P>(
    config: GitWorkspaceConfig,
    backend: std::sync::Arc<B>,
    credential_provider: std::sync::Arc<P>,
) -> Vec<crate::tool::DynTool>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    GitToolKind::all()
        .iter()
        .copied()
        .map(|kind| {
            GitTool::new(
                kind,
                config.clone(),
                backend.clone(),
                credential_provider.clone(),
            )
            .into()
        })
        .collect()
}

// Re-export for tests
#[cfg(test)]
use permission::approval_request;
#[cfg(test)]
use pl_protocol::TokenUsage;
#[cfg(test)]
use tool_dispatch::{ToolExecutionContext, execute_tool_call_batch, execute_tool_calls};
#[cfg(test)]
use turn_result::{
    failed_turn_result, looks_like_unexecuted_tool_call_text, normalize_provider_error,
    provider_error_severity,
};

#[cfg(test)]
mod unit_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enabled_tools_snapshot_remains_internal_trace_event() {
        let mut core = test_turn_engine();
        core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await
            .expect("install default tools");
        let tool_plan = core.acquire_tool_plan();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = crate::trace::TraceRecorder::new("session-1".to_string(), event_tx, 0);

        super::turn_loop::enabled_tools::record_enabled_tools(
            &mut recorder,
            "turn-1",
            0,
            &tool_plan,
        );
        let events = recorder.drain();
        let event = events
            .iter()
            .find_map(|event| match &event.kind {
                pl_trace::TraceEventKind::EnabledToolsRecorded { event } => Some(event),
                _ => None,
            })
            .expect("enabled tools event");

        assert_eq!(event.turn_id, "turn-1");
        assert!(event.tools.contains(&"read_file".to_string()));
    }

    fn test_turn_engine() -> TurnEngine {
        TurnEngineBuilder::from_route(&crate::ResolvedModelRoute {
            role: crate::AgentRoleId::new("test").unwrap(),
            provider_id: crate::ProviderId::new("test").unwrap(),
            endpoint: pl_model::provider::ProviderEndpoint::deepseek(None),
            model: pl_model::model::ModelInfo::fallback("deepseek-v4-flash"),
            effort: None,
        })
        .unwrap()
        .build()
    }
}
