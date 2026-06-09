use std::path::Path;

use anyhow::{Context, Result};
use pl_protocol::{AgentEventSender, TimelineItemKind, TraceEvent, TraceEventKind};

use crate::config::{ConfigStore, ModelRole};
use crate::skill::SkillCatalog;
use crate::studio::StudioStore;
use crate::studio::mappers::default_session_runtime_record;
use crate::studio::records::{
    ProjectRecord, SessionRecord, SessionRuntimeRecord, StudioPromptOutcome,
};
use crate::{
    CompileMode, CoreSession, PureCore, ToolApprovalCallback, TraceRecorder, TurnBudget,
    TurnOptions, TurnRequest, TurnResultStatus, load_workspace_instructions,
    resolve_workspace_root,
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

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
}

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        Ok(Self {
            store: StudioStore::default_app().await?,
            config_store: ConfigStore::default_app()?,
        })
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Self {
        Self {
            store,
            config_store,
        }
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.config_store
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

    pub async fn run_prompt(
        &self,
        session_id: &str,
        prompt: String,
        event_tx: AgentEventSender,
        approval_callback: ToolApprovalCallback,
        mut options: TurnOptions,
    ) -> Result<StudioPromptOutcome> {
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
        let mut request = TurnRequest::new(prompt.clone(), mode);
        if !workspace_instructions.trim().is_empty() {
            request = request.with_workspace_instructions(workspace_instructions.clone());
        }

        let mut core = PureCore::from_config(&config, ModelRole::Planner)?;
        core.register_default_tools(workspace_root.clone(), Some(workspace_instructions.clone()));
        if options.tool_approval_callback.is_none()
            && (options.requires_user_approval_callback() || mode == CompileMode::Plan)
        {
            options.tool_approval_callback = Some(approval_callback.clone());
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
            | TraceEventKind::PlanLifecycleChanged { .. } => false,
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
    use pl_protocol::{TimelineItem, TimelineItemStatus};
    use pretty_assertions::assert_eq;

    use super::*;

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
                    role: None,
                    content: String::new(),
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
}
