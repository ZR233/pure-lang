use anyhow::Result;
use pl_protocol::{
    ErrorSeverity, InteractionRequest, StudioAgentSnapshot, StudioEventKind, StudioSessionRuntime,
};
use pl_trace::AgentEvent;
use tokio::sync::broadcast::error::RecvError;

use crate::agent::AgentTerminalStateChange;
use crate::config::ConfigStore;
use crate::mcp::McpRuntimeRegistry;
use crate::studio::StudioStore;
use crate::studio::active_turns::StudioActiveTurns;
use crate::studio::records::SessionRecord;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionEmitter, InteractionRuntime, StudioEventRuntime, StudioRuntimeState,
    StudioRuntimeStatus,
};
use crate::{InteractionCallback, TurnOptions};

mod continuation;
mod lifecycle;
mod mcp_health;
mod plan_confirmation;
mod projection;
mod prompt_runner;
mod self_learning;
mod session_service;

use continuation::{ContinuationReason, ContinuationScheduler, SharedContinuationLauncher};
use projection::{studio_agent_snapshot, studio_session_runtime};

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
    pub(crate) history_policy: PromptHistoryPolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PromptHistoryPolicy {
    #[default]
    Persist,
    Ephemeral,
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
    mcp_health_watcher: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionRuntime,
    events: StudioEventRuntime,
    runtime_state: StudioRuntimeState,
    active_turns: StudioActiveTurns,
    task_coordinator: std::sync::Arc<TaskCoordinator>,
    lifecycle_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    lifecycle_epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
    post_turn_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    continuation_scheduler: ContinuationScheduler,
    continuation_launcher: Option<SharedContinuationLauncher>,
    #[cfg(test)]
    continuation_request_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    continuation_pre_submit_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    continuation_post_lifecycle_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    continuation_launch_error_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    prompt_finalization_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    active_turn_removal_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    shutdown_entry_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    shutdown_after_cancel_barrier: Option<continuation::ContinuationTestBarrier>,
    #[cfg(test)]
    initialization_entry_barrier: Option<std::sync::Arc<tokio::sync::Barrier>>,
}

impl StudioRuntime {
    fn lifecycle_epoch(&self) -> u64 {
        self.lifecycle_epoch
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn advance_lifecycle_epoch(&self) -> u64 {
        self.lifecycle_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }
}

impl StudioRuntime {
    pub async fn drain_agent_events(
        &self,
        session_id: String,
        event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    ) {
        self.drain_agent_events_inner(session_id, None, event_rx)
            .await;
    }

    async fn drain_prompt_agent_events(
        &self,
        session_id: String,
        turn_id: String,
        event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    ) {
        self.drain_agent_events_inner(session_id, Some(turn_id), event_rx)
            .await;
    }

    async fn drain_agent_events_inner(
        &self,
        session_id: String,
        turn_id: Option<String>,
        mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    ) {
        loop {
            match event_rx.recv().await {
                Ok(mut event) => {
                    let _post_turn_guard = if turn_id.is_some() {
                        Some(self.post_turn_lock.lock().await)
                    } else {
                        None
                    };
                    let visible = match turn_id.as_deref() {
                        Some(turn_id) => {
                            matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready)
                                && self.active_turns.contains_exact(&session_id, turn_id).await
                        }
                        None => true,
                    };
                    let mut terminal_projection = None;
                    if let AgentEvent::AgentStateChanged {
                        id,
                        role,
                        status,
                        summary,
                        error,
                        ..
                    } = &event
                    {
                        let change = AgentTerminalStateChange {
                            agent_id: id.clone(),
                            role: role.clone(),
                            status: *status,
                            summary: summary.clone(),
                            error: error.clone(),
                        };
                        let recording = match self
                            .task_coordinator
                            .record_terminal_agent_state(&session_id, &change)
                            .await
                        {
                            Ok(recording) => recording,
                            Err(error) => {
                                let diagnostic = format!(
                                    "terminal agent state persistence failed for {id}: {error}"
                                );
                                let _ = self
                                    .task_coordinator
                                    .block_terminal_persistence_failure(
                                        &session_id,
                                        &error.to_string(),
                                    )
                                    .await;
                                if visible {
                                    let _ = self
                                        .events
                                        .emit_agent_event(
                                            &session_id,
                                            AgentEvent::Error {
                                                message: diagnostic,
                                                severity: ErrorSeverity::Recoverable,
                                            },
                                        )
                                        .await;
                                }
                                continue;
                            }
                        };
                        terminal_projection = match recording {
                            crate::studio::task_coordinator::TerminalAgentStateRecording::Changed {
                                task_run_id,
                                projection,
                            } => {
                                self.request_task_continuation(
                                    task_run_id,
                                    ContinuationReason::AgentTerminal,
                                )
                                .await;
                                Some(projection)
                            }
                            crate::studio::task_coordinator::TerminalAgentStateRecording::Projected(
                                projection,
                            ) => Some(projection),
                            crate::studio::task_coordinator::TerminalAgentStateRecording::Unhandled => {
                                None
                            }
                            crate::studio::task_coordinator::TerminalAgentStateRecording::Suppressed => {
                                continue;
                            }
                        };
                    }
                    if !visible {
                        continue;
                    }
                    if let Some(projection) = terminal_projection
                        && let AgentEvent::AgentStateChanged {
                            status,
                            summary,
                            error,
                            ..
                        } = &mut event
                    {
                        *status = projection.status;
                        *summary = projection.summary;
                        *error = projection.error;
                    }
                    if let AgentEvent::SubAgentActivity {
                        agent_id: Some(agent_id),
                        status: Some(_),
                        ..
                    } = &event
                    {
                        let projection = match self
                            .task_coordinator
                            .project_agent_activity(&session_id, agent_id)
                            .await
                        {
                            Ok(projection) => projection,
                            Err(error) => {
                                let _ = self
                                    .events
                                    .emit_agent_event(
                                        &session_id,
                                        AgentEvent::Error {
                                            message: format!(
                                                "durable agent activity projection failed for {agent_id}: {error}"
                                            ),
                                            severity: ErrorSeverity::Recoverable,
                                        },
                                    )
                                    .await;
                                continue;
                            }
                        };
                        if let Some(projection) = projection
                            && let AgentEvent::SubAgentActivity { status, error, .. } = &mut event
                        {
                            *status = Some(projection.status);
                            *error = projection.error;
                        }
                    }
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
                    let _post_turn_guard = if turn_id.is_some() {
                        Some(self.post_turn_lock.lock().await)
                    } else {
                        None
                    };
                    let visible = match turn_id.as_deref() {
                        Some(turn_id) => {
                            matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready)
                                && self.active_turns.contains_exact(&session_id, turn_id).await
                        }
                        None => true,
                    };
                    if visible {
                        let _ = self.events.emit_stale(&session_id, skipped).await;
                    }
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
            | AgentEvent::SubAgentActivity { .. }
            | AgentEvent::TodoListUpdated { .. }
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
}

#[cfg(test)]
mod tests;
