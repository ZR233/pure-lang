use crate::config::StudioRole;
use crate::{
    PlanLifecycleEvent, PlanLifecycleState, SessionEventFact, SessionEventKind,
    StudioAgentDirectoryEntry, StudioAgentProgressRuntime,
};
use pl_core::{
    AgentActivityState, AgentCommitObserver, AgentCommittedEvent, AgentId, AgentLifecycleState,
    AgentProgressStage, AgentRuntimeEventKind, AgentRuntimeHandle, AgentSnapshot, SessionId,
    TurnOutcomeKind,
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
            store,
            resources,
            product_events,
            plan_confirmations: plan_confirmations.clone(),
            runtime: runtime_receiver,
        };
        tokio::spawn(async move {
            while let Some(committed) = receiver.recv().await {
                if let Err(error) = projector.project(committed).await {
                    tracing::warn!("failed to project durable Studio agent event: {error:#}");
                }
            }
        });
        Self {
            sender,
            runtime,
            plan_confirmations,
            runtime_state,
        }
    }

    pub(super) async fn attach_runtime(&self, runtime: AgentRuntimeHandle) {
        self.runtime.send_replace(Some(runtime));
        if let Err(error) = self.plan_confirmations.recover_missing().await {
            tracing::warn!("failed to recover pending plan confirmations: {error:#}");
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
                        .mark_active_turn(input.session_id.to_string(), input.turn_id.to_string());
                }
                AgentRuntimeEventKind::TurnFinished { outcome, .. }
                | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
                    self.runtime_state
                        .clear_active_turn(outcome.session_id.as_str(), outcome.turn_id.as_str());
                }
                AgentRuntimeEventKind::Registered { .. }
                | AgentRuntimeEventKind::StateChanged { .. }
                | AgentRuntimeEventKind::TurnStarted { .. }
                | AgentRuntimeEventKind::SessionOpened { .. }
                | AgentRuntimeEventKind::Faulted { .. } => {}
            }
        }
    }
}

impl StudioAgentEventProjector {
    async fn project(&self, committed: AgentCommittedEvent) -> anyhow::Result<()> {
        let AgentCommittedEvent {
            agent_id,
            runtime_events,
            trace_events,
            ..
        } = committed;
        let studio_session_id = self.resources.studio_session_id(&agent_id).await;
        if let Some(session_id) = studio_session_id.as_deref()
            && !trace_events.is_empty()
        {
            self.project_traces(agent_id.as_str(), session_id, trace_events)
                .await?;
        }
        for event in runtime_events {
            self.project_runtime_event(event, &studio_session_id)
                .await?;
        }
        if let Some(session_id) = studio_session_id
            && let Some(session) = self.store.read_session(&session_id).await?
        {
            self.product_events
                .refresh_session_task(&session.root_session_id)
                .await?;
        }
        Ok(())
    }

    async fn project_runtime_event(
        &self,
        event: pl_core::AgentRuntimeEvent,
        studio_session_id: &Option<String>,
    ) -> anyhow::Result<()> {
        match event.kind {
            AgentRuntimeEventKind::Registered { snapshot }
            | AgentRuntimeEventKind::StateChanged { snapshot }
            | AgentRuntimeEventKind::SessionOpened { snapshot, .. }
            | AgentRuntimeEventKind::Faulted { snapshot, .. } => {
                self.emit_agent_snapshot(studio_session_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnQueued {
                input: _, snapshot, ..
            } => {
                self.emit_agent_snapshot(studio_session_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnStarted { snapshot, .. } => {
                if snapshot.identity.role.as_str() == StudioRole::Executor.key() {
                    self.store
                        .mark_executor_turn_started(event.agent_id.as_str())
                        .await?;
                }
                self.emit_agent_snapshot(studio_session_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnFinished {
                outcome, snapshot, ..
            }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, snapshot } => {
                let is_executor = snapshot.identity.role.as_str() == StudioRole::Executor.key();
                let is_reviewer = snapshot.identity.role.as_str() == StudioRole::Reviewer.key();
                if is_executor {
                    self.store
                        .settle_executor_turn_finished(
                            event.agent_id.as_str(),
                            outcome.kind,
                            outcome.reason.as_deref(),
                        )
                        .await?;
                } else if is_reviewer {
                    self.store
                        .settle_reviewer_turn_finished(
                            event.agent_id.as_str(),
                            outcome.kind,
                            outcome.reason.as_deref(),
                        )
                        .await?;
                }
                if let Some(session_id) = studio_session_id.as_deref() {
                    self.project_plan_lifecycle(
                        event.agent_id.as_str(),
                        session_id,
                        outcome.turn_id.as_str(),
                        outcome.kind,
                        outcome.reason.clone(),
                    )
                    .await?;
                }
                self.emit_agent_snapshot(studio_session_id.as_deref(), snapshot)
                    .await?;
                if is_reviewer {
                    let runtime = wait_for_runtime(self.runtime.clone()).await?;
                    runtime
                        .close(AgentId::new(event.agent_id.to_string())?)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn project_traces(
        &self,
        agent_id: &str,
        studio_session_id: &str,
        traces: Vec<TraceEvent>,
    ) -> anyhow::Result<()> {
        for trace in traces {
            match trace.kind {
                TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                    self.plan_confirmations
                        .project(agent_id, studio_session_id, &item)
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
        studio_session_id: &str,
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
        let (state, reason) = match outcome {
            TurnOutcomeKind::Completed => (PlanLifecycleState::Implemented, None),
            TurnOutcomeKind::Cancelled
            | TurnOutcomeKind::Failed
            | TurnOutcomeKind::BudgetLimited => (PlanLifecycleState::ImplementationFailed, reason),
        };
        let updated_at = crate::studio::ids::unix_seconds();
        self.record_facts(
            agent_id,
            studio_session_id,
            vec![SessionEventFact::durable(
                Some(agent_id.to_string()),
                Some(turn_id.to_string()),
                updated_at,
                SessionEventKind::PlanChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan_id.to_string(),
                        state,
                        turn_id: Some(turn_id.to_string()),
                        reason,
                        updated_at,
                    },
                },
            )],
        )
        .await?;
        Ok(())
    }

    async fn emit_agent_snapshot(
        &self,
        studio_session_id: Option<&str>,
        snapshot: AgentSnapshot,
    ) -> anyhow::Result<()> {
        let Some(session_id) = studio_session_id else {
            return Ok(());
        };
        let resource = self.resources.get(&snapshot.identity.id).await;
        let status = agent_status_label(&snapshot);
        let error = snapshot
            .last_turn
            .as_ref()
            .filter(|outcome| {
                matches!(
                    outcome.kind,
                    TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited
                )
            })
            .and_then(|outcome| outcome.reason.clone());
        let summary = resource.as_ref().map(|resource| resource.task_name.clone());
        self.store
            .update_agent_session_status(session_id, status, summary, error, snapshot.updated_at)
            .await?;
        if let Some(session) = self.store.read_session(session_id).await? {
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
                &session.project_id,
                StudioAgentDirectoryEntry {
                    id: snapshot.identity.id.to_string(),
                    session_id: session.id.clone(),
                    root_session_id: session.root_session_id.clone(),
                    path: snapshot.identity.id.to_string(),
                    parent_path: snapshot
                        .identity
                        .parent_id
                        .as_ref()
                        .map(ToString::to_string),
                    role: snapshot.identity.role.to_string(),
                    task: resource.as_ref().map_or_else(
                        || session.title.clone(),
                        |resource| resource.task_name.clone(),
                    ),
                    status: status.to_string(),
                    summary: snapshot
                        .progress
                        .as_ref()
                        .map(|progress| progress.summary.clone()),
                    depth: snapshot.identity.depth,
                    error: snapshot
                        .last_turn
                        .as_ref()
                        .filter(|outcome| {
                            matches!(
                                outcome.kind,
                                TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited
                            )
                        })
                        .and_then(|outcome| outcome.reason.clone()),
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
                .emit_session_list(&session.project_id)
                .await?;
        }
        if snapshot.lifecycle == AgentLifecycleState::Closed {
            self.resources
                .release_after_close(&snapshot.identity.id)
                .await;
        }
        Ok(())
    }

    async fn record_facts(
        &self,
        agent_id: &str,
        session_id: &str,
        facts: Vec<SessionEventFact>,
    ) -> anyhow::Result<()> {
        let runtime = wait_for_runtime(self.runtime.clone()).await?;
        runtime
            .record_session_facts(
                pl_core::AgentId::new(agent_id.to_string())?,
                SessionId::new(session_id.to_string())?,
                facts,
            )
            .await
            .map_err(Into::into)
    }
}

fn agent_status_label(snapshot: &AgentSnapshot) -> &'static str {
    match snapshot.lifecycle {
        AgentLifecycleState::Closing | AgentLifecycleState::Closed => "shutdown",
        AgentLifecycleState::Faulted => "errored",
        AgentLifecycleState::Active => match snapshot.activity {
            AgentActivityState::Queued => "queued",
            AgentActivityState::Running => "running",
            AgentActivityState::WaitingTool
            | AgentActivityState::WaitingInteraction
            | AgentActivityState::Cancelling => "waiting",
            AgentActivityState::Idle => match snapshot.last_turn.as_ref().map(|turn| turn.kind) {
                Some(TurnOutcomeKind::Completed) => "completed",
                Some(TurnOutcomeKind::Cancelled) => "interrupted",
                Some(TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited) => "errored",
                None => "idle",
            },
        },
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
        AgentProgressStage::ReadyForReview => "readyForReview",
    }
}
