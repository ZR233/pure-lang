use pl_core::{
    AgentCommittedEvent, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeHandle,
    AgentSnapshot, AgentState, MailboxBudgetAction, ThreadId,
};
use pl_trace::{TraceEvent, TraceEventKind};
use tokio::sync::watch;

use crate::config::StudioRole;
use crate::{StudioAgentDirectoryEntry, StudioAgentProgressRuntime};

use crate::studio::task_coordinator::RecordTaskAgentFailure;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{ProductEventBus, StudioStore};

use super::super::resources::StudioAgentResources;
use super::super::wait_for_runtime;
use super::continuation::submit_executor_continuation;
use super::mapping::studio_agent_state;
use super::planner_wake::materialize_pending_task_planner_wakes;

pub(super) struct StudioAgentEventProjector {
    pub(super) store: StudioStore,
    pub(super) resources: StudioAgentResources,
    pub(super) product_events: ProductEventBus,
    pub(super) coordinator: std::sync::Arc<TaskCoordinator>,
    pub(super) runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
}

pub(super) struct StudioAgentProjectionFailure {
    pub(super) stage: &'static str,
    pub(super) source: anyhow::Error,
}

/// Adds a non-sensitive stage label while preserving the original projection error.
trait ProjectionResultExt<T> {
    fn at(self, stage: &'static str) -> Result<T, StudioAgentProjectionFailure>;
}

impl<T, E> ProjectionResultExt<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn at(self, stage: &'static str) -> Result<T, StudioAgentProjectionFailure> {
        self.map_err(|source| StudioAgentProjectionFailure {
            stage,
            source: source.into(),
        })
    }
}

impl StudioAgentEventProjector {
    pub(super) async fn project(
        &self,
        committed: AgentCommittedEvent,
    ) -> Result<(), StudioAgentProjectionFailure> {
        let AgentCommittedEvent {
            agent_id,
            runtime_events,
            trace_events,
            ..
        } = committed;
        let thread_id = self.resources.thread_id(&agent_id).await;
        if let Some(thread_id) = thread_id.as_deref()
            && !trace_events.is_empty()
        {
            self.project_traces(agent_id.as_str(), thread_id, trace_events)
                .await
                .at("traceEvents")?;
        }
        for event in runtime_events {
            self.project_runtime_event(event, &thread_id).await?;
        }
        if let Some(thread_id) = thread_id
            && let Some(thread) = self
                .store
                .read_thread(&thread_id)
                .await
                .at("readThreadForTaskRefresh")?
        {
            self.product_events
                .refresh_task(&thread.root_thread_id)
                .await
                .at("refreshThreadTask")?;
        }
        Ok(())
    }

    async fn project_runtime_event(
        &self,
        event: AgentRuntimeEvent,
        thread_id: &Option<String>,
    ) -> Result<(), StudioAgentProjectionFailure> {
        match event.kind {
            AgentRuntimeEventKind::Registered { snapshot }
            | AgentRuntimeEventKind::StateChanged { snapshot }
            | AgentRuntimeEventKind::ThreadOpened { snapshot, .. }
            | AgentRuntimeEventKind::TurnActivityChanged { snapshot, .. }
            | AgentRuntimeEventKind::Faulted { snapshot, .. } => {
                self.emit_agent_snapshot(thread_id.as_deref(), *snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnQueued {
                input, snapshot, ..
            } => {
                let is_task_executor = snapshot.identity.role.as_str()
                    == StudioRole::Executor.key()
                    && self.resources.get(&snapshot.identity.id).await.is_some();
                if is_task_executor && input.budget_action == MailboxBudgetAction::Refresh {
                    self.store
                        .mark_executor_turn_started(
                            event.agent_id.as_str(),
                            input.turn_id.as_str(),
                            MailboxBudgetAction::Refresh,
                        )
                        .await
                        .at("refreshExecutorBudgetTranche")?;
                }
                self.emit_agent_snapshot(thread_id.as_deref(), *snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnStarted {
                turn_id,
                input,
                snapshot,
                ..
            } => {
                if let Some(thread_id) = thread_id.as_deref() {
                    self.store
                        .resolve_recoverable_task_issues(thread_id)
                        .await
                        .at("resolveRecoverableTaskIssue")?;
                }
                let is_task_executor = snapshot.identity.role.as_str()
                    == StudioRole::Executor.key()
                    && self.resources.get(&snapshot.identity.id).await.is_some();
                if is_task_executor {
                    self.store
                        .mark_executor_turn_started(
                            event.agent_id.as_str(),
                            turn_id.as_str(),
                            input.budget_action,
                        )
                        .await
                        .at("markExecutorTurnStarted")?;
                }
                self.emit_agent_snapshot(thread_id.as_deref(), *snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnFinished {
                outcome, snapshot, ..
            }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, snapshot } => {
                let terminalized = if let (Some(failure), Some(thread_id)) =
                    (outcome.outcome.failure().cloned(), thread_id.as_deref())
                {
                    let thread = self
                        .store
                        .read_thread(thread_id)
                        .await
                        .at("readFailureThread")?;
                    if let Some(thread) = thread {
                        self.coordinator
                            .handle_agent_turn_failure(
                                RecordTaskAgentFailure {
                                    root_thread_id: thread.root_thread_id,
                                    source_thread_id: thread_id.to_string(),
                                    source_turn_id: outcome.turn_id.to_string(),
                                    source_agent_id: event.agent_id.to_string(),
                                    source_role: snapshot.identity.role.to_string(),
                                    failure,
                                },
                                &wait_for_runtime(self.runtime.clone())
                                    .await
                                    .at("waitForRuntimeToSettleFailure")?,
                            )
                            .await
                            .at("settleTaskAgentFailure")?
                    } else {
                        false
                    }
                } else {
                    false
                };
                let is_task_agent = self.resources.get(&snapshot.identity.id).await.is_some();
                let is_executor =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Executor.key();
                let is_reviewer =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Reviewer.key();
                if terminalized {
                    // Fatal failure already terminalized every Task child atomically.
                } else if is_executor {
                    let continuation = self
                        .store
                        .settle_executor_turn_finished(event.agent_id.as_str(), &outcome)
                        .await
                        .at("settleExecutorTurnFinished")?;
                    if let Some(continuation) = continuation
                        && let Err(error) = submit_executor_continuation(
                            &wait_for_runtime(self.runtime.clone())
                                .await
                                .at("waitForRuntimeToContinueExecutor")?,
                            &continuation,
                        )
                        .await
                    {
                        self.store
                            .fail_executor_continuation(&continuation, &error.to_string())
                            .await
                            .at("failExecutorContinuation")?;
                    }
                } else if is_reviewer {
                    self.store
                        .settle_reviewer_turn_finished(event.agent_id.as_str(), &outcome.outcome)
                        .await
                        .at("settleReviewerTurnFinished")?;
                }
                if !terminalized && (is_executor || is_reviewer) {
                    materialize_pending_task_planner_wakes(
                        &wait_for_runtime(self.runtime.clone())
                            .await
                            .at("waitForRuntimeToWakePlanner")?,
                        &self.store,
                        None,
                    )
                    .await
                    .at("recoverTaskPlannerWakes")?;
                }
                self.emit_agent_snapshot(thread_id.as_deref(), *snapshot)
                    .await?;
                if is_reviewer {
                    let runtime = wait_for_runtime(self.runtime.clone())
                        .await
                        .at("waitForRuntimeToCloseReviewer")?;
                    let reviewer_id =
                        ThreadId::new(event.agent_id.to_string()).at("parseReviewerAgentId")?;
                    runtime.close(reviewer_id).await.at("closeReviewer")?;
                }
            }
        }
        Ok(())
    }

    async fn project_traces(
        &self,
        _agent_id: &str,
        _thread_id: &str,
        traces: Vec<TraceEvent>,
    ) -> anyhow::Result<()> {
        for trace in traces {
            match trace.kind {
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => {}
            }
        }
        Ok(())
    }

    async fn emit_agent_snapshot(
        &self,
        thread_id: Option<&str>,
        snapshot: AgentSnapshot,
    ) -> Result<(), StudioAgentProjectionFailure> {
        let Some(thread_id) = thread_id else {
            return Ok(());
        };
        let resource = self.resources.get(&snapshot.identity.id).await;
        if let Some(thread) = self
            .store
            .read_thread(thread_id)
            .await
            .at("readThreadForAgentDirectory")?
        {
            let progress = snapshot
                .progress
                .as_ref()
                .map(StudioAgentProgressRuntime::from);
            let summary_age_seconds = u64::try_from(
                crate::studio::ids::unix_seconds()
                    .saturating_sub(
                        snapshot
                            .progress
                            .as_ref()
                            .map_or(snapshot.updated_at, |progress| progress.updated_at),
                    )
                    .max(0),
            )
            .unwrap_or_default();
            self.product_events
                .update_agent_directory(StudioAgentDirectoryEntry {
                    id: snapshot.identity.id.to_string(),
                    thread_id: thread.id.clone(),
                    root_thread_id: thread.root_thread_id.clone(),
                    path: snapshot.identity.id.to_string(),
                    parent_path: snapshot
                        .identity
                        .parent_id
                        .as_ref()
                        .map(ToString::to_string),
                    role: snapshot.identity.role.to_string(),
                    task: resource.as_ref().map_or_else(
                        || thread.title.clone(),
                        |resource| resource.task_name.clone(),
                    ),
                    summary: snapshot
                        .progress
                        .as_ref()
                        .map(|progress| progress.report.summary.clone()),
                    depth: snapshot.identity.depth,
                    state: studio_agent_state(&snapshot.state),
                    progress,
                    updated_at: snapshot.updated_at,
                    summary_age_seconds,
                })
                .await;
            // 目录走内存索引增量：这里 upsert 刚重读的 canonical Thread 行。
            let directory_thread: pl_protocol::Thread = thread.into();
            self.product_events
                .apply_thread_delta(vec![directory_thread], Vec::new())
                .await
                .at("emitThreadDirectory")?;
        }
        if matches!(snapshot.state, AgentState::Closed(_)) {
            self.resources
                .release_after_close(&snapshot.identity.id)
                .await;
        }
        Ok(())
    }
}
