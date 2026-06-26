use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_protocol::{
    ContentPart, ImageSource, InteractionChangedEvent, InteractionKind, InteractionPayload,
    InteractionRequest, InteractionResolution, InteractionScope, InteractionStatus, MessageContent,
    PlanConfirmationResolution, PlanLifecycleEvent, PlanLifecycleState, StudioAgentSnapshot,
    StudioEventKind, StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart,
    StudioPartStatus, StudioPartType, StudioRuntimeUsage, StudioSessionRuntime, StudioTextChannel,
    StudioTurnStatus,
};
use pl_trace::{AgentEvent, TraceEvent, TraceEventKind, TracePart, TracePartKind};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::config::{ConfigStore, ModelRole, PureConfig, ReasoningEffort, RoleConfig};
use crate::mcp::McpRuntimeRegistry;
use crate::skill::SkillCatalog;
use crate::studio::StudioStore;
use crate::studio::active_turns::StudioActiveTurns;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::default_session_runtime_record;
use crate::studio::records::{
    AgentSnapshotRecord, ProjectRecord, SessionRecord, SessionRuntimeRecord, StudioPromptOutcome,
};
use crate::studio::{
    InteractionEmitter, InteractionRuntime, StudioEventRuntime, StudioRuntimeSnapshot,
    StudioRuntimeState, StudioRuntimeStatus, resolution_matches_kind,
};
use crate::{
    CompileMode, CoreSession, InstructionAssembler, InstructionAssemblyRequest,
    InstructionSnapshot, InteractionCallback, PureCore, TraceRecorder, TurnAbortReason, TurnBudget,
    TurnOptions, TurnRequest, TurnResultStatus, load_workspace_instructions,
    resolve_workspace_root,
};

const IMPLEMENT_PLAN_CURRENT_SESSION_PREFIX: &str = "A previous agent produced the plan below to accomplish the user's task. Implement the plan in the current session. Treat the plan as the source of user intent, re-read files as needed, and carry the work through implementation and verification.";

const SELF_LEARNING_REVIEW_PROMPT: &str = r#"你是 Pure-Lang 项目 skills 自学习 reviewer。

请复盘上一轮完整对话和工具结果，只在发现可复用项目经验时更新当前项目 `skills/` 目录。

规则：
- 只能使用 `skills_list`、`skill_view`、`skill_manage`。
- 优先 patch 本轮已经读取过的项目 skill。
- 其次 patch 现有项目 umbrella skill。
- 没有合适项目 skill 时，才 create 一个泛化的项目 skill。
- 不要记录一次性任务、瞬时环境失败、负面工具断言、provider 临时错误或纯用户私密偏好。
- 不要修改用户级或外部只读 skill；如需复用，创建项目级覆盖或项目级新 skill。
- 不要修改系统内置 skill；如需覆盖或沉淀项目经验，创建/更新项目级 skill。
- 如果没有值得沉淀的内容，直接简短说明无需更新，不要调用工具。
"#;

pub struct RunPromptRequest {
    pub session_id: String,
    pub turn_id: String,
    pub prompt: String,
    pub attachment_ids: Vec<String>,
    pub interaction_callback: InteractionCallback,
    pub interaction_emitter: InteractionEmitter,
    pub options: TurnOptions,
}

/// Studio UI 提交 prompt 的请求。
///
/// 这是面向桌面端 runtime 的高层 API，会创建 turn、发出用户消息快照，并在后台
/// 独立运行核心 turn。调用方不需要自己管理 cancellation token。
pub struct StudioSubmitPromptRequest {
    pub session_id: String,
    pub prompt: String,
    pub attachment_ids: Vec<String>,
    pub options: StudioSubmitPromptOptions,
}

/// Studio UI 提交 prompt 的附加选项。
///
/// 选项描述用户消息如何进入 timeline，以及是否把 turn 关联到计划实施生命周期。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioSubmitPromptOptions {
    pub user_prompt: StudioUserPromptPresentation,
    pub lifecycle: Option<StudioPlanImplementationLifecycle>,
}

/// 用户 prompt 在 Studio timeline 中的展示方式。
///
/// 常规用户输入用 `Normal`；runtime 合成的 follow-up 可以选择可见标签，
/// 或标记为 ignored，避免污染长期会话语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StudioUserPromptPresentation {
    #[default]
    Normal,
    SyntheticVisible {
        visible_prompt: String,
    },
    SyntheticIgnored {
        visible_prompt: String,
    },
}

impl StudioUserPromptPresentation {
    fn visible_prompt<'a>(&'a self, prompt: &'a str) -> &'a str {
        match self {
            Self::Normal => prompt,
            Self::SyntheticVisible { visible_prompt }
            | Self::SyntheticIgnored { visible_prompt } => visible_prompt.as_str(),
        }
    }

    fn is_synthetic(&self) -> bool {
        matches!(
            self,
            Self::SyntheticVisible { .. } | Self::SyntheticIgnored { .. }
        )
    }

    fn is_ignored(&self) -> bool {
        matches!(self, Self::SyntheticIgnored { .. })
    }
}

/// 计划实施 turn 的生命周期关联。
///
/// runtime 在实施 turn 完成、失败或中断时，会用此信息补充计划 lifecycle event。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPlanImplementationLifecycle {
    pub session_id: String,
    pub plan_id: String,
}

/// Studio UI 提交 prompt 后立即得到的后台 turn 信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioSubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

/// Studio UI 请求停止当前会话 turn 后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioStopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

/// Studio UI resolve interaction 后的核心响应。
#[derive(Debug, Clone, PartialEq)]
pub struct StudioResolveInteractionResponse {
    pub session_id: String,
    pub interaction: InteractionRequest,
    pub sessions: Vec<SessionRecord>,
}

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
    mcp_runtime: McpRuntimeRegistry,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionRuntime,
    events: StudioEventRuntime,
    runtime_state: StudioRuntimeState,
    active_turns: StudioActiveTurns,
}

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        let store = StudioStore::default_app().await?;
        let runtime = Self::with_runtime_state(
            store,
            ConfigStore::default_app()?,
            StudioRuntimeState::new(),
        );
        let _ = runtime.initialize_runtime().await?;
        Ok(runtime)
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Self {
        Self::with_runtime_state(store, config_store, StudioRuntimeState::ready())
    }

    fn with_runtime_state(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
    ) -> Self {
        Self {
            interactions: InteractionRuntime::new(store.clone()),
            events: StudioEventRuntime::new(store.clone()),
            store,
            config_store,
            mcp_runtime: McpRuntimeRegistry::new(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
            runtime_state: runtime_state.clone(),
            active_turns: StudioActiveTurns::new(runtime_state),
        }
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn interactions(&self) -> &InteractionRuntime {
        &self.interactions
    }

    pub fn events(&self) -> &StudioEventRuntime {
        &self.events
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.config_store
    }

    pub fn mcp_runtime(&self) -> &McpRuntimeRegistry {
        &self.mcp_runtime
    }

    pub fn lsp_runtime(&self) -> &pl_lsp::LspRuntimeRegistry {
        &self.lsp_runtime
    }

    pub fn runtime_snapshot(&self) -> StudioRuntimeSnapshot {
        self.runtime_state.snapshot()
    }

    pub async fn initialize_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        if matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready) {
            return Ok(self.runtime_snapshot());
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::Initializing, None)?;
        let initialization = async {
            let turns = self
                .store
                .cancel_unfinished_turns("application restarted")
                .await?;
            self.cancel_recovered_transient_interactions(turns).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        match initialization {
            Ok(()) => self
                .runtime_state
                .transition(StudioRuntimeStatus::Ready, None),
            Err(error) => {
                let message = format!("{error:#}");
                let _ = self
                    .runtime_state
                    .transition(StudioRuntimeStatus::Failed, Some(message));
                Err(error)
            }
        }
    }

    pub async fn start_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        if !matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready) {
            let _ = self.initialize_runtime().await?;
        }
        if let Err(error) = self.reconcile_mcp_runtime().await {
            let message = format!("{error:#}");
            let _ = self
                .runtime_state
                .transition(StudioRuntimeStatus::Failed, Some(message));
            return Err(error);
        }
        Ok(self.runtime_snapshot())
    }

    pub async fn shutdown_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        let status = self.runtime_snapshot().status;
        if matches!(
            status,
            StudioRuntimeStatus::Stopped | StudioRuntimeStatus::Failed
        ) {
            return Ok(self.runtime_snapshot());
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::ShuttingDown, None)?;
        self.active_turns.cancel_all_and_clear().await;
        self.mcp_runtime.shutdown().await;
        self.lsp_runtime.shutdown().await;
        self.runtime_state
            .transition(StudioRuntimeStatus::Stopped, None)
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_runtime().await;
    }

    pub async fn reconcile_mcp_runtime(&self) -> Result<()> {
        let config = self.config_store.load_or_default()?;
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await;
        Ok(())
    }

    pub async fn recheck_mcp_runtime(&self) -> Result<()> {
        let config = self.config_store.load_or_default()?;
        self.mcp_runtime
            .recheck(crate::config::effective_mcp_servers(&config))
            .await;
        Ok(())
    }

    pub async fn reconcile_lsp_runtime_for_project(&self, project_id: &str) -> Result<()> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        self.lsp_runtime.reconcile_workspace(workspace_root).await;
        Ok(())
    }

    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        self.store.upsert_project(path).await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.store.list_projects().await
    }

    pub async fn ensure_project_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        let mut sessions = self.store.list_sessions(project_id).await?;
        if sessions.is_empty() {
            sessions.push(
                self.store
                    .create_session(project_id, "新会话", CompileMode::Auto)
                    .await?,
            );
        }
        Ok(sessions)
    }

    pub async fn create_session(&self, project_id: &str, title: &str) -> Result<SessionRecord> {
        let session = self
            .store
            .create_session(project_id, title, CompileMode::Auto)
            .await?;
        self.events.emit_session_list(project_id).await?;
        Ok(session)
    }

    pub async fn archive_session(&self, session_id: String) -> Result<Option<SessionRecord>> {
        if self.active_turns.contains(&session_id).await {
            bail!("session has an active turn");
        }
        let emitter = self.interaction_emitter(session_id.clone());
        self.interactions
            .cancel_session(&session_id, "session archived", emitter)
            .await?;
        let archived = self.store.archive_session(&session_id).await?;
        if let Some(session) = &archived {
            self.events.emit_session_list(&session.project_id).await?;
        }
        Ok(archived)
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        let session_ids = self.store.list_project_session_ids(project_id).await?;
        if self.active_turns.contains_any(&session_ids).await {
            bail!("project has an active turn");
        }
        for session_id in session_ids {
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_session(&session_id, "project archived", emitter)
                .await?;
        }
        let archived = self.store.archive_project(project_id).await?;
        if archived.is_some() {
            self.events.emit_session_list(project_id).await?;
        }
        Ok(archived)
    }

    pub async fn set_session_mode(&self, session_id: &str, mode: CompileMode) -> Result<()> {
        self.store.set_session_mode(session_id, mode).await?;
        let Some(session) = self.store.read_session(session_id).await? else {
            return Ok(());
        };
        self.events.emit_session_list(&session.project_id).await?;
        Ok(())
    }

    pub fn set_model_role(
        &self,
        role: ModelRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<PureConfig> {
        let provider_id = provider_id.trim();
        let model_slug = model_slug.trim();
        let mut config = self.config_store.load_or_default()?;
        let resolved_effort = {
            let provider = config.providers.get(provider_id).with_context(|| {
                format!(
                    "role {} references missing provider: {provider_id}",
                    role.key()
                )
            })?;
            let model = provider
                .models
                .iter()
                .find(|model| model.slug == model_slug)
                .with_context(|| {
                    format!(
                        "role {} references missing model: {provider_id}.{model_slug}",
                        role.key()
                    )
                })?;
            match effort.map(str::trim).filter(|value| !value.is_empty()) {
                Some(value) => {
                    if !model
                        .supported_efforts()
                        .iter()
                        .any(|candidate| candidate == value)
                    {
                        bail!(
                            "role {} uses unsupported effort '{}' for model {provider_id}.{model_slug}",
                            role.key(),
                            value
                        );
                    }
                    value.to_string()
                }
                None => model.default_effort().with_context(|| {
                    format!(
                        "role {} model {provider_id}.{model_slug} must define effort",
                        role.key()
                    )
                })?,
            }
        };
        let next_role = RoleConfig {
            provider: provider_id.to_string(),
            model: model_slug.to_string(),
            effort: ReasoningEffort::new(resolved_effort),
        };
        match role {
            ModelRole::Explorer => config.roles.explorer = next_role,
            ModelRole::Planner => config.roles.planner = next_role,
            ModelRole::Executor => config.roles.executor = next_role,
            ModelRole::Reviewer => config.roles.reviewer = next_role,
        }
        config.validate()?;
        self.config_store.save(&config)?;
        Ok(config)
    }

    pub async fn session_runtime(&self, session_id: &str) -> Result<SessionRuntimeRecord> {
        if let Some(snapshot) = self.store.load_session_runtime(session_id).await? {
            return Ok(snapshot);
        }
        let config = self.config_store.load_or_default()?;
        let resolved = config.resolve_role(ModelRole::Planner)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == resolved.role_config.model)
            .or_else(|| resolved.models.first());
        Ok(default_session_runtime_record(session_id, model))
    }

    pub async fn provider_usages(&self) -> Result<Vec<crate::ProviderUsageRecord>> {
        let config = self.config_store.load_or_default()?;
        Ok(crate::provider_usage_records(&config).await)
    }

    pub async fn discovered_skills(&self, project_id: &str) -> Result<SkillCatalog> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let config = self.config_store.load_or_default()?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        Ok(SkillCatalog::discover(&workspace_root, &config.skills)?)
    }

    pub async fn submit_prompt(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        let StudioSubmitPromptRequest {
            session_id,
            prompt,
            attachment_ids,
            options,
        } = request;
        if prompt.trim().is_empty() && attachment_ids.is_empty() {
            bail!("prompt is empty");
        }
        let turn_id = new_id("turn");
        let cancellation_token = CancellationToken::new();
        self.active_turns
            .insert(
                session_id.clone(),
                turn_id.clone(),
                cancellation_token.clone(),
            )
            .await?;
        let submit_result = async {
            self.events
                .emit_turn(&session_id, &turn_id, StudioTurnStatus::Queued, None)
                .await?;
            self.events
                .emit_turn(
                    &session_id,
                    &turn_id,
                    StudioTurnStatus::ContextLoading,
                    None,
                )
                .await?;
            self.emit_user_prompt_snapshots(
                &session_id,
                &turn_id,
                &prompt,
                &attachment_ids,
                &options,
            )
            .await?;
            self.store.next_studio_event_sequence(&session_id).await
        }
        .await;
        let cursor = match submit_result {
            Ok(cursor) => cursor as u64,
            Err(error) => {
                self.active_turns.remove(&session_id).await;
                return Err(error);
            }
        };
        let run_runtime = self.clone();
        let run_session_id = session_id.clone();
        let run_turn_id = turn_id.clone();
        tokio::spawn(async move {
            run_runtime
                .run_prompt_background(
                    run_session_id,
                    run_turn_id,
                    prompt,
                    attachment_ids,
                    cancellation_token,
                    options.lifecycle,
                )
                .await;
        });
        Ok(StudioSubmitPromptResponse {
            session_id,
            turn_id,
            cursor,
        })
    }

    pub async fn stop_prompt(&self, session_id: String) -> Result<StudioStopPromptResponse> {
        let token = self.active_turns.token(&session_id).await;
        let Some(token) = token else {
            return Ok(StudioStopPromptResponse {
                session_id,
                stopped: false,
            });
        };
        token.cancel();
        let emitter = self.interaction_emitter(session_id.clone());
        self.interactions
            .cancel_session(&session_id, "interrupted by user", emitter)
            .await?;
        Ok(StudioStopPromptResponse {
            session_id,
            stopped: true,
        })
    }

    pub async fn resolve_interaction(
        &self,
        interaction_id: String,
        resolution: InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        let current = self
            .store
            .read_interaction(&interaction_id)
            .await?
            .context("interaction not found")?;
        let session_id = current.scope.session_id.clone();
        if !resolution_matches_kind(&current.kind, &resolution) {
            bail!("interaction resolution kind does not match interaction");
        }
        let emitter = self.interaction_emitter(session_id.clone());

        if current.kind == InteractionKind::PlanConfirmation {
            return self
                .resolve_plan_confirmation(interaction_id, current, resolution, emitter)
                .await;
        }

        let resolved = self
            .interactions
            .resolve(&interaction_id, resolution, emitter)
            .await?;
        Ok(StudioResolveInteractionResponse {
            session_id,
            interaction: resolved,
            sessions: Vec::new(),
        })
    }

    pub async fn run_prompt(&self, request: RunPromptRequest) -> Result<StudioPromptOutcome> {
        let mut request = request;
        let session_id = request.session_id.clone();
        let turn_id = request.turn_id.clone();
        let cancellation_token = request
            .options
            .cancellation_token
            .clone()
            .unwrap_or_default();
        if request.options.cancellation_token.is_none() {
            request.options = request
                .options
                .with_cancellation(cancellation_token.clone());
        }
        self.active_turns
            .insert(session_id.clone(), turn_id, cancellation_token)
            .await?;
        let outcome = self.run_prompt_inner(request).await;
        self.active_turns.remove(&session_id).await;
        outcome
    }

    async fn run_prompt_background(
        &self,
        session_id: String,
        turn_id: String,
        prompt: String,
        attachment_ids: Vec<String>,
        cancellation_token: CancellationToken,
        lifecycle: Option<StudioPlanImplementationLifecycle>,
    ) {
        let _ = self
            .events
            .emit_turn(
                &session_id,
                &turn_id,
                StudioTurnStatus::WaitingForModel,
                None,
            )
            .await;
        let emitter = self.interaction_emitter(session_id.clone());
        let interaction_callback = self
            .interactions
            .callback(session_id.clone(), emitter.clone());
        let options = TurnOptions::default()
            .with_cancellation(cancellation_token)
            .with_interaction_callback(interaction_callback.clone());
        let result = self
            .run_prompt_inner(RunPromptRequest {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                prompt,
                attachment_ids,
                interaction_callback,
                interaction_emitter: emitter.clone(),
                options,
            })
            .await;
        self.active_turns.remove(&session_id).await;
        let _ = self
            .interactions
            .cancel_transient_interactions(&session_id, "turn completed", emitter)
            .await;
        match result {
            Ok(outcome) => {
                self.emit_turn_completion(&session_id, &turn_id, &outcome)
                    .await;
                if let Some(lifecycle) = lifecycle {
                    let (state, reason) = match outcome.result.status {
                        TurnResultStatus::Completed => (PlanLifecycleState::Implemented, None),
                        TurnResultStatus::Aborted => (
                            PlanLifecycleState::ImplementationFailed,
                            outcome
                                .result
                                .abort_reason
                                .map(|reason| reason.as_str().to_string())
                                .or_else(|| Some("turn aborted".to_string())),
                        ),
                        TurnResultStatus::Errored => (
                            PlanLifecycleState::ImplementationFailed,
                            outcome
                                .result
                                .error
                                .or_else(|| Some("turn errored".to_string())),
                        ),
                    };
                    let _ = self
                        .append_plan_lifecycle_event(
                            &lifecycle.session_id,
                            &lifecycle.plan_id,
                            state,
                            Some(turn_id),
                            reason,
                        )
                        .await;
                }
            }
            Err(error) => {
                let _ = self
                    .events
                    .emit_turn(
                        &session_id,
                        &turn_id,
                        StudioTurnStatus::Failed,
                        Some(format!("{error:#}")),
                    )
                    .await;
            }
        }
    }

    async fn resolve_plan_confirmation(
        &self,
        interaction_id: String,
        current: InteractionRequest,
        resolution: InteractionResolution,
        emitter: InteractionEmitter,
    ) -> Result<StudioResolveInteractionResponse> {
        let session_id = current.scope.session_id.clone();
        let InteractionPayload::PlanConfirmation { plan_id, content } = &current.payload else {
            unreachable!("plan confirmation resolution was validated before resolving");
        };
        let InteractionResolution::PlanConfirmation {
            decision,
            content: resolution_content,
            reason,
        } = resolution
        else {
            unreachable!("resolution kind was validated before resolving");
        };

        if current.status != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                session_id,
                interaction: current,
                sessions: Vec::new(),
            });
        }

        let resolved = self
            .interactions
            .resolve(
                &interaction_id,
                InteractionResolution::PlanConfirmation {
                    decision: decision.clone(),
                    content: resolution_content.clone(),
                    reason: reason.clone(),
                },
                emitter,
            )
            .await?;

        match decision {
            PlanConfirmationResolution::ImplementFreshContext => {
                self.set_session_mode(&session_id, CompileMode::Auto)
                    .await?;
                self.append_plan_lifecycle_event(
                    &session_id,
                    plan_id,
                    PlanLifecycleState::Accepted,
                    None,
                    reason.filter(|value| !value.trim().is_empty()),
                )
                .await?;
                self.append_plan_lifecycle_event(
                    &session_id,
                    plan_id,
                    PlanLifecycleState::Implementing,
                    None,
                    None,
                )
                .await?;
                let plan_content = resolution_content
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| content.clone())
                    .trim()
                    .to_string();
                if plan_content.is_empty() {
                    bail!("plan content is empty");
                }
                let prompt = format!("{IMPLEMENT_PLAN_CURRENT_SESSION_PREFIX}\n\n{plan_content}");
                let _ = self
                    .submit_prompt(StudioSubmitPromptRequest {
                        session_id: session_id.clone(),
                        prompt,
                        attachment_ids: Vec::new(),
                        options: StudioSubmitPromptOptions {
                            user_prompt: StudioUserPromptPresentation::SyntheticIgnored {
                                visible_prompt: "实施计划".to_string(),
                            },
                            lifecycle: Some(StudioPlanImplementationLifecycle {
                                session_id: session_id.clone(),
                                plan_id: plan_id.clone(),
                            }),
                        },
                    })
                    .await?;
            }
            PlanConfirmationResolution::ContinuePlanning => {
                self.append_plan_lifecycle_event(
                    &session_id,
                    plan_id,
                    PlanLifecycleState::ContinuedPlanning,
                    None,
                    reason.or(resolution_content),
                )
                .await?;
            }
            PlanConfirmationResolution::Dismiss => {
                self.append_plan_lifecycle_event(
                    &session_id,
                    plan_id,
                    PlanLifecycleState::Dismissed,
                    None,
                    reason,
                )
                .await?;
            }
        }

        let sessions = if let Some(session) = self.store.read_session(&session_id).await? {
            self.store.list_sessions(&session.project_id).await?
        } else {
            Vec::new()
        };

        Ok(StudioResolveInteractionResponse {
            session_id,
            interaction: resolved,
            sessions,
        })
    }

    async fn emit_user_prompt_snapshots(
        &self,
        session_id: &str,
        turn_id: &str,
        prompt: &str,
        attachment_ids: &[String],
        options: &StudioSubmitPromptOptions,
    ) -> Result<()> {
        let now = unix_seconds();
        let message_id = format!("{turn_id}:user");
        let part_id = format!("{turn_id}:user-text");
        let attachments = self
            .store
            .load_attachments(session_id, attachment_ids)
            .await?
            .iter()
            .map(crate::studio::studio_attachment)
            .collect::<Vec<_>>();
        let message = StudioMessage {
            message_id: message_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            role: StudioMessageRole::User,
            status: StudioMessageStatus::Completed,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: None,
            metadata: if options.user_prompt.is_synthetic() || options.user_prompt.is_ignored() {
                serde_json::json!({
                    "synthetic": options.user_prompt.is_synthetic(),
                    "ignored": options.user_prompt.is_ignored(),
                })
            } else {
                serde_json::json!({})
            },
        };
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                Some(turn_id.to_string()),
                StudioEventKind::MessageUpdated {
                    message: Box::new(message),
                },
            )
            .await?;
        let part = StudioPart {
            part_id,
            message_id,
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            part_type: StudioPartType::Text,
            order: 0,
            revision: 0,
            status: StudioPartStatus::Completed,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: None,
            text_channel: Some(StudioTextChannel::User),
            text: options.user_prompt.visible_prompt(prompt).to_string(),
            attachments,
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: options.user_prompt.is_synthetic(),
            ignored: options.user_prompt.is_ignored(),
        };
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                Some(turn_id.to_string()),
                StudioEventKind::MessagePartUpdated {
                    part: Box::new(part),
                },
            )
            .await?;
        Ok(())
    }

    async fn emit_turn_completion(
        &self,
        session_id: &str,
        turn_id: &str,
        outcome: &StudioPromptOutcome,
    ) {
        let status = match outcome.result.status {
            TurnResultStatus::Completed => StudioTurnStatus::Completed,
            TurnResultStatus::Aborted
                if outcome.result.abort_reason == Some(TurnAbortReason::Interrupted) =>
            {
                StudioTurnStatus::Cancelled
            }
            TurnResultStatus::Aborted | TurnResultStatus::Errored => StudioTurnStatus::Failed,
        };
        let reason = outcome.result.error.clone().or_else(|| {
            outcome
                .result
                .abort_reason
                .map(|reason| reason.as_str().to_string())
        });
        let _ = self
            .events
            .emit_turn(session_id, turn_id, status, reason)
            .await;
    }

    async fn append_plan_lifecycle_event(
        &self,
        session_id: &str,
        plan_id: &str,
        state: PlanLifecycleState,
        turn_id: Option<String>,
        reason: Option<String>,
    ) -> Result<()> {
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                turn_id.clone(),
                StudioEventKind::PlanLifecycleChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan_id.to_string(),
                        state,
                        turn_id,
                        reason,
                        updated_at: unix_seconds(),
                    },
                },
            )
            .await?;
        Ok(())
    }

    fn interaction_emitter(&self, session_id: String) -> InteractionEmitter {
        let runtime = self.clone();
        Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            Box::pin(async move {
                runtime
                    .events
                    .emit_interaction(&session_id, InteractionChangedEvent { interaction })
                    .await?;
                Ok(())
            })
        })
    }

    async fn cancel_recovered_transient_interactions(
        &self,
        cancelled_turns: Vec<crate::studio::records::StudioTurnRecord>,
    ) -> Result<()> {
        let mut session_ids = cancelled_turns
            .into_iter()
            .map(|turn| turn.session_id)
            .collect::<Vec<_>>();
        session_ids.extend(
            self.store
                .list_sessions_with_transient_pending_interactions()
                .await?,
        );
        session_ids.sort();
        session_ids.dedup();
        for session_id in session_ids {
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_transient_interactions(&session_id, "application restarted", emitter)
                .await?;
        }
        Ok(())
    }

    async fn run_prompt_inner(&self, request: RunPromptRequest) -> Result<StudioPromptOutcome> {
        let RunPromptRequest {
            session_id,
            turn_id,
            prompt,
            attachment_ids,
            interaction_callback,
            interaction_emitter,
            mut options,
        } = request;
        let session_id = session_id.as_str();
        let session_record = self
            .store
            .read_session(session_id)
            .await?
            .context("selected session not found")?;
        let project = self
            .store
            .read_project(&session_record.project_id)
            .await?
            .context("selected project not found")?;
        let mut session = self.store.load_core_session(session_id).await?;
        let config = self.config_store.load_or_default()?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let workspace_instructions = load_workspace_instructions(&workspace_root)?;
        let previous_revision = session.revision();
        let previous_len = session.len();
        let mode = CompileMode::from_label(&session_record.mode);
        options = options.with_permission_mode(config.runtime.permission_mode);
        let selected_attachments = self
            .store
            .load_attachments(session_id, &attachment_ids)
            .await?;
        let selected_materialized = self
            .store
            .materialize_attachments(session_id, &attachment_ids)
            .await?;
        let trace_attachments = selected_attachments
            .iter()
            .map(|record| {
                let mut attachment = crate::studio::store::trace_attachment(record);
                attachment.data_url = selected_materialized
                    .iter()
                    .find(|materialized| materialized.attachment_id == record.id)
                    .map(|materialized| {
                        format!(
                            "data:{};base64,{}",
                            materialized.media_type, materialized.data
                        )
                    });
                attachment
            })
            .collect::<Vec<_>>();
        let mut materialized_attachments = self
            .store
            .materialize_session_attachments(session_id)
            .await?;
        for attachment in selected_materialized {
            if !materialized_attachments
                .iter()
                .any(|existing| existing.attachment_id == attachment.attachment_id)
            {
                materialized_attachments.push(attachment);
            }
        }
        let user_content = prompt_content(&prompt, &selected_attachments);
        let mut request = TurnRequest::new(prompt.clone(), mode)
            .with_turn_id(turn_id.clone())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized_attachments)
            .with_trace_attachments(trace_attachments);
        if !workspace_instructions.trim().is_empty() {
            request = request.with_workspace_instructions(workspace_instructions.clone());
        }
        let instruction_snapshot = self
            .resolve_instruction_snapshot(
                session_id,
                session_record.instruction_snapshot.as_ref(),
                &config,
                &workspace_root,
                Path::new(&project.path),
                mode,
            )
            .await?;
        request = request.with_instruction_snapshot(instruction_snapshot);
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await;
        self.lsp_runtime.reconcile_workspace(&workspace_root).await;

        let mut core = PureCore::from_config(&config, ModelRole::Planner)?
            .with_mcp_runtime(self.mcp_runtime.clone())
            .with_lsp_runtime(self.lsp_runtime.clone());
        core.register_default_tools(workspace_root.clone(), Some(workspace_instructions.clone()))
            .await;
        core.register_available_mcp_tools().await?;
        if options.interaction_callback.is_none()
            && (options.requires_user_approval_callback() || mode == CompileMode::Plan)
        {
            options.interaction_callback = Some(interaction_callback.clone());
        }
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
        let event_runtime = self.clone();
        let event_session_id = session_id.to_string();
        let event_task = tokio::spawn(async move {
            event_runtime
                .drain_agent_events(event_session_id, event_rx)
                .await;
        });
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx.clone(), 0);
        let result = core
            .run_turn_with_trace(&mut session, request, &mut recorder, options)
            .await;
        drop(recorder);
        drop(event_tx);
        let _ = event_task.await;
        let result = result?;
        let trace_events = result.trace_events.clone();
        if session.revision() != previous_revision {
            self.store
                .replace_turn_records(session_id, &trace_events, session.messages())
                .await?;
        } else {
            let new_messages = &session.messages()[previous_len..];
            self.store
                .append_turn_records(session_id, &trace_events, new_messages)
                .await?;
        }
        if matches!(mode, CompileMode::Plan)
            && matches!(result.status, TurnResultStatus::Completed)
            && let Some(plan) = completed_plan_item(&trace_events)
        {
            self.create_plan_confirmation(session_id, &plan, interaction_emitter)
                .await?;
        }
        let resolved = config.resolve_role(ModelRole::Planner)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == result.model)
            .or_else(|| {
                resolved
                    .models
                    .iter()
                    .find(|model| model.slug == resolved.role_config.model)
            })
            .or_else(|| resolved.models.first());
        self.store
            .upsert_session_runtime_for_turn(session_id, &turn_id, &result, model)
            .await?;
        if should_start_self_learning(&config, &result.status, &trace_events) {
            let review_messages = session.messages().to_vec();
            spawn_self_learning_review(
                config.clone(),
                workspace_root.clone(),
                workspace_instructions.clone(),
                review_messages,
            );
        }
        if previous_len == 0 {
            self.store
                .rename_session(session_id, &session_title_from_prompt(&prompt))
                .await?;
        }
        let messages = self.store.load_messages(session_id).await?;
        Ok(StudioPromptOutcome {
            result,
            messages,
            trace_events,
        })
    }

    pub async fn drain_agent_events(
        &self,
        session_id: String,
        mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    ) {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    if let AgentEvent::AgentRuntimeUpdated { delta } = &event {
                        let _ = self
                            .store
                            .record_agent_runtime_delta(&session_id, delta)
                            .await;
                    }
                    let _ = self
                        .events
                        .emit_agent_event(&session_id, event.clone())
                        .await
                        .ok()
                        .flatten();
                    if let Some(agent) = self.agent_snapshot_for_event(&session_id, &event).await {
                        let _ = self
                            .events
                            .emit(
                                None,
                                Some(session_id.clone()),
                                None,
                                StudioEventKind::AgentChanged { agent },
                            )
                            .await;
                    }
                    if matches!(
                        event,
                        AgentEvent::AgentRuntimeUpdated { .. } | AgentEvent::SkillActivated { .. }
                    ) && let Ok(runtime) = self.session_runtime_event(&session_id).await
                    {
                        let _ = self
                            .events
                            .emit(
                                None,
                                Some(session_id.clone()),
                                None,
                                StudioEventKind::SessionRuntimeChanged { runtime },
                            )
                            .await;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    let _ = self.events.emit_stale(&session_id, skipped).await;
                }
                Err(RecvError::Closed) => break,
            }
        }
    }

    async fn agent_snapshot_for_event(
        &self,
        session_id: &str,
        event: &AgentEvent,
    ) -> Option<StudioAgentSnapshot> {
        match event {
            AgentEvent::AgentStateChanged { id, .. } => self
                .store
                .list_agents(session_id)
                .await
                .ok()
                .and_then(|agents| {
                    agents
                        .into_iter()
                        .find(|agent| agent.id == *id)
                        .map(studio_agent_snapshot)
                }),
            AgentEvent::AgentRuntimeUpdated { delta } if delta.agent_id != "agent-root" => self
                .store
                .list_agents(session_id)
                .await
                .ok()
                .and_then(|agents| {
                    agents
                        .into_iter()
                        .find(|agent| agent.id == delta.agent_id)
                        .map(studio_agent_snapshot)
                }),
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => None,
        }
    }

    async fn session_runtime_event(&self, session_id: &str) -> Result<StudioSessionRuntime> {
        let runtime = self.session_runtime(session_id).await?;
        let active_skills = self.store.list_session_skill_names(session_id).await?;
        Ok(studio_session_runtime(
            runtime,
            active_skills,
            self.mcp_runtime.available_server_names().await,
            self.lsp_runtime.active_server_names().await,
        ))
    }

    async fn create_plan_confirmation(
        &self,
        session_id: &str,
        plan: &TracePart,
        interaction_emitter: InteractionEmitter,
    ) -> Result<()> {
        if plan.content.trim().is_empty() {
            return Ok(());
        }
        if self
            .store
            .read_interaction(&plan_confirmation_id(&plan.item_id))
            .await?
            .is_some()
        {
            return Ok(());
        }

        let now = unix_seconds();
        let lifecycle = PlanLifecycleEvent {
            plan_id: plan.item_id.clone(),
            state: PlanLifecycleState::PendingConfirmation,
            turn_id: Some(plan.turn_id.clone()),
            reason: None,
            updated_at: now,
        };
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                Some(plan.turn_id.clone()),
                pl_protocol::StudioEventKind::PlanLifecycleChanged { event: lifecycle },
            )
            .await?;

        let interaction = InteractionRequest {
            interaction_id: plan_confirmation_id(&plan.item_id),
            kind: InteractionKind::PlanConfirmation,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: session_id.to_string(),
                turn_id: plan.turn_id.clone(),
                item_id: Some(plan.item_id.clone()),
                tool_id: None,
                agent_path: None,
            },
            payload: InteractionPayload::PlanConfirmation {
                plan_id: plan.item_id.clone(),
                content: plan.content.clone(),
            },
            created_at: now,
            updated_at: now,
            resolved_at: None,
            resolution: None,
        };
        self.interactions
            .create(interaction, interaction_emitter)
            .await?;
        Ok(())
    }

    async fn resolve_instruction_snapshot(
        &self,
        session_id: &str,
        existing: Option<&InstructionSnapshot>,
        config: &crate::config::PureConfig,
        workspace_root: &Path,
        project_path: &Path,
        mode: CompileMode,
    ) -> Result<InstructionSnapshot> {
        let resolved = config.resolve_role(ModelRole::Planner)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == resolved.role_config.model)
            .cloned()
            .unwrap_or_else(|| pl_model::ModelInfo::fallback(&resolved.role_config.model));
        let current_dir =
            std::fs::canonicalize(project_path).unwrap_or_else(|_| workspace_root.to_path_buf());
        if let Some(snapshot) = existing {
            return Ok(snapshot.with_turn_overlay(InstructionAssemblyRequest {
                config: Some(config),
                model: &model,
                mode,
                workspace_root,
                current_dir: &current_dir,
                workspace_instructions: None,
                subagent_constraint: None,
            })?);
        }
        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: Some(config),
            model: &model,
            mode,
            workspace_root,
            current_dir: &current_dir,
            workspace_instructions: None,
            subagent_constraint: None,
        })?;
        self.store
            .save_instruction_snapshot(session_id, &snapshot)
            .await?
            .context("selected session disappeared while saving instruction snapshot")?;
        Ok(snapshot)
    }
}

fn completed_plan_item(events: &[TraceEvent]) -> Option<TracePart> {
    events.iter().rev().find_map(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Plan && !item.content.trim().is_empty() =>
        {
            Some(item.clone())
        }
        TraceEventKind::TracePartStarted { .. }
        | TraceEventKind::TracePartDelta { .. }
        | TraceEventKind::TracePartCompleted { .. }
        | TraceEventKind::TracePartFailed { .. }
        | TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => None,
    })
}

fn prompt_content(prompt: &str, attachments: &[crate::studio::AttachmentRecord]) -> MessageContent {
    if attachments.is_empty() {
        return MessageContent::Text(prompt.to_string());
    }
    let mut parts = Vec::new();
    if !prompt.is_empty() {
        parts.push(ContentPart::Text {
            text: prompt.to_string(),
        });
    }
    parts.extend(attachments.iter().map(|attachment| ContentPart::Image {
        source: ImageSource::Attachment {
            attachment_id: attachment.id.clone(),
        },
        media_type: attachment.media_type.clone(),
        filename: attachment.filename.clone(),
    }));
    MessageContent::MultiPart(parts)
}

fn plan_confirmation_id(plan_id: &str) -> String {
    format!("plan-confirmation-{plan_id}")
}

fn should_start_self_learning(
    config: &crate::config::PureConfig,
    status: &TurnResultStatus,
    trace_events: &[TraceEvent],
) -> bool {
    config.skills.enabled
        && config.skills.auto_learn
        && matches!(status, TurnResultStatus::Completed)
        && tool_call_count(trace_events) >= config.skills.auto_learn_min_tool_calls
}

fn tool_call_count(trace_events: &[TraceEvent]) -> u32 {
    trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item } if item.kind == TracePartKind::Tool => {
                Some(item.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<std::collections::HashSet<_>>()
        .len() as u32
}

#[cfg(test)]
fn started_tool_snapshot_count(trace_events: &[TraceEvent]) -> u32 {
    trace_events
        .iter()
        .filter(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item } if item.kind == TracePartKind::Tool => true,
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
        .count() as u32
}

fn studio_agent_snapshot(agent: AgentSnapshotRecord) -> StudioAgentSnapshot {
    StudioAgentSnapshot {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth.max(0) as u32,
        error: agent.error,
        reason: agent.reason,
        budget_limit_kind: agent.budget_limit_kind,
        budget_usage: agent.budget_usage,
        runtime_usage: agent.runtime_usage.map(studio_runtime_usage),
        updated_at: agent.updated_at,
    }
}

fn studio_session_runtime(
    runtime: SessionRuntimeRecord,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
    active_lsp_servers: Vec<String>,
) -> StudioSessionRuntime {
    StudioSessionRuntime {
        session_id: runtime.session_id,
        usage: studio_runtime_usage(pl_protocol::RuntimeUsageSnapshot {
            model: runtime.model,
            context_window: runtime.context_window,
            latest_context_tokens: runtime.latest_context_tokens,
            prompt_tokens: runtime.prompt_tokens,
            completion_tokens: runtime.completion_tokens,
            cached_prompt_tokens: runtime.cached_prompt_tokens,
            total_tokens: runtime.total_tokens,
            estimated_costs: runtime.estimated_costs,
            has_unpriced_usage: runtime.has_unpriced_usage,
            updated_at: runtime.updated_at,
        }),
        active_skills,
        active_mcp_servers,
        active_lsp_servers,
        updated_at: runtime.updated_at,
    }
}

fn studio_runtime_usage(usage: pl_protocol::RuntimeUsageSnapshot) -> StudioRuntimeUsage {
    let cache_hit_rate = if usage.prompt_tokens == 0 {
        None
    } else {
        Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64)
    };
    StudioRuntimeUsage {
        model: usage.model,
        context_window: usage.context_window,
        latest_context_tokens: usage.latest_context_tokens,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        total_tokens: usage.total_tokens,
        cache_hit_rate,
        estimated_costs: usage.estimated_costs,
        has_unpriced_usage: usage.has_unpriced_usage,
        updated_at: usage.updated_at,
    }
}

fn spawn_self_learning_review(
    config: crate::config::PureConfig,
    workspace_root: std::path::PathBuf,
    workspace_instructions: String,
    messages: Vec<pl_protocol::Message>,
) {
    tokio::spawn(async move {
        if let Err(error) =
            run_self_learning_review(config, workspace_root, workspace_instructions, messages).await
        {
            eprintln!("[pl-core] self-learning skill review failed: {error}");
        }
    });
}

async fn run_self_learning_review(
    config: crate::config::PureConfig,
    workspace_root: std::path::PathBuf,
    workspace_instructions: String,
    messages: Vec<pl_protocol::Message>,
) -> Result<()> {
    let mut core = PureCore::from_config(&config, ModelRole::Reviewer)?;
    core.register_skill_tools(workspace_root, Some(workspace_instructions.clone()));
    let mut session = CoreSession::from_messages(messages);
    let request = TurnRequest::new(SELF_LEARNING_REVIEW_PROMPT.to_string(), CompileMode::Auto)
        .with_workspace_instructions(workspace_instructions)
        .with_budget(TurnBudget::new(120_000));
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::disabled(event_tx);
    let _ = core
        .run_turn_with_trace(&mut session, request, &mut recorder, TurnOptions::default())
        .await?;
    Ok(())
}

fn session_title_from_prompt(prompt: &str) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return "新会话".to_string();
    }
    prompt.chars().take(42).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pl_model::{ModelInfo, ProviderInfo};
    use pl_trace::{TracePart, TracePartStatus, TraceTextChannel};
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::{Mutex, oneshot};

    use super::*;
    use crate::config::{ProviderConfig, RoleConfig, RoleConfigs};

    const TEST_RUNTIME_TIMEOUT: Duration = Duration::from_secs(20);

    async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
        serve_sse_sequence(vec![sse_body]).await
    }

    async fn serve_sse_sequence(sse_bodies: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            for sse_body in sse_bodies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = Vec::new();
                let mut temp = [0_u8; 1024];
                let (header_end, content_length) = loop {
                    let n = socket.read(&mut temp).await.unwrap();
                    assert_ne!(n, 0);
                    buffer.extend_from_slice(&temp[..n]);
                    if let Some(header_end) =
                        buffer.windows(4).position(|window| window == b"\r\n\r\n")
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
            }
        });

        (format!("http://{addr}"), handle)
    }

    async fn serve_delayed_sse() -> (
        String,
        tokio::task::JoinHandle<()>,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            loop {
                let n = socket.read(&mut temp).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                buffer.extend_from_slice(&temp[..n]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = accepted_tx.send(());
            let _ = release_rx.await;
            let sse_body = "data: [DONE]\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        (format!("http://{addr}"), handle, accepted_rx, release_tx)
    }

    fn test_config(base_url: String) -> crate::config::PureConfig {
        let mut model = ModelInfo::fallback("local-responses");
        model.parameters = vec![crate::ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["none".to_string()],
            wire: std::collections::BTreeMap::new(),
        }];
        let mut info = ProviderInfo::openai(Some(base_url));
        info.default_model = "local-responses".to_string();
        let provider = ProviderConfig::from_provider_info(info, vec![model]);
        let role = RoleConfig {
            provider: "local".to_string(),
            model: "local-responses".to_string(),
            effort: crate::config::ReasoningEffort::new("none"),
        };
        crate::config::PureConfig {
            roles: RoleConfigs::from_default_role(role),
            providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
            ..crate::config::PureConfig::default_config()
        }
    }

    fn test_chat_config(base_url: String) -> crate::config::PureConfig {
        let mut model = ModelInfo::fallback("local-chat");
        model.context_window = Some(128_000);
        model.parameters = vec![crate::ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["none".to_string()],
            wire: std::collections::BTreeMap::new(),
        }];
        let mut info = ProviderInfo::deepseek(Some(base_url));
        info.default_model = "local-chat".to_string();
        let provider = ProviderConfig::from_provider_info(info, vec![model]);
        let role = RoleConfig {
            provider: "local".to_string(),
            model: "local-chat".to_string(),
            effort: crate::config::ReasoningEffort::new("none"),
        };
        crate::config::PureConfig {
            roles: RoleConfigs::from_default_role(role),
            providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
            ..crate::config::PureConfig::default_config()
        }
    }

    fn emitter(
        events: std::sync::Arc<Mutex<Vec<InteractionRequest>>>,
    ) -> crate::studio::InteractionEmitter {
        std::sync::Arc::new(move |interaction| {
            let events = events.clone();
            Box::pin(async move {
                events.lock().await.push(interaction);
                Ok(())
            })
        })
    }

    fn pending_interaction(
        id: &str,
        session_id: &str,
        kind: InteractionKind,
        payload: InteractionPayload,
    ) -> InteractionRequest {
        InteractionRequest {
            interaction_id: id.to_string(),
            kind,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: session_id.to_string(),
                turn_id: "turn-recovered".to_string(),
                item_id: Some(id.to_string()),
                tool_id: Some(id.to_string()),
                agent_path: None,
            },
            payload,
            created_at: 1,
            updated_at: 1,
            resolved_at: None,
            resolution: None,
        }
    }

    async fn wait_for_no_active_turn(runtime: &StudioRuntime) {
        tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
            loop {
                if runtime.runtime_snapshot().active_turns.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn set_model_role_persists_planner_model_and_default_effort() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-role-runtime-home-{unique}"));
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        let mut config = test_config("http://127.0.0.1:9".to_string());
        let mut fast_model = ModelInfo::fallback("local-fast");
        fast_model.parameters = vec![crate::ModelParameter {
            name: "effort".to_string(),
            label: None,
            candidates: vec!["low".to_string(), "high".to_string()],
            wire: std::collections::BTreeMap::new(),
        }];
        config
            .providers
            .get_mut("local")
            .unwrap()
            .models
            .push(fast_model);
        config_store.save(&config).unwrap();
        let runtime = StudioRuntime::new(StudioStore::open_memory().await.unwrap(), config_store);

        let next = runtime
            .set_model_role(ModelRole::Planner, "local", "local-fast", None)
            .unwrap();

        assert_eq!(next.roles.planner.provider, "local");
        assert_eq!(next.roles.planner.model, "local-fast");
        assert_eq!(next.roles.planner.effort.as_str(), "low");
        let saved = runtime.config_store().load_or_default().unwrap();
        assert_eq!(saved.roles.planner, next.roles.planner);
        let _ = tokio::fs::remove_dir_all(home).await;
    }

    #[tokio::test]
    async fn initialize_runtime_cancels_recovered_transient_interactions() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/recovered").await.unwrap();
        let session = store
            .create_session(&project.id, "Recovered", CompileMode::Auto)
            .await
            .unwrap();
        store
            .create_turn(
                &session.id,
                "turn-recovered",
                StudioTurnStatus::WaitingForModel,
                1,
            )
            .await
            .unwrap();
        store
            .upsert_interaction(&pending_interaction(
                "ask-recovered",
                &session.id,
                InteractionKind::UserInput,
                InteractionPayload::UserInput {
                    questions: Vec::new(),
                },
            ))
            .await
            .unwrap();
        store
            .upsert_interaction(&pending_interaction(
                "approval-recovered",
                &session.id,
                InteractionKind::ToolApproval,
                InteractionPayload::ToolApproval {
                    name: "bash".to_string(),
                    arguments: serde_json::json!({"command": "echo hi"}),
                    working_directory: None,
                    parent_agent_id: None,
                },
            ))
            .await
            .unwrap();
        store
            .upsert_interaction(&pending_interaction(
                "plan-recovered",
                &session.id,
                InteractionKind::PlanConfirmation,
                InteractionPayload::PlanConfirmation {
                    plan_id: "plan-1".to_string(),
                    content: "Plan".to_string(),
                },
            ))
            .await
            .unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-recovered-runtime-home-{unique}"));
        let runtime = StudioRuntime::with_runtime_state(
            store.clone(),
            ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
            StudioRuntimeState::new(),
        );

        let snapshot = runtime.initialize_runtime().await.unwrap();

        assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
        assert_eq!(snapshot.active_turns, Vec::new());
        let ask = store
            .read_interaction("ask-recovered")
            .await
            .unwrap()
            .unwrap();
        let approval = store
            .read_interaction("approval-recovered")
            .await
            .unwrap()
            .unwrap();
        let plan = store
            .read_interaction("plan-recovered")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ask.status, InteractionStatus::Cancelled);
        assert_eq!(approval.status, InteractionStatus::Cancelled);
        assert_eq!(plan.status, InteractionStatus::Pending);
        let studio_events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        let cancelled_interactions = studio_events
            .iter()
            .filter(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::InteractionChanged { event }
                        if event.interaction.status == InteractionStatus::Cancelled
                )
            })
            .count();
        assert_eq!(cancelled_interactions, 2);
        let _ = tokio::fs::remove_dir_all(home).await;
    }

    #[tokio::test]
    async fn ui_submit_and_stop_are_core_runtime_apis() {
        let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-ui-runtime-home-{unique}"));
        let workspace = std::env::temp_dir().join(format!("pure-ui-runtime-workspace-{unique}"));
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        config_store.save(&test_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "UI runtime", CompileMode::Auto)
            .await
            .unwrap();

        let submitted = runtime
            .submit_prompt(StudioSubmitPromptRequest {
                session_id: session.id.clone(),
                prompt: "wait until stopped".to_string(),
                attachment_ids: Vec::new(),
                options: StudioSubmitPromptOptions::default(),
            })
            .await
            .unwrap();

        assert_eq!(submitted.session_id, session.id);
        assert_eq!(runtime.runtime_snapshot().active_turns.len(), 1);
        tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
            .await
            .unwrap()
            .unwrap();
        let stopped = runtime.stop_prompt(session.id.clone()).await.unwrap();

        assert_eq!(stopped.session_id, session.id);
        assert!(stopped.stopped);
        let _ = release_tx.send(());
        wait_for_no_active_turn(&runtime).await;
        handle.await.unwrap();
        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }

    #[tokio::test]
    async fn ui_submit_clears_active_runtime_snapshot_after_completion() {
        let sse_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"done\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-ui-complete-home-{unique}"));
        let workspace = std::env::temp_dir().join(format!("pure-ui-complete-workspace-{unique}"));
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        config_store.save(&test_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "UI completion", CompileMode::Auto)
            .await
            .unwrap();

        runtime
            .submit_prompt(StudioSubmitPromptRequest {
                session_id: session.id.clone(),
                prompt: "complete".to_string(),
                attachment_ids: Vec::new(),
                options: StudioSubmitPromptOptions::default(),
            })
            .await
            .unwrap();
        assert_eq!(runtime.runtime_snapshot().active_turns.len(), 1);

        wait_for_no_active_turn(&runtime).await;
        handle.await.unwrap();
        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }

    #[test]
    fn counts_started_tool_items_for_self_learning_threshold() {
        let mut item = TracePart {
            turn_id: "turn".to_string(),
            item_id: "tool".to_string(),
            started_sequence: 1,
            revision: 0,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Started,
            created_at: 1,
            updated_at: 1,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        let started = TraceEvent {
            session_id: "session".to_string(),
            sequence: 1,
            timestamp: 1,
            kind: TraceEventKind::TracePartStarted { item: item.clone() },
        };
        item.status = TracePartStatus::Running;
        let running = TraceEvent {
            session_id: "session".to_string(),
            sequence: 2,
            timestamp: 2,
            kind: TraceEventKind::TracePartStarted { item },
        };

        assert_eq!(
            started_tool_snapshot_count(&[started.clone(), running.clone()]),
            2
        );
        assert_eq!(tool_call_count(&[started, running]), 1);
    }

    #[tokio::test]
    async fn proposed_plan_tag_does_not_create_pending_confirmation_interaction() {
        let sse_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<proposed_plan>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"1. Inspect\\\\n2. Implement\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"</proposed_plan><final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
        let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        config_store.save(&test_chat_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "Plan test", CompileMode::Plan)
            .await
            .unwrap();
        let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let interaction_emitter = emitter(interaction_events.clone());
        let interaction_callback = runtime
            .interactions()
            .callback(session.id.clone(), interaction_emitter.clone());

        let outcome = runtime
            .run_prompt(RunPromptRequest {
                session_id: session.id.clone(),
                turn_id: "turn-plan-test".to_string(),
                prompt: "make a plan".to_string(),
                attachment_ids: Vec::new(),
                interaction_callback,
                interaction_emitter,
                options: TurnOptions::default(),
            })
            .await
            .unwrap();
        handle.await.unwrap();

        assert_eq!(outcome.result.status, TurnResultStatus::Completed);
        assert!(outcome.result.content.contains("Ready"));
        let plan_item = outcome
            .trace_events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                    Some(item)
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            });
        assert!(plan_item.is_none());
        assert!(outcome.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.text_channel == Some(TraceTextChannel::Final)
                    && item.content.contains("Ready")
        )));
        let studio_events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        assert!(!studio_events.iter().any(|envelope| {
            matches!(
                &envelope.kind,
                StudioEventKind::PlanLifecycleChanged { event }
                    if event.state == PlanLifecycleState::PendingConfirmation
            )
        }));
        assert!(interaction_events.lock().await.is_empty());
        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }

    #[tokio::test]
    async fn plan_exit_tool_creates_pending_confirmation_interaction() {
        let tool_sse = concat!(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\",\"arguments\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let final_sse = concat!(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_2\",\"delta\":\"Plan submitted.\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Plan submitted.\"}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
        let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        config_store.save(&test_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "Plan exit test", CompileMode::Plan)
            .await
            .unwrap();
        let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let interaction_emitter = emitter(interaction_events.clone());
        let interaction_callback = runtime
            .interactions()
            .callback(session.id.clone(), interaction_emitter.clone());

        let outcome = runtime
            .run_prompt(RunPromptRequest {
                session_id: session.id.clone(),
                turn_id: "turn-plan-exit-test".to_string(),
                prompt: "make a plan".to_string(),
                attachment_ids: Vec::new(),
                interaction_callback,
                interaction_emitter,
                options: TurnOptions::default(),
            })
            .await
            .unwrap();
        handle.await.unwrap();

        assert_eq!(outcome.result.status, TurnResultStatus::Completed);
        assert_eq!(outcome.result.content, "Plan submitted.");
        let plan_item = outcome
            .trace_events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                    Some(item)
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("completed plan item");
        assert_eq!(plan_item.content, "# Plan\n\n- Inspect\n- Implement");

        let interaction = store
            .read_interaction(&plan_confirmation_id(&plan_item.item_id))
            .await
            .unwrap()
            .expect("plan confirmation interaction");
        assert_eq!(interaction.kind, InteractionKind::PlanConfirmation);
        assert_eq!(interaction.status, InteractionStatus::Pending);
        assert_eq!(
            interaction.payload,
            InteractionPayload::PlanConfirmation {
                plan_id: plan_item.item_id.clone(),
                content: "# Plan\n\n- Inspect\n- Implement".to_string(),
            }
        );
        let studio_events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        assert!(studio_events.iter().any(|envelope| {
            matches!(
                &envelope.kind,
                StudioEventKind::PlanLifecycleChanged { event }
                    if event.plan_id == plan_item.item_id
                        && event.state == PlanLifecycleState::PendingConfirmation
            )
        }));
        assert_eq!(interaction_events.lock().await.len(), 1);
        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }

    #[tokio::test]
    async fn tool_boundary_with_reused_provider_ids_creates_new_parts_after_tool() {
        let tool_sse = concat!(
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"before tool\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"before \"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"path\\\":\\\".\\\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\",\"arguments\":\"{\\\"path\\\":\\\".\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let final_sse = concat!(
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"after tool\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"after\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
        let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::write(workspace.join("alpha.txt"), "alpha")
            .await
            .unwrap();
        let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
        config_store.save(&test_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "Tool boundary test", CompileMode::Auto)
            .await
            .unwrap();
        let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let interaction_emitter = emitter(interaction_events.clone());
        let interaction_callback = runtime
            .interactions()
            .callback(session.id.clone(), interaction_emitter.clone());

        let outcome = runtime
            .run_prompt(RunPromptRequest {
                session_id: session.id.clone(),
                turn_id: "turn-tool-boundary-test".to_string(),
                prompt: "list files and continue".to_string(),
                attachment_ids: Vec::new(),
                interaction_callback,
                interaction_emitter,
                options: TurnOptions::default(),
            })
            .await
            .unwrap();
        handle.await.unwrap();

        assert_eq!(outcome.result.status, TurnResultStatus::Completed);
        assert_eq!(outcome.result.content, "after");

        let parts = store.load_message_parts(&session.id).await.unwrap();
        let assistant_parts = parts
            .iter()
            .filter(|record| record.part.message_id == "turn-tool-boundary-test:assistant")
            .map(|record| &record.part)
            .collect::<Vec<_>>();
        let compact = assistant_parts
            .iter()
            .filter_map(|part| match part.part_type {
                pl_protocol::StudioPartType::Reasoning | pl_protocol::StudioPartType::Text => {
                    Some((
                        part.part_id.as_str(),
                        part.part_type,
                        part.text.as_str(),
                        part.order,
                    ))
                }
                pl_protocol::StudioPartType::Tool
                | pl_protocol::StudioPartType::Agent
                | pl_protocol::StudioPartType::Turn
                | pl_protocol::StudioPartType::Inference
                | pl_protocol::StudioPartType::Plan
                | pl_protocol::StudioPartType::File => None,
            })
            .collect::<Vec<_>>();
        let compact_identity = compact
            .iter()
            .map(|(part_id, part_type, text, _)| (*part_id, *part_type, *text))
            .collect::<Vec<_>>();

        assert_eq!(
            compact_identity,
            vec![
                (
                    "turn-tool-boundary-test-inf-0-reasoning-1",
                    pl_protocol::StudioPartType::Reasoning,
                    "before tool",
                ),
                (
                    "turn-tool-boundary-test-inf-0-text-final-1",
                    pl_protocol::StudioPartType::Text,
                    "before ",
                ),
                (
                    "turn-tool-boundary-test-inf-1-reasoning-1",
                    pl_protocol::StudioPartType::Reasoning,
                    "after tool",
                ),
                (
                    "turn-tool-boundary-test-inf-1-text-final-1",
                    pl_protocol::StudioPartType::Text,
                    "after",
                ),
            ]
        );
        assert!(compact[0].3 < compact[1].3);
        assert!(compact[1].3 < compact[2].3);
        assert!(compact[2].3 < compact[3].3);
        let tool = assistant_parts
            .iter()
            .find(|part| {
                part.part_type == pl_protocol::StudioPartType::Tool
                    && part.part_id == "turn-tool-boundary-test-fc_1"
            })
            .expect("tool part");
        assert!(tool.order > compact[1].3 && tool.order < compact[2].3);

        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }
}
