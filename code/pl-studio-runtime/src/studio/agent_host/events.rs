use crate::config::StudioRole;
use crate::{PlanLifecycleState, StudioAgentDirectoryEntry, StudioAgentProgressRuntime};
use pl_core::{
    AgentActivityState, AgentCommitObserver, AgentCommittedEvent, AgentId, AgentLifecycleState,
    AgentProgressStage, AgentRuntimeEventKind, AgentRuntimeHandle, AgentSnapshot,
    AgentSubmitRequest, AgentTurnSubmitPolicy, MailboxPresentation, ThreadId, TurnOutcomeKind,
};
use pl_trace::{TraceEvent, TraceEventKind, TracePartKind};
use tokio::sync::{mpsc, watch};

use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRuntimeState, StudioStore,
};

use super::resources::StudioAgentResources;
use super::{StudioPlanConfirmationProjector, wait_for_runtime};

/// 把已提交的 framework event/trace 投影到 Studio durable event stream。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentCommitObserver {
    sender: mpsc::UnboundedSender<AgentCommittedEvent>,
    runtime: watch::Sender<Option<AgentRuntimeHandle>>,
    store: StudioStore,
    plan_confirmations: StudioPlanConfirmationProjector,
    runtime_state: StudioRuntimeState,
}

struct StudioAgentEventProjector {
    store: StudioStore,
    resources: StudioAgentResources,
    product_events: StudioProductEventRuntime,
    plan_confirmations: StudioPlanConfirmationProjector,
    runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
}

struct StudioAgentProjectionFailure {
    stage: &'static str,
    source: anyhow::Error,
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

impl StudioAgentCommitObserver {
    pub(super) fn new(
        store: StudioStore,
        interactions: InteractionRuntime,
        runtime_state: StudioRuntimeState,
        resources: StudioAgentResources,
        product_events: StudioProductEventRuntime,
    ) -> Self {
        let (runtime, runtime_receiver) = watch::channel(None);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let plan_confirmations = StudioPlanConfirmationProjector::new(
            store.clone(),
            interactions.clone(),
            product_events.clone(),
            runtime_receiver.clone(),
        );
        let projector = StudioAgentEventProjector {
            store: store.clone(),
            resources,
            product_events,
            plan_confirmations: plan_confirmations.clone(),
            runtime: runtime_receiver,
        };
        tokio::spawn(async move {
            while let Some(committed) = receiver.recv().await {
                if let Err(error) = projector.project(committed).await {
                    tracing::warn!(
                        stage = error.stage,
                        error_bytes = error.source.to_string().len(),
                        "failed to project durable Studio agent event"
                    );
                }
            }
        });
        Self {
            sender,
            runtime,
            store,
            plan_confirmations,
            runtime_state,
        }
    }

    pub(super) async fn attach_runtime(&self, runtime: AgentRuntimeHandle) {
        self.runtime.send_replace(Some(runtime.clone()));
        match self.store.list_pending_executor_continuations().await {
            Ok(continuations) => {
                for continuation in continuations {
                    if let Err(error) =
                        recover_executor_continuation(&runtime, &self.store, &continuation).await
                        && let Err(store_error) = self
                            .store
                            .fail_executor_continuation(&continuation, &error.to_string())
                            .await
                    {
                        tracing::warn!(
                            error_bytes = store_error.to_string().len(),
                            "failed to persist executor continuation recovery failure"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(
                error_bytes = error.to_string().len(),
                "failed to list pending executor continuations"
            ),
        }
        if let Err(error) = self.plan_confirmations.recover_missing().await {
            tracing::warn!(
                error_bytes = error.to_string().len(),
                "failed to recover pending plan confirmations"
            );
        }
    }

    pub(super) async fn detach_runtime(&self) {
        self.runtime.send_replace(None);
    }
}

impl AgentCommitObserver for StudioAgentCommitObserver {
    async fn publish(&self, committed: AgentCommittedEvent) {
        self.project_runtime_state(&committed);
        if self.sender.send(committed).is_err() {
            tracing::warn!("Studio agent event projector is no longer running");
        }
    }
}

impl StudioAgentCommitObserver {
    fn project_runtime_state(&self, committed: &AgentCommittedEvent) {
        for event in &committed.runtime_events {
            match &event.kind {
                AgentRuntimeEventKind::TurnQueued { input, .. } => {
                    self.runtime_state
                        .mark_active_turn(input.thread_id.to_string(), input.turn_id.to_string());
                }
                AgentRuntimeEventKind::TurnFinished { outcome, .. }
                | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
                    self.runtime_state
                        .clear_active_turn(outcome.thread_id.as_str(), outcome.turn_id.as_str());
                }
                AgentRuntimeEventKind::Registered { .. }
                | AgentRuntimeEventKind::StateChanged { .. }
                | AgentRuntimeEventKind::TurnStarted { .. }
                | AgentRuntimeEventKind::ThreadOpened { .. }
                | AgentRuntimeEventKind::Faulted { .. } => {}
            }
        }
    }
}

impl StudioAgentEventProjector {
    async fn project(
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
        event: pl_core::AgentRuntimeEvent,
        thread_id: &Option<String>,
    ) -> Result<(), StudioAgentProjectionFailure> {
        match event.kind {
            AgentRuntimeEventKind::Registered { snapshot }
            | AgentRuntimeEventKind::StateChanged { snapshot }
            | AgentRuntimeEventKind::ThreadOpened { snapshot, .. }
            | AgentRuntimeEventKind::Faulted { snapshot, .. } => {
                self.emit_agent_snapshot(thread_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnQueued {
                input: _, snapshot, ..
            } => {
                self.emit_agent_snapshot(thread_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnStarted { snapshot, .. } => {
                let is_task_executor = snapshot.identity.role.as_str()
                    == StudioRole::Executor.key()
                    && self.resources.get(&snapshot.identity.id).await.is_some();
                if is_task_executor {
                    self.store
                        .mark_executor_turn_started(event.agent_id.as_str())
                        .await
                        .at("markExecutorTurnStarted")?;
                }
                self.emit_agent_snapshot(thread_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnFinished {
                outcome, snapshot, ..
            }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, snapshot } => {
                let is_task_agent = self.resources.get(&snapshot.identity.id).await.is_some();
                let is_executor =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Executor.key();
                let is_reviewer =
                    is_task_agent && snapshot.identity.role.as_str() == StudioRole::Reviewer.key();
                if is_executor {
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
                        .settle_reviewer_turn_finished(
                            event.agent_id.as_str(),
                            outcome.kind,
                            outcome.reason.as_deref(),
                        )
                        .await
                        .at("settleReviewerTurnFinished")?;
                }
                if let Some(thread_id) = thread_id.as_deref() {
                    self.project_plan_lifecycle(
                        event.agent_id.as_str(),
                        thread_id,
                        outcome.turn_id.as_str(),
                        outcome.kind,
                        outcome.reason.clone(),
                    )
                    .await
                    .at("projectPlanLifecycle")?;
                }
                self.emit_agent_snapshot(thread_id.as_deref(), snapshot)
                    .await?;
                if is_reviewer {
                    let runtime = wait_for_runtime(self.runtime.clone())
                        .await
                        .at("waitForRuntimeToCloseReviewer")?;
                    let reviewer_id =
                        AgentId::new(event.agent_id.to_string()).at("parseReviewerAgentId")?;
                    runtime.close(reviewer_id).await.at("closeReviewer")?;
                }
            }
        }
        Ok(())
    }

    async fn project_traces(
        &self,
        agent_id: &str,
        thread_id: &str,
        traces: Vec<TraceEvent>,
    ) -> anyhow::Result<()> {
        for trace in traces {
            match trace.kind {
                TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                    self.plan_confirmations
                        .project(agent_id, thread_id, &item)
                        .await?;
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => {}
            }
        }
        Ok(())
    }

    async fn project_plan_lifecycle(
        &self,
        agent_id: &str,
        thread_id: &str,
        turn_id: &str,
        outcome: TurnOutcomeKind,
        reason: Option<String>,
    ) -> anyhow::Result<()> {
        let metadata = self.store.agent_turn_metadata(agent_id, turn_id).await?;
        let Some(lifecycle) = metadata
            .as_ref()
            .and_then(|metadata| metadata.get("planLifecycle"))
            .filter(|value| !value.is_null())
        else {
            return Ok(());
        };
        let Some(plan_id) = lifecycle.get("planId").and_then(serde_json::Value::as_str) else {
            return Ok(());
        };
        let Some((state, reason)) = plan_terminal_projection(outcome, reason) else {
            return Ok(());
        };
        // Plan 本身已经由 Trace 投影为 ThreadItem；终态属于 Task 产品状态，
        // 不再复制成第二条会话事实。
        let _ = (agent_id, thread_id, turn_id, plan_id, state, reason);
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
        let status = status_label(&snapshot);
        let snapshot_error = error(&snapshot);
        let summary = resource.as_ref().map(|resource| resource.task_name.clone());
        self.store
            .update_thread_status(
                thread_id,
                status,
                summary,
                snapshot_error.clone(),
                snapshot.updated_at,
            )
            .await
            .at("updateAgentThreadStatus")?;
        if let Some(thread) = self
            .store
            .read_thread(thread_id)
            .await
            .at("readThreadForAgentDirectory")?
        {
            let progress = snapshot
                .progress
                .as_ref()
                .map(|progress| StudioAgentProgressRuntime {
                    stage: progress_stage_label(progress.stage).to_string(),
                    summary: progress.summary.clone(),
                    next_step: progress.next_step.clone(),
                    revision: progress.revision,
                    updated_at: progress.updated_at,
                });
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
            self.product_events.emit_agent_directory(
                &thread.project_id,
                StudioAgentDirectoryEntry {
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
                    status: status.to_string(),
                    summary: snapshot
                        .progress
                        .as_ref()
                        .map(|progress| progress.summary.clone()),
                    depth: snapshot.identity.depth,
                    error: snapshot_error,
                    reason: snapshot
                        .last_turn
                        .as_ref()
                        .and_then(|outcome| outcome.reason.clone()),
                    lifecycle: lifecycle_label(snapshot.lifecycle).to_string(),
                    activity: activity_label(snapshot.activity).to_string(),
                    progress,
                    updated_at: snapshot.updated_at,
                    summary_age_seconds,
                },
            );
            self.product_events
                .emit_thread_directory(&thread.project_id)
                .await
                .at("emitThreadDirectory")?;
        }
        if snapshot.lifecycle == AgentLifecycleState::Closed {
            self.resources
                .release_after_close(&snapshot.identity.id)
                .await;
        }
        Ok(())
    }
}

async fn submit_executor_continuation(
    runtime: &AgentRuntimeHandle,
    continuation: &crate::studio::task_coordinator::ExecutorContinuationRequest,
) -> anyhow::Result<()> {
    let agent_id = AgentId::new(continuation.agent_id.clone())?;
    let thread_id = ThreadId::new(continuation.agent_id.clone())?;
    let request = AgentSubmitRequest::start(
        thread_id,
        "Continue the assigned task from the compacted canonical session. Re-read current task status, finish the remaining work, verify it, and report completion.",
    )
    .with_presentation(MailboxPresentation::Hidden)
    .with_metadata(serde_json::json!({
        "kind": "executorBudgetContinuation",
        "workUnitId": continuation.work_unit_id,
        "sourceTurnId": continuation.source_turn_id,
        "slice": continuation.slice_count,
    }))
    .with_mail_id(continuation.mail_id())
    .with_turn_policy(AgentTurnSubmitPolicy::StartOnly);
    runtime
        .submit(agent_id, request)
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

async fn recover_executor_continuation(
    runtime: &AgentRuntimeHandle,
    store: &StudioStore,
    continuation: &crate::studio::task_coordinator::ExecutorContinuationRequest,
) -> anyhow::Result<()> {
    if let Some(turn_id) = store.executor_continuation_turn_id(continuation).await? {
        let agent_id = AgentId::new(continuation.agent_id.clone())?;
        let snapshot = runtime.snapshot(agent_id).await?;
        if snapshot
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn_id)
        {
            store
                .mark_executor_turn_started(&continuation.agent_id)
                .await?;
            return Ok(());
        }
        if let Some(outcome) = snapshot
            .last_turn
            .as_ref()
            .filter(|outcome| outcome.turn_id.as_str() == turn_id)
        {
            if let Some(next) = store
                .settle_executor_turn_finished(&continuation.agent_id, outcome)
                .await?
            {
                submit_executor_continuation(runtime, &next).await?;
            }
            return Ok(());
        }
    }
    submit_executor_continuation(runtime, continuation).await
}

fn status_label(snapshot: &AgentSnapshot) -> &'static str {
    match snapshot.lifecycle {
        AgentLifecycleState::Closing | AgentLifecycleState::Closed => "closed",
        AgentLifecycleState::Faulted => "failed",
        AgentLifecycleState::Active => match snapshot.activity {
            AgentActivityState::Queued => "running",
            AgentActivityState::Running => "running",
            AgentActivityState::WaitingTool
            | AgentActivityState::WaitingInteraction
            | AgentActivityState::Cancelling => "waiting",
            AgentActivityState::Idle => "idle",
        },
    }
}

fn error(snapshot: &AgentSnapshot) -> Option<String> {
    snapshot
        .last_turn
        .as_ref()
        .filter(|outcome| outcome.kind == TurnOutcomeKind::Failed)
        .and_then(|outcome| outcome.reason.clone())
}

fn plan_terminal_projection(
    outcome: TurnOutcomeKind,
    reason: Option<String>,
) -> Option<(PlanLifecycleState, Option<String>)> {
    match outcome {
        TurnOutcomeKind::Completed => Some((PlanLifecycleState::Implemented, None)),
        TurnOutcomeKind::Cancelled | TurnOutcomeKind::Failed => {
            Some((PlanLifecycleState::ImplementationFailed, reason))
        }
        TurnOutcomeKind::BudgetLimited => None,
    }
}

const fn lifecycle_label(lifecycle: AgentLifecycleState) -> &'static str {
    match lifecycle {
        AgentLifecycleState::Active => "active",
        AgentLifecycleState::Closing => "closing",
        AgentLifecycleState::Closed => "closed",
        AgentLifecycleState::Faulted => "faulted",
    }
}

const fn activity_label(activity: AgentActivityState) -> &'static str {
    match activity {
        AgentActivityState::Idle => "idle",
        AgentActivityState::Queued => "queued",
        AgentActivityState::Running => "running",
        AgentActivityState::WaitingTool => "waitingTool",
        AgentActivityState::WaitingInteraction => "waitingInteraction",
        AgentActivityState::Cancelling => "cancelling",
    }
}

const fn progress_stage_label(stage: AgentProgressStage) -> &'static str {
    match stage {
        AgentProgressStage::Exploring => "exploring",
        AgentProgressStage::Implementing => "implementing",
        AgentProgressStage::Verifying => "verifying",
        AgentProgressStage::Blocked => "blocked",
        AgentProgressStage::ReadyForCompletion => "readyForCompletion",
        AgentProgressStage::ReadyForReview => "readyForReview",
    }
}

#[cfg(test)]
mod tests {
    use pl_core::{AgentIdentity, AgentRoleId, AgentTurnOutcome, TurnId};

    use super::*;

    #[test]
    fn budget_limited_turn_leaves_thread_idle_without_error() {
        let snapshot = snapshot_with_outcome(TurnOutcomeKind::BudgetLimited);

        assert_eq!(status_label(&snapshot), "idle");
        assert_eq!(error(&snapshot), None);
    }

    #[test]
    fn budget_limited_plan_keeps_implementing_lifecycle() {
        assert_eq!(
            plan_terminal_projection(
                TurnOutcomeKind::BudgetLimited,
                Some("budget reached".to_string()),
            ),
            None
        );
        assert_eq!(
            plan_terminal_projection(TurnOutcomeKind::Failed, Some("failed".to_string())),
            Some((
                PlanLifecycleState::ImplementationFailed,
                Some("failed".to_string()),
            ))
        );
    }

    fn snapshot_with_outcome(kind: TurnOutcomeKind) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new("agent-1").expect("agent id"),
                parent_id: None,
                role: AgentRoleId::new("planner").expect("role id"),
                depth: 0,
            },
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Idle,
            active_turn_id: None,
            pending_inputs: 0,
            progress: None,
            last_turn: Some(AgentTurnOutcome {
                turn_id: TurnId::new("turn-1").expect("turn id"),
                thread_id: pl_core::ThreadId::new("session-1").expect("thread id"),
                kind,
                reason: Some("budget reached".to_string()),
                failure: None,
                budget_limit: None,
                rollover_compacted: false,
                rollover_compaction_error: None,
                usage: Default::default(),
                finished_at: 7,
            }),
            revision: 1,
            event_sequence: 1,
            updated_at: 7,
        }
    }
}
