use pl_core::{
    AgentCommittedEvent, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeHandle,
    AgentSnapshot, AgentState, AgentTurnOutcome, MailboxBudgetAction, ThreadId, TurnId,
};
use pl_trace::{TraceEvent, TraceEventKind};
use tokio::sync::watch;

use crate::StudioAgentDirectoryEntry;
use crate::config::StudioRole;

use crate::studio::ProductEventBus;
use crate::studio::task_coordinator::{
    RecordTaskAgentFailure, TaskCoordinator, TaskIssueDisposition,
};

use super::super::resources::StudioAgentResources;
use super::super::wait_for_runtime;
use super::continuation::submit_executor_continuation;
use super::planner_wake::materialize_pending_task_planner_wakes;

pub(super) struct StudioAgentEventProjector {
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
            | AgentRuntimeEventKind::TurnActivityChanged { snapshot, .. } => {
                self.emit_agent_snapshot(thread_id.as_deref(), *snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::Faulted { snapshot, reason } => {
                let outcome = snapshot
                    .last_turn
                    .clone()
                    .unwrap_or_else(|| synthetic_fault_outcome(&snapshot, &reason));
                let is_task_agent = self.resources.get(&snapshot.identity.id).await.is_some();
                let is_executor =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Executor.key();
                let is_reviewer =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Reviewer.key();
                let disposition = if (is_executor || is_reviewer)
                    && self.planner_is_operational(thread_id.as_deref()).await?
                {
                    TaskIssueDisposition::Recoverable
                } else {
                    // 根计划者已经 Faulted，或者子代理已无健康计划者可以接管。
                    TaskIssueDisposition::Fatal
                };
                let terminalized = self
                    .settle_task_failure(
                        event.agent_id.as_str(),
                        thread_id.as_deref(),
                        &snapshot,
                        &outcome,
                        Some(disposition),
                    )
                    .await?;
                if !terminalized && is_executor {
                    self.coordinator
                        .task_runtime()
                        .fail_faulted_executor(
                            event.agent_id.as_str(),
                            outcome.turn_id.as_str(),
                            outcome
                                .outcome
                                .failure()
                                .map_or(reason.as_str(), |failure| failure.message.as_str()),
                        )
                        .await
                        .at("failFaultedExecutor")?;
                } else if !terminalized && is_reviewer {
                    self.coordinator
                        .task_runtime()
                        .settle_reviewer_turn_finished(event.agent_id.as_str(), &outcome.outcome)
                        .await
                        .at("settleFaultedReviewerTurn")?;
                }
                if !terminalized && (is_executor || is_reviewer) {
                    materialize_pending_task_planner_wakes(
                        &wait_for_runtime(self.runtime.clone())
                            .await
                            .at("waitForRuntimeToWakePlannerAfterFault")?,
                        &self.coordinator.task_runtime(),
                        None,
                    )
                    .await
                    .at("wakePlannerAfterAgentFault")?;
                }
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
                    self.coordinator
                        .task_runtime()
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
                    self.coordinator
                        .task_runtime()
                        .resolve_recoverable_issues(thread_id)
                        .await
                        .at("resolveRecoverableTaskIssue")?;
                }
                let is_task_executor = snapshot.identity.role.as_str()
                    == StudioRole::Executor.key()
                    && self.resources.get(&snapshot.identity.id).await.is_some();
                if is_task_executor {
                    self.coordinator
                        .task_runtime()
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
                let terminalized = self
                    .settle_task_failure(
                        event.agent_id.as_str(),
                        thread_id.as_deref(),
                        &snapshot,
                        &outcome,
                        None,
                    )
                    .await?;
                let is_task_agent = self.resources.get(&snapshot.identity.id).await.is_some();
                let is_executor =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Executor.key();
                let is_reviewer =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Reviewer.key();
                if terminalized {
                    // Fatal failure already terminalized every Task child atomically.
                } else if is_executor {
                    let continuation = self
                        .coordinator
                        .task_runtime()
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
                        self.coordinator
                            .task_runtime()
                            .fail_executor_continuation(&continuation, &error.to_string())
                            .await
                            .at("failExecutorContinuation")?;
                    }
                } else if is_reviewer {
                    self.coordinator
                        .task_runtime()
                        .settle_reviewer_turn_finished(event.agent_id.as_str(), &outcome.outcome)
                        .await
                        .at("settleReviewerTurnFinished")?;
                }
                if !terminalized && (is_executor || is_reviewer) {
                    materialize_pending_task_planner_wakes(
                        &wait_for_runtime(self.runtime.clone())
                            .await
                            .at("waitForRuntimeToWakePlanner")?,
                        &self.coordinator.task_runtime(),
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
        if let Some(mut thread) = self.product_events.thread_snapshot(thread_id) {
            thread.status = super::super::repository::labels::thread_status(&snapshot.state);
            thread.updated_at = thread.updated_at.max(snapshot.updated_at);
            let progress = snapshot.progress.clone();
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
                    state: snapshot.state.clone(),
                    progress,
                    updated_at: snapshot.updated_at,
                    summary_age_seconds,
                })
                .await;
            self.product_events
                .apply_thread_delta(vec![thread], Vec::new())
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

    async fn settle_task_failure(
        &self,
        source_agent_id: &str,
        thread_id: Option<&str>,
        snapshot: &AgentSnapshot,
        outcome: &AgentTurnOutcome,
        disposition: Option<TaskIssueDisposition>,
    ) -> Result<bool, StudioAgentProjectionFailure> {
        let Some(failure) = outcome.outcome.failure().cloned() else {
            return Ok(false);
        };
        let disposition =
            disposition.unwrap_or_else(|| TaskIssueDisposition::for_turn_failure(&failure));
        let Some(thread_id) = thread_id else {
            return Ok(false);
        };
        let Some(thread) = self.product_events.thread_snapshot(thread_id) else {
            return Ok(false);
        };
        self.coordinator
            .handle_agent_turn_failure(
                RecordTaskAgentFailure {
                    root_thread_id: thread.root_thread_id,
                    source_thread_id: thread_id.to_string(),
                    source_turn_id: outcome.turn_id.to_string(),
                    source_agent_id: source_agent_id.to_string(),
                    source_role: snapshot.identity.role.to_string(),
                    failure,
                    disposition,
                },
                &wait_for_runtime(self.runtime.clone())
                    .await
                    .at("waitForRuntimeToSettleFailure")?,
            )
            .await
            .at("settleTaskAgentFailure")
    }

    async fn planner_is_operational(
        &self,
        thread_id: Option<&str>,
    ) -> Result<bool, StudioAgentProjectionFailure> {
        let Some(thread_id) = thread_id else {
            return Ok(false);
        };
        let Some(thread) = self.product_events.thread_snapshot(thread_id) else {
            return Ok(false);
        };
        let planner_id = crate::studio::agent_host::root_agent_id(&thread.root_thread_id);
        let runtime = wait_for_runtime(self.runtime.clone())
            .await
            .at("waitForRuntimeToCheckPlanner")?;
        let planner = match runtime.snapshot(planner_id).await {
            Ok(snapshot) => snapshot,
            Err(pl_core::AgentRuntimeError::NotFound(_)) => return Ok(false),
            Err(error) => {
                return Err(StudioAgentProjectionFailure {
                    stage: "snapshotPlannerForFaultSettlement",
                    source: anyhow::anyhow!(error.to_string()),
                });
            }
        };
        Ok(!matches!(
            planner.state,
            AgentState::Faulted(_) | AgentState::Closing(_) | AgentState::Closed(_)
        ))
    }
}

/// 某些 lifecycle/补偿故障没有可关联的模型 Turn。TaskRuntime 仍必须收到一个
/// 稳定、类型化的终态，不能让 TaskRun 因缺少 `last_turn` 永久悬挂。
fn synthetic_fault_outcome(snapshot: &AgentSnapshot, reason: &str) -> AgentTurnOutcome {
    let turn_id = match &snapshot.state {
        AgentState::Faulted(state) => state.turn_id().cloned(),
        AgentState::Idle(_)
        | AgentState::Queued(_)
        | AgentState::Running(_)
        | AgentState::WaitingTool(_)
        | AgentState::WaitingInteraction(_)
        | AgentState::Cancelling(_)
        | AgentState::Closing(_)
        | AgentState::Closed(_) => None,
    }
    .unwrap_or_else(|| {
        TurnId::new(format!(
            "fault-{}-{}",
            snapshot.identity.id, snapshot.revision
        ))
        .expect("synthetic fault Turn identity is valid")
    });
    let message = match &snapshot.state {
        AgentState::Faulted(state) => state.error().message.clone(),
        AgentState::Idle(_)
        | AgentState::Queued(_)
        | AgentState::Running(_)
        | AgentState::WaitingTool(_)
        | AgentState::WaitingInteraction(_)
        | AgentState::Cancelling(_)
        | AgentState::Closing(_)
        | AgentState::Closed(_) => reason.to_string(),
    };
    AgentTurnOutcome {
        turn_id,
        thread_id: snapshot.identity.id.clone(),
        outcome: pl_protocol::TurnOutcome::failed(pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Internal,
            message,
        )),
        usage: pl_model::TokenUsage::default(),
        started_at: None,
        finished_at: snapshot.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use pl_core::{AgentIdentity, AgentRoleId, FaultedAgentState};
    use pl_protocol::StateError;

    use super::*;

    #[test]
    fn fault_without_diagnostic_turn_still_produces_stable_task_terminal() {
        let snapshot = AgentSnapshot {
            identity: AgentIdentity {
                id: ThreadId::new("agent-fault-without-turn").unwrap(),
                parent_id: None,
                role: AgentRoleId::new("planner").unwrap(),
                depth: 0,
            },
            state: AgentState::Faulted(FaultedAgentState::new(
                StateError {
                    code: "agentRuntimeFault".to_string(),
                    message: "aggregate validation failed".to_string(),
                    retryable: false,
                },
                None,
            )),
            pending_inputs: 0,
            progress: None,
            last_turn: None,
            revision: 7,
            event_sequence: 9,
            updated_at: 11,
        };

        let first = synthetic_fault_outcome(&snapshot, "fallback");
        let second = synthetic_fault_outcome(&snapshot, "fallback");

        assert_eq!(first.turn_id, second.turn_id);
        assert_eq!(first.turn_id.as_str(), "fault-agent-fault-without-turn-7");
        assert_eq!(first.finished_at, 11);
        assert_eq!(
            first.outcome.failure().unwrap().message,
            "aggregate validation failed"
        );
    }
}
