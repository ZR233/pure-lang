use std::path::Path;

use anyhow::{Context, Result};
use pl_protocol::{
    AgentEventSender, ContentPart, ImageSource, InteractionKind, InteractionPayload,
    InteractionRequest, InteractionScope, InteractionStatus, MessageContent, PlanLifecycleEvent,
    PlanLifecycleState, TimelineItem, TimelineItemKind, TraceEvent, TraceEventKind,
};

use crate::config::{ConfigStore, ModelRole};
use crate::mcp::McpRuntimeRegistry;
use crate::skill::SkillCatalog;
use crate::studio::StudioStore;
use crate::studio::ids::unix_seconds;
use crate::studio::mappers::default_session_runtime_record;
use crate::studio::records::{
    ProjectRecord, SessionRecord, SessionRuntimeRecord, StudioPromptOutcome,
};
use crate::studio::{InteractionEmitter, InteractionRuntime, StudioEventRuntime};
use crate::{
    CompileMode, CoreSession, InstructionAssembler, InstructionAssemblyRequest,
    InstructionSnapshot, InteractionCallback, PureCore, TraceRecorder, TurnBudget, TurnOptions,
    TurnRequest, TurnResultStatus, load_workspace_instructions, resolve_workspace_root,
};

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
    pub prompt: String,
    pub attachment_ids: Vec<String>,
    pub event_tx: AgentEventSender,
    pub interaction_callback: InteractionCallback,
    pub interaction_emitter: InteractionEmitter,
    pub options: TurnOptions,
}

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
    mcp_runtime: McpRuntimeRegistry,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionRuntime,
    events: StudioEventRuntime,
}

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        let store = StudioStore::default_app().await?;
        let runtime = Self {
            interactions: InteractionRuntime::new(store.clone()),
            events: StudioEventRuntime::new(store.clone()),
            store,
            config_store: ConfigStore::default_app()?,
            mcp_runtime: McpRuntimeRegistry::new(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
        };
        let _ = runtime
            .store
            .cancel_unfinished_turns("application restarted")
            .await?;
        Ok(runtime)
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Self {
        Self {
            interactions: InteractionRuntime::new(store.clone()),
            events: StudioEventRuntime::new(store.clone()),
            store,
            config_store,
            mcp_runtime: McpRuntimeRegistry::new(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
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

    pub async fn shutdown(&self) {
        self.mcp_runtime.shutdown().await;
        self.lsp_runtime.shutdown().await;
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
        self.store
            .create_session(project_id, title, CompileMode::Auto)
            .await
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

    pub async fn run_prompt(&self, request: RunPromptRequest) -> Result<StudioPromptOutcome> {
        let RunPromptRequest {
            session_id,
            prompt,
            attachment_ids,
            event_tx,
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
        let timeline_attachments = selected_attachments
            .iter()
            .map(|record| {
                let mut attachment = crate::studio::store::timeline_attachment(record);
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
            .with_user_content(user_content)
            .with_materialized_attachments(materialized_attachments)
            .with_timeline_attachments(timeline_attachments);
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
        let starting_sequence = self.store.next_timeline_sequence(session_id).await?;
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, starting_sequence);
        let result = core
            .run_turn_with_trace(&mut session, request, &mut recorder, options)
            .await?;
        let timeline_events = result.timeline_events.clone();
        if session.revision() != previous_revision {
            self.store
                .replace_turn_records(session_id, &timeline_events, session.messages())
                .await?;
        } else {
            let new_messages = &session.messages()[previous_len..];
            self.store
                .append_turn_records(session_id, &timeline_events, new_messages)
                .await?;
        }
        if matches!(mode, CompileMode::Plan)
            && matches!(result.status, TurnResultStatus::Completed)
            && let Some(plan) = completed_plan_item(&timeline_events)
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
            .upsert_session_runtime(session_id, &result, model)
            .await?;
        if should_start_self_learning(&config, &result.status, &timeline_events) {
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
            timeline_events,
        })
    }

    async fn create_plan_confirmation(
        &self,
        session_id: &str,
        plan: &TimelineItem,
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

fn completed_plan_item(events: &[TraceEvent]) -> Option<TimelineItem> {
    events.iter().rev().find_map(|event| match &event.kind {
        TraceEventKind::TimelineItemCompleted { item }
            if item.kind == TimelineItemKind::Plan && !item.content.trim().is_empty() =>
        {
            Some(item.clone())
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
    timeline_events: &[TraceEvent],
) -> bool {
    config.skills.enabled
        && config.skills.auto_learn
        && matches!(status, TurnResultStatus::Completed)
        && tool_call_count(timeline_events) >= config.skills.auto_learn_min_tool_calls
}

fn tool_call_count(timeline_events: &[TraceEvent]) -> u32 {
    timeline_events
        .iter()
        .filter(|event| match &event.kind {
            TraceEventKind::TimelineItemStarted { item } => item.kind == TimelineItemKind::Tool,
            TraceEventKind::TimelineItemDelta { .. }
            | TraceEventKind::TimelineItemCompleted { .. }
            | TraceEventKind::TimelineItemFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
        .count() as u32
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
    use pl_model::{ModelInfo, ProviderInfo};
    use pl_protocol::{
        AgentEvent, InteractionRequest, TimelineItem, TimelineItemStatus, TimelineTextChannel,
    };
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    use super::*;
    use crate::config::{ModelConfig, ProviderConfig, RoleConfig, RoleConfigs};

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

    fn test_config(base_url: String) -> crate::config::PureConfig {
        let mut model = ModelInfo::fallback("local-responses");
        model.reasoning_efforts = vec!["none".to_string()];
        let model = ModelConfig::from_model_info(model);
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

    #[test]
    fn counts_started_tool_items_for_self_learning_threshold() {
        let event = TraceEvent {
            session_id: "session".to_string(),
            sequence: 1,
            timestamp: 1,
            kind: TraceEventKind::TimelineItemStarted {
                item: TimelineItem {
                    turn_id: "turn".to_string(),
                    item_id: "tool".to_string(),
                    sequence: 1,
                    kind: TimelineItemKind::Tool,
                    status: TimelineItemStatus::Running,
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
                },
            },
        };

        assert_eq!(tool_call_count(&[event]), 1);
    }

    #[tokio::test]
    async fn plan_turn_creates_pending_confirmation_interaction() {
        let sse_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"<proposed_plan>\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"1. Inspect\\\\n2. Implement\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"</proposed_plan><final>Ready</final>\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
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
        config_store.save(&test_config(base_url)).unwrap();
        let store = StudioStore::open_memory().await.unwrap();
        let runtime = StudioRuntime::new(store.clone(), config_store);
        let project = runtime.open_project(&workspace).await.unwrap();
        let session = store
            .create_session(&project.id, "Plan test", CompileMode::Plan)
            .await
            .unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<AgentEvent>(32);
        let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let interaction_emitter = emitter(interaction_events.clone());
        let interaction_callback = runtime
            .interactions()
            .callback(session.id.clone(), interaction_emitter.clone());

        let outcome = runtime
            .run_prompt(RunPromptRequest {
                session_id: session.id.clone(),
                prompt: "make a plan".to_string(),
                attachment_ids: Vec::new(),
                event_tx,
                interaction_callback,
                interaction_emitter,
                options: TurnOptions::default(),
            })
            .await
            .unwrap();
        handle.await.unwrap();

        assert_eq!(outcome.result.status, TurnResultStatus::Completed);
        assert_eq!(outcome.result.content, "Ready");
        let plan_item = outcome
            .timeline_events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemCompleted { item }
                    if item.kind == TimelineItemKind::Plan =>
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
            .expect("completed plan item");
        assert_eq!(plan_item.content, "1. Inspect\\n2. Implement");
        assert!(outcome.timeline_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.text_channel == Some(TimelineTextChannel::Final)
                    && item.content == "Ready"
        )));

        let interaction = store
            .read_interaction(&plan_confirmation_id(&plan_item.item_id))
            .await
            .unwrap()
            .expect("plan confirmation interaction");
        assert_eq!(interaction.kind, InteractionKind::PlanConfirmation);
        assert_eq!(interaction.status, InteractionStatus::Pending);
        assert_eq!(
            interaction.scope.item_id.as_deref(),
            Some(plan_item.item_id.as_str())
        );
        assert_eq!(
            interaction.payload,
            InteractionPayload::PlanConfirmation {
                plan_id: plan_item.item_id.clone(),
                content: plan_item.content.clone(),
            }
        );
        let timeline = store
            .load_timeline_events(&session.id, None, None)
            .await
            .unwrap();
        assert!(timeline.iter().any(|record| {
            serde_json::from_str::<TraceEventKind>(&record.payload_json).is_ok_and(|kind| {
                matches!(
                    kind,
                    TraceEventKind::PlanLifecycleChanged { event }
                        if event.plan_id == plan_item.item_id
                            && event.state == PlanLifecycleState::PendingConfirmation
                )
            })
        }));
        assert_eq!(interaction_events.lock().await.len(), 1);
        let _ = tokio::fs::remove_dir_all(home).await;
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }
}
