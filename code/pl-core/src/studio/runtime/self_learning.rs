use anyhow::Result;
use pl_trace::{TraceEvent, TraceEventKind, TracePartKind};

use crate::config::ModelRole;
use crate::{
    CompileMode, CoreSession, PureCore, TraceRecorder, TurnBudget, TurnOptions, TurnRequest,
    TurnResultStatus,
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

pub(super) fn should_start_self_learning(
    config: &crate::config::PureConfig,
    mode: CompileMode,
    status: &TurnResultStatus,
    trace_events: &[TraceEvent],
) -> bool {
    mode == CompileMode::Simple
        && config.skills.enabled
        && config.skills.auto_learn
        && matches!(status, TurnResultStatus::Completed)
        && tool_call_count(trace_events) >= config.skills.auto_learn_min_tool_calls
}

pub(super) fn spawn_self_learning_review(
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
    let request = TurnRequest::new(SELF_LEARNING_REVIEW_PROMPT.to_string(), CompileMode::Simple)
        .with_workspace_instructions(workspace_instructions)
        .with_budget(TurnBudget::new(120_000));
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::disabled(event_tx);
    let _ = core
        .run_turn_with_trace(&mut session, request, &mut recorder, TurnOptions::default())
        .await?;
    Ok(())
}

pub(super) fn tool_call_count(trace_events: &[TraceEvent]) -> u32 {
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
pub(super) fn started_tool_snapshot_count(trace_events: &[TraceEvent]) -> u32 {
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
