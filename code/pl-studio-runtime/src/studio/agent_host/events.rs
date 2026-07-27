use crate::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionScope, InteractionStatus,
    PlanLifecycleEvent, PlanLifecycleState, SessionEventFact, SessionEventKind,
};
use anyhow::Context;
use pl_core::{
    AgentActivityState, AgentCommitObserver, AgentCommittedEvent, AgentLifecycleState,
    AgentRuntimeEventKind, AgentRuntimeHandle, AgentSnapshot, SessionId, TurnOutcomeKind,
};
use pl_trace::{TraceEvent, TraceEventKind, TracePart, TracePartKind};
use tokio::sync::{mpsc, watch};

use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRuntimeState, StudioStore,
};

use super::StudioContinuationService;
use super::resources::StudioAgentResources;

/// 把已提交的 framework event/trace 投影到 Studio durable event stream。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentCommitObserver {
    sender: mpsc::UnboundedSender<AgentCommittedEvent>,
    runtime: watch::Sender<Option<AgentRuntimeHandle>>,
    runtime_state: StudioRuntimeState,
}

struct StudioAgentEventProjector {
    store: StudioStore,
    interactions: InteractionRuntime,
    resources: StudioAgentResources,
    continuations: StudioContinuationService,
    product_events: StudioProductEventRuntime,
    runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
}

impl StudioAgentCommitObserver {
    pub(super) fn new(
        store: StudioStore,
        interactions: InteractionRuntime,
        runtime_state: StudioRuntimeState,
        resources: StudioAgentResources,
        continuations: StudioContinuationService,
        product_events: StudioProductEventRuntime,
    ) -> Self {
        let (runtime, runtime_receiver) = watch::channel(None);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let projector = StudioAgentEventProjector {
            store,
            interactions,
            resources,
            continuations,
            product_events,
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
            runtime_state,
        }
    }

    pub(super) async fn attach_runtime(&self, runtime: AgentRuntimeHandle) {
        self.runtime.send_replace(Some(runtime));
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
            session_events: _,
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
                self.emit_agent_snapshot(studio_session_id.as_deref(), snapshot)
                    .await?;
            }
            AgentRuntimeEventKind::TurnFinished { outcome, snapshot }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, snapshot } => {
                if let Some(session_id) = studio_session_id.as_deref() {
                    // continuation 是 durable task orchestration，不得被后续 trace/UI
                    // projection 的非关键失败截断。
                    if snapshot.identity.parent_id.is_some()
                        && self
                            .is_canonical_child_terminal(&snapshot, outcome.turn_id.as_str())
                            .await?
                    {
                        let root_session_id = self
                            .store
                            .read_session(session_id)
                            .await?
                            .context("child agent Studio session not found")?
                            .root_session_id;
                        self.continuations
                            .record_child_terminal(
                                &root_session_id,
                                snapshot.identity.id.as_str(),
                                snapshot.identity.role.as_str(),
                                outcome.session_id.as_str(),
                                outcome.kind,
                                outcome.reason.clone(),
                            )
                            .await;
                    } else {
                        self.continuations.request_merge_follow_up(session_id).await;
                    }
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
            }
        }
        Ok(())
    }

    async fn is_canonical_child_terminal(
        &self,
        event_snapshot: &AgentSnapshot,
        turn_id: &str,
    ) -> anyhow::Result<bool> {
        if !snapshot_is_canonical_terminal(event_snapshot, turn_id) {
            return Ok(false);
        }
        let runtime = wait_for_runtime(self.runtime.clone()).await?;
        let latest = runtime
            .snapshot(event_snapshot.identity.id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(snapshot_is_canonical_terminal(&latest, turn_id))
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
                    self.project_plan_confirmation(agent_id, studio_session_id, &item)
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

    async fn project_plan_confirmation(
        &self,
        agent_id: &str,
        studio_session_id: &str,
        plan: &TracePart,
    ) -> anyhow::Result<()> {
        if plan.content.trim().is_empty() {
            return Ok(());
        }
        let metadata = self
            .store
            .agent_turn_metadata(agent_id, &plan.turn_id)
            .await?;
        if metadata
            .as_ref()
            .and_then(|metadata| metadata.get("historyPolicy"))
            .and_then(serde_json::Value::as_str)
            == Some("ephemeral")
        {
            return Ok(());
        }
        let interaction_id = format!("plan-confirmation-{}", plan.item_id);
        if self
            .store
            .read_interaction(&interaction_id)
            .await?
            .is_some()
        {
            return Ok(());
        }
        let now = crate::studio::ids::unix_seconds();
        self.record_facts(
            agent_id,
            studio_session_id,
            vec![SessionEventFact::durable(
                Some(agent_id.to_string()),
                Some(plan.turn_id.clone()),
                now,
                SessionEventKind::PlanChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan.item_id.clone(),
                        state: PlanLifecycleState::PendingConfirmation,
                        turn_id: Some(plan.turn_id.clone()),
                        reason: None,
                        updated_at: now,
                    },
                },
            )],
        )
        .await?;
        let interaction = InteractionRequest {
            interaction_id,
            kind: InteractionKind::PlanConfirmation,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: studio_session_id.to_string(),
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
        let runtime = self.runtime.clone();
        let session_id = studio_session_id.to_string();
        let owner_agent_id = agent_id.to_string();
        let emitter: crate::studio::InteractionEmitter = std::sync::Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            let owner_agent_id = owner_agent_id.clone();
            Box::pin(async move {
                let runtime = wait_for_runtime(runtime).await?;
                let target_agent = pl_core::AgentId::new(owner_agent_id)?;
                let target_session = SessionId::new(session_id)?;
                tokio::spawn(async move {
                    if let Err(error) = runtime
                        .record_session_facts(
                            target_agent,
                            target_session,
                            vec![SessionEventFact::durable(
                                None,
                                Some(interaction.scope.turn_id.clone()),
                                interaction.updated_at,
                                SessionEventKind::InteractionChanged {
                                    event: Box::new(crate::InteractionChangedEvent { interaction }),
                                },
                            )],
                        )
                        .await
                    {
                        tracing::warn!("failed to record Studio interaction fact: {error}");
                    }
                });
                Ok(())
            })
        });
        self.interactions.create(interaction, emitter).await?;
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
            self.product_events
                .emit_session_list(&session.project_id)
                .await?;
        }
        if snapshot.lifecycle == AgentLifecycleState::Closed {
            self.resources.remove(&snapshot.identity.id).await;
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

async fn wait_for_runtime(
    mut runtime: watch::Receiver<Option<AgentRuntimeHandle>>,
) -> anyhow::Result<AgentRuntimeHandle> {
    loop {
        if let Some(runtime) = runtime.borrow_and_update().clone() {
            return Ok(runtime);
        }
        runtime
            .changed()
            .await
            .map_err(|_| anyhow::anyhow!("Studio agent runtime attachment channel closed"))?;
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
            | AgentActivityState::WaitingAgents => "waiting",
            AgentActivityState::Idle => match snapshot.last_turn.as_ref().map(|turn| turn.kind) {
                Some(TurnOutcomeKind::Completed) => "completed",
                Some(TurnOutcomeKind::Cancelled) => "interrupted",
                Some(TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited) => "errored",
                None => "idle",
            },
        },
    }
}

fn snapshot_is_canonical_terminal(snapshot: &AgentSnapshot, turn_id: &str) -> bool {
    snapshot.activity == AgentActivityState::Idle
        && snapshot.active_turn_id.is_none()
        && snapshot.pending_inputs == 0
        && snapshot
            .last_turn
            .as_ref()
            .is_some_and(|outcome| outcome.turn_id.as_str() == turn_id)
}

#[cfg(test)]
mod tests {
    use pl_core::{
        AgentId, AgentIdentity, AgentLifecycleState, AgentRoleId, AgentTurnOutcome, SessionId,
        TurnId,
    };
    use pl_model::TokenUsage;

    use super::*;

    #[test]
    fn queued_follow_up_is_not_a_canonical_terminal() {
        let mut snapshot = snapshot(AgentActivityState::Queued, 1, "turn-1");
        snapshot.active_turn_id = None;

        assert!(!snapshot_is_canonical_terminal(&snapshot, "turn-1"));
    }

    #[test]
    fn idle_queue_drained_snapshot_is_a_canonical_terminal() {
        let snapshot = snapshot(AgentActivityState::Idle, 0, "turn-1");

        assert!(snapshot_is_canonical_terminal(&snapshot, "turn-1"));
    }

    #[test]
    fn stale_terminal_event_does_not_match_latest_turn() {
        let snapshot = snapshot(AgentActivityState::Idle, 0, "turn-2");

        assert!(!snapshot_is_canonical_terminal(&snapshot, "turn-1"));
    }

    fn snapshot(
        activity: AgentActivityState,
        pending_inputs: usize,
        last_turn_id: &str,
    ) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new("agent-1").unwrap(),
                parent_id: Some(AgentId::new("root").unwrap()),
                role: AgentRoleId::new("executor").unwrap(),
                depth: 1,
            },
            wake_policy: pl_core::AgentWakePolicy::ProductGated,
            lifecycle: AgentLifecycleState::Active,
            activity,
            active_turn_id: None,
            active_session_id: None,
            pending_inputs,
            last_turn: Some(AgentTurnOutcome {
                turn_id: TurnId::new(last_turn_id).unwrap(),
                session_id: SessionId::new("session-1").unwrap(),
                kind: TurnOutcomeKind::Cancelled,
                reason: Some("cancelled".to_string()),
                failure: None,
                usage: TokenUsage::default(),
                finished_at: 1,
            }),
            revision: 1,
            event_sequence: 1,
            updated_at: 1,
        }
    }
}
