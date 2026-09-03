use pl_protocol::{Message, MessageContent, MessageRole, PureError, Result};
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
#[cfg(test)]
use tool_dispatch::{ToolExecutionContext, execute_tool_calls};

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use pretty_assertions::assert_eq;

    use pl_model::completion::ToolCall;
    use pl_protocol::{
        InteractionContent, InteractionResolution, ToolApprovalResolution,
        ToolApprovalResolutionPayload,
    };
    use pl_trace::TraceEventKind;

    use super::test_support::*;
    use super::tool_dispatch::ToolExecutionOutcome;
    use super::*;
    use crate::ToolEffect;
    use crate::turn::PermissionMode;

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

    #[test]
    fn default_turn_options_request_approval_for_workspace_escape() {
        let options = TurnOptions::default();

        assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
        assert!(options.interaction_callback.is_none());
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
        let mut core = test_turn_engine();
        core.register_test_tool(LocalWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            crate::tool::ToolWorkspace::new(crate::tool::AgentWorkspace::local(
                workspace_root.clone(),
            )),
        ));
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({"path": outside_file.to_string_lossy()}),
            "call-1",
        );
        let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen_interaction_for_callback = seen_interaction.clone();
        let options = TurnOptions::default().with_interaction_callback(std::sync::Arc::new(
            move |interaction| {
                let seen_interaction = seen_interaction_for_callback.clone();
                async move {
                    match &interaction.content {
                        InteractionContent::ToolApproval(approval) => {
                            assert_eq!(approval.request().name, "read_file")
                        }
                        other => panic!("unexpected payload: {other:?}"),
                    }
                    *seen_interaction.lock().unwrap() = Some(interaction);
                    InteractionResolution::ToolApproval(ToolApprovalResolutionPayload {
                        decision: ToolApprovalResolution::Approved,
                        reason: None,
                    })
                }
                .boxed()
            },
        ));
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert!(records[0].result.contains("external ok"));
        assert!(seen_interaction.lock().unwrap().is_some());
        assert!(runtime_progress_texts(&mut event_rx).is_empty());
        let events = recorder.drain();
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-call-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::AwaitingApproval,
                TestToolPhase::Approved,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
        let _ = tokio::fs::remove_dir_all(outside_root).await;
    }

    #[tokio::test]
    async fn workspace_tool_without_approval_skips_approved_trace_phase() {
        let workspace = tempfile::tempdir().unwrap();
        let mut core = test_turn_engine();
        core.register_test_tool(WriteFileTool::new(crate::tool::ToolWorkspace::new(
            crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
        )));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "write_file",
            serde_json::json!({
                "path": "note.txt",
                "content": "direct",
                "mode": "create",
            }),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
    }

    #[tokio::test]
    async fn unknown_tool_records_one_terminal_event_and_tool_result() {
        let core = test_turn_engine();
        let tool_call = ToolCall::function(
            "provider-item-1",
            "missing_tool",
            serde_json::json!({"value": 1}),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
        );
        assert_eq!(records[0].id, "provider-item-1");
        assert_eq!(records[0].call_id, "call-1");
        assert!(records[0].result.contains("Unknown tool: missing_tool"));
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![TestToolPhase::Started, TestToolPhase::Failed,]
        );
    }

    #[tokio::test]
    async fn execution_policy_denied_tool_records_one_terminal_event_and_tool_result() {
        let mut core = test_turn_engine();
        let tool_workspace = core.tool_workspace();
        core.register_test_tool(WriteFileTool::new(tool_workspace));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "write_file",
            serde_json::json!({"path": "note.txt", "content": "nope"}),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
        let options =
            TurnOptions::default().with_execution_policy(crate::AgentExecutionPolicy::default());

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Denied);
        assert!(
            records[0]
                .result
                .contains("Tool disabled by execution policy: write_file")
        );
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![TestToolPhase::Started, TestToolPhase::Denied,]
        );
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => Some(item),
                TraceEventKind::TracePartFailed { item } => Some(item),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("terminal tool item");
        assert_eq!(
            terminal.tool().and_then(|tool| match tool.state() {
                pl_trace::TraceToolState::Denied(state) => Some(state.reason()),
                pl_trace::TraceToolState::Started(_)
                | pl_trace::TraceToolState::Streaming(_)
                | pl_trace::TraceToolState::AwaitingApproval(_)
                | pl_trace::TraceToolState::Approved(_)
                | pl_trace::TraceToolState::Running(_)
                | pl_trace::TraceToolState::Succeeded(_)
                | pl_trace::TraceToolState::Failed(_)
                | pl_trace::TraceToolState::Cancelled(_) => None,
            }),
            Some("Tool disabled by execution policy: write_file")
        );
    }

    #[tokio::test]
    async fn cancelling_running_tool_records_interrupted_terminal_event() {
        let mut core = test_turn_engine();
        core.register_test_tool(SleepingTool);
        let tool_call = ToolCall::function(
            "provider-item-1",
            "sleeping_tool",
            serde_json::json!({}),
            "call-1",
        );
        let token = tokio_util::sync::CancellationToken::new();
        let options = TurnOptions::default().with_cancellation(token.clone());
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
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
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        cancel_task.await.unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Cancelled);
        assert_eq!(records[0].result, "Tool execution interrupted");
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Cancelled,
            ]
        );
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartFailed { item } => Some(item),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("interrupted tool item");
        assert!(matches!(
            terminal.tool().map(pl_trace::TraceToolPart::state),
            Some(pl_trace::TraceToolState::Cancelled(_)),
        ));
    }

    #[test]
    fn approval_request_extracts_working_directory() {
        let call = ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({
                "command": "pwd",
                "cwd": "C:/work"
            }),
            "call-1",
        );

        let request = approval_request(&call, None);

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn approval_request_marks_parent_agent() {
        let call = ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({"command": "pwd"}),
            "call-1",
        );
        let active_subagent = SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            agent_path: None,
            role: "executor".to_string(),
            task: "inspect".to_string(),
            depth: 1,
        };

        let request = approval_request(&call, Some(&active_subagent));

        assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
    }

    fn has_tool(core: &TurnEngine, name: &str) -> bool {
        core.tool_names().iter().any(|tool| tool == name)
    }

    #[test]
    fn session_note_tools_declare_read_effect_for_plan_policy() {
        use crate::tool::{SessionNoteTool, SessionNoteToolKind, StaticTool};
        for kind in SessionNoteToolKind::all() {
            assert_eq!(
                SessionNoteTool::new(*kind, crate::TurnWorkingSetHandle::default())
                    .policy()
                    .effect(),
                Some(ToolEffect::Read),
                "{}",
                kind.name()
            );
        }
    }

    #[tokio::test]
    async fn default_tools_register_shared_tools_without_product_collaboration() {
        let mut core = test_turn_engine();

        core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await
            .expect("install default tools");

        assert!(has_tool(&core, "exec"));
        assert!(has_tool(&core, "write_stdin"));
        assert!(!has_tool(&core, "spawn_agent"));
        assert!(!has_tool(&core, "list_agents"));
        assert!(!has_tool(&core, "send_input"));
        assert!(!has_tool(&core, "close_agent"));
        assert!(has_tool(&core, "request_user_input"));
        for name in [
            "plan_current",
            "plan_next",
            "plan_history",
            "plan_submit",
            "plan_restart",
        ] {
            assert!(has_tool(&core, name), "missing Plan tool {name}");
        }
        assert!(!has_tool(&core, "submit_plan"));
        assert!(has_tool(&core, "update_todo_list"));
        assert!(has_tool(&core, "read_session_note"));
        assert!(has_tool(&core, "search_session_note"));
        assert!(has_tool(&core, "write_session_note"));
        assert!(has_tool(&core, "apply_session_note_patch"));
        assert!(!has_tool(&core, "plan_exit"));
        assert!(!has_tool(&core, "send_message"));
        assert!(!has_tool(&core, "followup_task"));
        assert!(!has_tool(&core, "subagent"));
        assert!(has_tool(&core, "read_file"));
        assert!(has_tool(&core, "apply_patch"));
        assert!(!has_tool(&core, "lsp_query"));
        assert!(!has_tool(&core, "git_status"));
        assert!(!has_tool(&core, "git_push"));
        assert!(!has_tool(&core, "docker"));
        assert!(!has_tool(&core, "container"));
    }

    #[tokio::test]
    async fn default_tool_builder_exposes_only_framework_independent_names() {
        let mut core = test_turn_engine();
        let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
        let workspace_root = std::env::temp_dir();

        BuiltinToolInstaller::host_provided(capabilities)
            .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
                workspace_root.clone(),
            )))
            .with_workspace_file_backend(std::sync::Arc::new(
                crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                    FakeContainerBackend,
                )),
            ))
            .with_git_tools(
                crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
                std::sync::Arc::new(crate::tool::LocalExecutionBackend),
                std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
            )
            .install(&mut core, workspace_root, None)
            .await
            .expect("install host tools");

        let names = core.tool_names();
        for canonical in [
            "git_workspace_info",
            "exec",
            "write_stdin",
            "read_file",
            "list_files",
            "apply_patch",
            "request_user_input",
            "plan_current",
            "plan_next",
            "plan_history",
            "plan_submit",
            "plan_restart",
            "update_todo_list",
            "read_session_note",
            "search_session_note",
            "write_session_note",
            "apply_session_note_patch",
        ] {
            assert!(
                names.contains(&canonical.to_string()),
                "missing canonical tool `{canonical}` in {names:?}"
            );
        }
    }

    #[test]
    fn workspace_file_tool_kind_rejects_dot_aliases() {
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("read_file"),
            Some(crate::tool::WorkspaceFileToolKind::ReadFile)
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("list_files"),
            Some(crate::tool::WorkspaceFileToolKind::ListFiles)
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("search_files"),
            None
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("apply_patch"),
            Some(crate::tool::WorkspaceFileToolKind::ApplyPatch)
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("read.file"),
            None
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("list.files"),
            None
        );
        assert_eq!(
            crate::tool::WorkspaceFileToolKind::from_name("apply.patch"),
            None
        );
    }

    #[tokio::test]
    async fn builtin_tool_installer_can_disable_exec() {
        let mut core = test_turn_engine();
        let capabilities = crate::config::ToolCapabilityConfig {
            exec: false,
            ..Default::default()
        };

        core.install_tools_with_capabilities(std::env::temp_dir(), None, capabilities)
            .await
            .expect("install selected tools");

        assert!(!has_tool(&core, "exec"));
        assert!(!has_tool(&core, "write_stdin"));
        assert!(!has_tool(&core, "spawn_agent"));
        assert!(has_tool(&core, "read_file"));
        assert!(has_tool(&core, "request_user_input"));
        assert!(has_tool(&core, "plan_current"));
        assert!(has_tool(&core, "plan_submit"));
        assert!(has_tool(&core, "plan_restart"));
        assert!(!has_tool(&core, "submit_plan"));
        assert!(!has_tool(&core, "plan_exit"));
    }

    #[test]
    fn register_git_tools_exposes_git_pack_explicitly() {
        let mut core = test_turn_engine();

        core.install_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .expect("install git tools");

        assert!(has_tool(&core, "git_status"));
        assert!(has_tool(&core, "git_diff"));
        assert!(has_tool(&core, "git_branch"));
        assert!(has_tool(&core, "git_fetch"));
        assert!(has_tool(&core, "git_commit"));
        assert!(has_tool(&core, "git_push"));
        assert!(has_tool(&core, "git_workspace_info"));
        assert!(has_tool(&core, "git_sync_default_branch"));
    }

    #[tokio::test]
    async fn builtin_tool_installer_registers_git_only_with_runtime_config() {
        let capabilities = crate::config::ToolCapabilityConfig {
            git: true,
            ..Default::default()
        };
        let mut core = test_turn_engine();

        BuiltinToolInstaller::from_capabilities(capabilities.clone())
            .install(&mut core, std::env::temp_dir(), None)
            .await
            .expect("install without git runtime");

        assert!(!has_tool(&core, "git_status"));

        let mut core = test_turn_engine();
        BuiltinToolInstaller::from_capabilities(capabilities)
            .with_git_tools(
                crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
                std::sync::Arc::new(crate::tool::LocalExecutionBackend),
                std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
            )
            .install(&mut core, std::env::temp_dir(), None)
            .await
            .expect("install with git runtime");

        assert!(has_tool(&core, "git_status"));
        assert!(has_tool(&core, "git_push"));
    }

    #[derive(Debug, Clone, Default)]
    struct FakeContainerBackend;

    impl crate::tool::ContainerBackend for FakeContainerBackend {
        type Error = String;

        async fn exec(
            &self,
            _request: crate::tool::ContainerExecRequest,
        ) -> std::result::Result<crate::tool::ContainerExecOutput, Self::Error> {
            Ok(crate::tool::ContainerExecOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_bytes: 0,
                stderr_bytes: 0,
                output_artifacts: Vec::new(),
            })
        }

        async fn copy_from(
            &self,
            _request: crate::tool::ContainerCopyFromRequest,
        ) -> std::result::Result<Vec<u8>, Self::Error> {
            Ok(Vec::new())
        }

        async fn copy_to(
            &self,
            _request: crate::tool::ContainerCopyToRequest,
        ) -> std::result::Result<(), Self::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn host_provided_tool_set_requires_explicit_workspace_backends() {
        let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
        let mut core = test_turn_engine();

        BuiltinToolInstaller::host_provided(capabilities.clone())
            .install(&mut core, std::env::temp_dir(), None)
            .await
            .expect("install host tools without backends");

        assert!(!has_tool(&core, "exec"));
        assert!(!has_tool(&core, "write_stdin"));
        assert!(!has_tool(&core, "read_file"));
        assert!(!has_tool(&core, "list_files"));
        assert!(!has_tool(&core, "apply_patch"));

        let mut core = test_turn_engine();
        BuiltinToolInstaller::host_provided(capabilities)
            .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
                std::env::temp_dir(),
            )))
            .with_workspace_file_backend(std::sync::Arc::new(
                crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                    FakeContainerBackend,
                )),
            ))
            .install(&mut core, std::env::temp_dir(), None)
            .await
            .expect("install host tools with backends");

        assert!(has_tool(&core, "exec"));
        assert!(has_tool(&core, "write_stdin"));
        assert!(has_tool(&core, "read_file"));
        assert!(has_tool(&core, "list_files"));
        assert!(!has_tool(&core, "search_files"));
        assert!(has_tool(&core, "apply_patch"));
    }

    #[tokio::test]
    async fn profiled_local_workspace_installs_workspace_tools_in_the_unified_plan() {
        let runtime = CoreRuntimeProfile::local_workspace(std::env::temp_dir())
            .with_workspace_instructions("rules");
        let mut core = test_turn_engine_builder(
            pl_model::provider::ProviderEndpoint::deepseek(None),
            pl_model::model::ModelInfo::fallback("deepseek-v4-flash"),
        )
        .with_runtime_profile(runtime)
        .build();

        core.install_profile_tools()
            .await
            .expect("install profile tools");

        let lease = core.acquire_tool_plan();
        let read_tool = lease.binding("read_file").expect("read_file tool");
        let patch_tool = lease.binding("apply_patch").expect("apply_patch tool");
        assert_eq!(
            read_tool.tool().definition().spec(),
            &crate::tool::WorkspaceFileToolKind::ReadFile.to_spec()
        );
        assert_eq!(
            patch_tool.tool().definition().spec(),
            &crate::tool::WorkspaceFileToolKind::ApplyPatch.to_spec()
        );
        assert_eq!(
            read_tool.tool().execution(),
            crate::tool::ToolExecution::Local
        );
        assert_eq!(
            patch_tool.tool().execution(),
            crate::tool::ToolExecution::Local
        );
    }

    #[tokio::test]
    async fn profiled_host_tools_do_not_register_local_workspace_tools() {
        let runtime = CoreRuntimeProfile::minimal()
            .with_agent_workspace(crate::tool::AgentWorkspace::local(std::env::temp_dir()))
            .with_workspace_instructions("rules");
        let mut core = test_turn_engine_builder(
            pl_model::provider::ProviderEndpoint::deepseek(None),
            pl_model::model::ModelInfo::fallback("deepseek-v4-flash"),
        )
        .with_runtime_profile(runtime)
        .build();

        core.install_profile_tools()
            .await
            .expect("install profile tools");

        assert!(core.tool_names().is_empty());
    }

    #[tokio::test]
    async fn default_tools_register_lsp_query_when_runtime_is_shared() {
        let registry = pl_lsp::runtime::LspRuntimeRegistry::new();
        let mut core = test_turn_engine().with_lsp_runtime(registry.clone());

        core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await
            .expect("install default tools");

        // 空注册表没有可用语言，不应注册任何按语言命名的 LSP 工具。
        assert!(
            core.tool_names()
                .iter()
                .all(|name| !name.starts_with("lsp_query_"))
        );
    }

    #[tokio::test]
    async fn enabled_tools_snapshot_records_registered_tools() {
        let mut core = test_turn_engine();
        core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await
            .expect("install default tools");

        let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
        let event = enabled_tools_event(&events);

        assert_eq!(event.turn_id, "turn-1");
        assert!(event.tools.contains(&"exec".to_string()));
        assert!(event.tools.contains(&"read_file".to_string()));
        assert!(event.tools.contains(&"plan_current".to_string()));
        assert!(event.tools.contains(&"plan_submit".to_string()));
        assert!(!event.tools.contains(&"submit_plan".to_string()));
        assert!(!event.tools.contains(&"plan_exit".to_string()));
        assert!(event.tools.contains(&"write_file".to_string()));
        assert!(event.tools.contains(&"apply_patch".to_string()));
    }
}

/// core 测试共享基建：引擎构造器与 trace 断言 helper。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::tool::{StaticTool, ToolCallContext, ToolPolicy, ToolResult};
    use pl_model::model::ModelInfo;
    use pl_model::provider::ProviderEndpoint;
    use pl_trace::{
        AgentEvent, TraceEvent, TraceEventKind, TracePartKind, TracePartSource, TraceTextChannel,
    };

    pub(crate) fn test_static_tool_definition(
        name: &'static str,
        description: &'static str,
    ) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(crate::tool::ToolName::builtin(name), description)
    }

    pub(crate) fn test_route(
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> crate::ResolvedModelRoute {
        crate::ResolvedModelRoute {
            role: crate::AgentRoleId::new("test").unwrap(),
            provider_id: crate::ProviderId::new("test").unwrap(),
            endpoint,
            model,
            effort: None,
        }
    }

    pub(crate) fn test_turn_engine_builder(
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> TurnEngineBuilder {
        TurnEngineBuilder::from_route(&test_route(endpoint, model)).unwrap()
    }

    pub(crate) fn test_turn_engine() -> TurnEngine {
        test_turn_engine_builder(
            ProviderEndpoint::deepseek(None),
            ModelInfo::fallback("deepseek-v4-flash"),
        )
        .build()
    }

    pub(crate) fn terminal_tool_event_count(events: &[TraceEvent]) -> usize {
        events
            .iter()
            .filter(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => {
                    item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
                }
                TraceEventKind::TracePartFailed { item } => {
                    item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => false,
            })
            .count()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum TestToolPhase {
        Started,
        Streaming,
        AwaitingApproval,
        Approved,
        Running,
        Succeeded,
        Failed,
        Denied,
        Cancelled,
    }

    impl From<&pl_trace::TraceToolState> for TestToolPhase {
        fn from(state: &pl_trace::TraceToolState) -> Self {
            match state {
                pl_trace::TraceToolState::Started(_) => Self::Started,
                pl_trace::TraceToolState::Streaming(_) => Self::Streaming,
                pl_trace::TraceToolState::AwaitingApproval(_) => Self::AwaitingApproval,
                pl_trace::TraceToolState::Approved(_) => Self::Approved,
                pl_trace::TraceToolState::Running(_) => Self::Running,
                pl_trace::TraceToolState::Succeeded(_) => Self::Succeeded,
                pl_trace::TraceToolState::Failed(_) => Self::Failed,
                pl_trace::TraceToolState::Denied(_) => Self::Denied,
                pl_trace::TraceToolState::Cancelled(_) => Self::Cancelled,
            }
        }
    }

    pub(crate) fn tool_statuses(events: &[TraceEvent], item_id: &str) -> Vec<TestToolPhase> {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item }
                    if item.kind() == TracePartKind::Tool && item.item_id() == item_id =>
                {
                    item.tool().map(|tool| TestToolPhase::from(tool.state()))
                }
                TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. }
                | TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. } => None,
            })
            .collect()
    }

    pub(crate) fn live_tool_result_deltas(events: &[AgentEvent], item_id: &str) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::TracePartDelta { event }
                    if event.kind() == TracePartKind::Tool && event.item_id == item_id =>
                {
                    match &event.delta {
                        pl_trace::TraceDelta::ToolResult { delta } => Some(delta.clone()),
                        pl_trace::TraceDelta::Text { .. }
                        | pl_trace::TraceDelta::Thinking { .. }
                        | pl_trace::TraceDelta::ReasoningContent { .. }
                        | pl_trace::TraceDelta::ToolArguments { .. } => None,
                    }
                }
                AgentEvent::TracePartStarted { .. }
                | AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. }
                | AgentEvent::Done => None,
            })
            .collect()
    }

    pub(crate) fn runtime_progress_texts(
        event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
    ) -> Vec<String> {
        let mut progress_texts = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            match event {
                AgentEvent::TracePartCompleted { item }
                    if item.source() == TracePartSource::Runtime
                        && item
                            .text()
                            .is_some_and(|text| text.channel() == TraceTextChannel::Commentary) =>
                {
                    progress_texts.push(
                        item.text()
                            .expect("runtime commentary text")
                            .content()
                            .to_string(),
                    )
                }
                AgentEvent::TracePartStarted { .. }
                | AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. }
                | AgentEvent::Done => {}
            }
        }
        progress_texts
    }

    pub(crate) fn record_enabled_tools_for_core(
        core: &TurnEngine,
        session_id: &str,
        turn_id: &str,
    ) -> Vec<TraceEvent> {
        let tool_plan = core.acquire_tool_plan();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, 0);

        super::turn_loop::enabled_tools::record_enabled_tools(
            &mut recorder,
            turn_id,
            0,
            &tool_plan,
        );

        recorder.drain()
    }

    pub(crate) fn enabled_tools_event(events: &[TraceEvent]) -> &pl_trace::EnabledToolsEvent {
        events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::EnabledToolsRecorded { event } => Some(event),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. } => None,
            })
            .expect("enabled tools event")
    }

    #[derive(Debug)]
    pub(crate) struct SleepingTool;

    impl StaticTool for SleepingTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("sleeping_tool", "Sleeps until the turn is cancelled")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default().with_parallel_tool_calls()
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(ToolResult::success("done"))
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct DeltaEchoTool;

    impl StaticTool for DeltaEchoTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("delta_echo", "Echoes a trace delta before completing")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
        }

        fn execute(
            &self,
            _input: Self::Input,
            context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                let now = crate::time::unix_seconds();
                let event = pl_trace::TracePartDeltaEvent {
                    turn_id: context.identity().turn_id.clone(),
                    item_id: context.identity().item_id.clone(),
                    started_sequence: 0,
                    revision: context.identity().revision_base.saturating_add(1),
                    created_at: now,
                    updated_at: now,
                    delta: pl_trace::TraceDelta::ToolResult {
                        delta: "runtime delta".to_string(),
                    },
                };
                let _ = context.events().send(AgentEvent::TracePartDelta { event });
                Ok(ToolResult::success("delta complete"))
            }
        }
    }
}
