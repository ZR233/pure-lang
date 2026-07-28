use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::time::Duration;

use tokio::time::Instant;

mod timer;
mod wake;
use timer::{
    TimerEntry, arm_all_child_timers, arm_child_timer, handle_due_timers, invalidate_timers,
    wait_for_deadline,
};
use wake::{has_live_children, remember_signal, wake_parent};

use super::super::{
    AgentActivityState, AgentId, AgentLifecycleState, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentSnapshot, AgentUpdateEnvelope, AgentUpdateKind, AgentWakePolicy,
    AgentWakeReason, TurnId, TurnOutcomeKind,
};

#[derive(Default)]
struct ParentWaitState {
    pending: BTreeMap<String, AgentUpdateEnvelope>,
    pending_turns: BTreeMap<String, TurnId>,
    seen_signals: BTreeSet<String>,
    seen_order: VecDeque<String>,
    last_meaningful_updates: BTreeMap<AgentId, Instant>,
    timer_generations: BTreeMap<AgentId, u64>,
    product_terminal: BTreeSet<AgentId>,
    waiting_epoch: u64,
    wake_in_flight: bool,
}

impl ParentWaitState {
    fn remove_pending(&mut self, signal_id: &str) {
        self.pending.remove(signal_id);
        self.pending_turns.remove(signal_id);
    }
}

pub(super) fn spawn_waiting_agents_supervisor(
    runtime: AgentRuntimeHandle,
    inactivity_timeout: Duration,
) {
    tokio::spawn(async move {
        let mut updates = runtime.agent_events.subscribe_all();
        let mut runtime_events = runtime.agent_events.subscribe_runtime();
        let mut parents = BTreeMap::<AgentId, ParentWaitState>::new();
        let mut timers = BinaryHeap::<TimerEntry>::new();
        restore_waiting_parents(&runtime, &mut parents, &mut timers, inactivity_timeout);
        reconcile_restored_parents(&runtime, &mut parents, &mut timers, inactivity_timeout).await;

        loop {
            let deadline = timers.peek().map(|entry| (entry.0).0);
            tokio::select! {
                update = updates.recv() => match update {
                    Ok(update) => {
                        handle_update(
                            &runtime,
                            &mut parents,
                            &mut timers,
                            inactivity_timeout,
                            update,
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        reconcile_after_lag(
                            &runtime,
                            &mut parents,
                            &mut timers,
                            inactivity_timeout,
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                event = runtime_events.recv() => match event {
                    Ok(event) => {
                        handle_runtime_event(
                            &runtime,
                            &mut parents,
                            &mut timers,
                            inactivity_timeout,
                            event,
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        reconcile_after_lag(
                            &runtime,
                            &mut parents,
                            &mut timers,
                            inactivity_timeout,
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                () = wait_for_deadline(deadline) => {
                    handle_due_timers(&runtime, &mut parents, &mut timers).await;
                }
            }
        }
    });
}

async fn handle_update(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
    update: AgentUpdateEnvelope,
) {
    if !update_triggers_parent_wake(&update.kind) {
        return;
    }
    let parent_id = update.parent_agent_id.clone();
    let child_id = update.agent_id.clone();
    if runtime
        .wake_accepted(parent_id.clone(), None, vec![update.signal_id.clone()])
        .await
        .unwrap_or(false)
    {
        return;
    }
    let Ok(parent) = runtime.agent_events.snapshot(&parent_id) else {
        return;
    };
    let active_turn_id = parent.active_turn_id.clone();
    let state = parents.entry(parent_id.clone()).or_default();
    if !remember_signal(state, &update.signal_id) {
        return;
    }
    if matches!(update.kind, AgentUpdateKind::ProductPhaseChanged { .. }) {
        state.product_terminal.insert(child_id.clone());
    }
    state
        .last_meaningful_updates
        .insert(child_id.clone(), Instant::now());
    if let Some(turn_id) = active_turn_id {
        state
            .pending_turns
            .insert(update.signal_id.clone(), turn_id);
    }
    state.pending.insert(update.signal_id.clone(), update);

    if parent.activity == AgentActivityState::WaitingAgents {
        arm_child_timer(
            runtime,
            state,
            timers,
            inactivity_timeout,
            &parent_id,
            &child_id,
        );
    }
    if parent.lifecycle == AgentLifecycleState::Active
        && matches!(
            parent.activity,
            AgentActivityState::Idle | AgentActivityState::WaitingAgents
        )
    {
        wake_parent(runtime, parents, parent_id, AgentWakeReason::Updates).await;
    }
}

fn update_triggers_parent_wake(kind: &AgentUpdateKind) -> bool {
    matches!(
        kind,
        AgentUpdateKind::ProgressReported
            | AgentUpdateKind::TodoPhaseChanged
            | AgentUpdateKind::NeedsAttention
            | AgentUpdateKind::RuntimeTerminal { .. }
            | AgentUpdateKind::ProductPhaseChanged { .. }
    )
}

async fn handle_runtime_event(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
    event: AgentRuntimeEvent,
) {
    let snapshot = event_snapshot(&event);
    let parent_id = snapshot.identity.id.clone();
    let state = parents.entry(parent_id.clone()).or_default();
    if snapshot.activity != AgentActivityState::WaitingAgents {
        invalidate_timers(state);
    }
    match event.kind {
        AgentRuntimeEventKind::TurnFinished {
            outcome,
            finalized_with_tool,
            ..
        } => {
            state.wake_in_flight = false;
            if outcome.kind == TurnOutcomeKind::Completed && finalized_with_tool.is_some() {
                accept_finalized_updates(runtime, parents, &parent_id, outcome.turn_id).await;
            }
            settle_parent(runtime, parents, parent_id).await;
        }
        AgentRuntimeEventKind::RecoveryCancelledTurn { .. } => {
            state.wake_in_flight = false;
            settle_parent(runtime, parents, parent_id).await;
        }
        AgentRuntimeEventKind::StateChanged { snapshot }
            if snapshot.activity == AgentActivityState::WaitingAgents =>
        {
            let state = parents.entry(parent_id.clone()).or_default();
            state.waiting_epoch = state.waiting_epoch.saturating_add(1);
            arm_all_child_timers(runtime, state, timers, inactivity_timeout, &parent_id);
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::TurnQueued { .. }
        | AgentRuntimeEventKind::TurnStarted { .. }
        | AgentRuntimeEventKind::SessionOpened { .. }
        | AgentRuntimeEventKind::Faulted { .. } => {}
    }
}

async fn accept_finalized_updates(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    parent_id: &AgentId,
    turn_id: TurnId,
) {
    let Some(state) = parents.get(parent_id) else {
        return;
    };
    let signal_ids = finalized_signal_ids(state, &turn_id);
    if signal_ids.is_empty() {
        return;
    }
    match runtime
        .accept_wake_signals(parent_id.clone(), turn_id, signal_ids.clone())
        .await
    {
        Ok(()) => {
            let state = parents.entry(parent_id.clone()).or_default();
            for signal_id in signal_ids {
                state.remove_pending(&signal_id);
            }
        }
        Err(error) => {
            tracing::warn!(
                parent_agent_id = %parent_id,
                %error,
                "failed to accept child updates at finalization barrier"
            );
        }
    }
}

fn finalized_signal_ids(state: &ParentWaitState, turn_id: &TurnId) -> Vec<String> {
    state
        .pending_turns
        .iter()
        .filter(|(_, pending_turn_id)| *pending_turn_id == turn_id)
        .map(|(signal_id, _)| signal_id.clone())
        .collect()
}

async fn settle_parent(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    parent_id: AgentId,
) {
    let Ok(parent) = runtime.agent_events.snapshot(&parent_id) else {
        return;
    };
    if parent.lifecycle != AgentLifecycleState::Active
        || !matches!(
            parent.activity,
            AgentActivityState::Idle | AgentActivityState::WaitingAgents
        )
    {
        return;
    }
    if parents
        .get(&parent_id)
        .is_some_and(|state| !state.pending.is_empty())
    {
        wake_parent(runtime, parents, parent_id, AgentWakeReason::Updates).await;
        return;
    }
    let state = parents.entry(parent_id.clone()).or_default();
    if has_live_children(runtime, state, &parent_id) {
        let _ = runtime.enter_waiting_agents(parent_id).await;
    }
}

fn restore_waiting_parents(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
) {
    for snapshot in runtime.agent_events.snapshots() {
        if snapshot.lifecycle == AgentLifecycleState::Active
            && snapshot.activity == AgentActivityState::WaitingAgents
        {
            let parent_id = snapshot.identity.id;
            let state = parents.entry(parent_id.clone()).or_default();
            state.waiting_epoch = state.waiting_epoch.saturating_add(1);
            arm_all_child_timers(runtime, state, timers, inactivity_timeout, &parent_id);
        }
    }
}

async fn reconcile_after_lag(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
) {
    for child in runtime.agent_events.snapshots() {
        let Some(parent_id) = child.identity.parent_id.clone() else {
            continue;
        };
        if child.wake_policy == AgentWakePolicy::ProductGated {
            continue;
        }
        let kind = if child.wake_policy == AgentWakePolicy::RuntimeTerminal
            && child.activity == AgentActivityState::Idle
            && child.last_turn.is_some()
        {
            AgentUpdateKind::RuntimeTerminal {
                outcome: child.last_turn.clone(),
            }
        } else {
            AgentUpdateKind::ActivityChanged {
                activity: child.activity,
            }
        };
        handle_update(
            runtime,
            parents,
            timers,
            inactivity_timeout,
            AgentUpdateEnvelope {
                signal_id: format!(
                    "stale:{}:{}:{}",
                    child.identity.id, child.revision, child.event_sequence
                ),
                parent_agent_id: parent_id,
                agent_id: child.identity.id.clone(),
                agent_revision: child.revision,
                event_sequence: child.event_sequence,
                occurred_at: child.updated_at,
                kind,
                snapshot: child,
                summary: Some("subscription lagged; canonical child snapshot reloaded".to_string()),
            },
        )
        .await;
    }

    let parent_snapshots = runtime.agent_events.snapshots();
    for snapshot in &parent_snapshots {
        if snapshot.lifecycle == AgentLifecycleState::Active
            && snapshot.pending_inputs == 0
            && matches!(
                snapshot.activity,
                AgentActivityState::Idle | AgentActivityState::WaitingAgents
            )
            && let Some(state) = parents.get_mut(&snapshot.identity.id)
        {
            state.wake_in_flight = false;
        }
    }
    let idle_parents = parent_snapshots
        .into_iter()
        .filter(|snapshot| {
            snapshot.lifecycle == AgentLifecycleState::Active
                && matches!(
                    snapshot.activity,
                    AgentActivityState::Idle | AgentActivityState::WaitingAgents
                )
        })
        .map(|snapshot| snapshot.identity.id)
        .collect::<Vec<_>>();
    for parent_id in idle_parents {
        settle_parent(runtime, parents, parent_id).await;
    }
}

async fn reconcile_restored_parents(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
) {
    for child in runtime.agent_events.snapshots() {
        if child.wake_policy != AgentWakePolicy::RuntimeTerminal
            || child.lifecycle != AgentLifecycleState::Active
            || child.activity != AgentActivityState::Idle
            || child.last_turn.is_none()
        {
            continue;
        }
        let Some(parent_id) = child.identity.parent_id.clone() else {
            continue;
        };
        let Ok(parent) = runtime.agent_events.snapshot(&parent_id) else {
            continue;
        };
        if parent.pending_inputs != 0
            || !matches!(
                parent.activity,
                AgentActivityState::Idle | AgentActivityState::WaitingAgents
            )
        {
            continue;
        }
        handle_update(
            runtime,
            parents,
            timers,
            inactivity_timeout,
            AgentUpdateEnvelope {
                signal_id: format!("runtime:{}:{}", child.identity.id, child.event_sequence),
                parent_agent_id: parent_id,
                agent_id: child.identity.id.clone(),
                agent_revision: child.revision,
                event_sequence: child.event_sequence,
                occurred_at: child.updated_at,
                kind: AgentUpdateKind::RuntimeTerminal {
                    outcome: child.last_turn.clone(),
                },
                snapshot: child,
                summary: Some("runtime restored a terminal direct-child snapshot".to_string()),
            },
        )
        .await;
    }

    let idle_parents = runtime
        .agent_events
        .snapshots()
        .into_iter()
        .filter(|snapshot| {
            snapshot.lifecycle == AgentLifecycleState::Active
                && snapshot.pending_inputs == 0
                && matches!(
                    snapshot.activity,
                    AgentActivityState::Idle | AgentActivityState::WaitingAgents
                )
        })
        .map(|snapshot| snapshot.identity.id)
        .collect::<Vec<_>>();
    for parent_id in idle_parents {
        settle_parent(runtime, parents, parent_id).await;
    }
}

fn event_snapshot(event: &AgentRuntimeEvent) -> AgentSnapshot {
    match &event.kind {
        AgentRuntimeEventKind::Registered { snapshot }
        | AgentRuntimeEventKind::StateChanged { snapshot }
        | AgentRuntimeEventKind::TurnQueued { snapshot, .. }
        | AgentRuntimeEventKind::TurnStarted { snapshot, .. }
        | AgentRuntimeEventKind::SessionOpened { snapshot, .. }
        | AgentRuntimeEventKind::TurnFinished { snapshot, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { snapshot, .. }
        | AgentRuntimeEventKind::Faulted { snapshot, .. } => snapshot.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::AgentRoleId;

    use super::*;

    #[test]
    fn finalized_signal_ids_only_returns_signals_observed_during_that_turn() {
        let turn_1 = TurnId::new("turn-1").unwrap();
        let turn_2 = TurnId::new("turn-2").unwrap();
        let mut state = ParentWaitState::default();
        state
            .pending_turns
            .insert("signal-a".to_string(), turn_1.clone());
        state.pending_turns.insert("signal-b".to_string(), turn_2);
        state
            .pending
            .insert("signal-c".to_string(), pending_update("signal-c"));

        assert_eq!(finalized_signal_ids(&state, &turn_1), vec!["signal-a"]);
    }

    fn pending_update(signal_id: &str) -> AgentUpdateEnvelope {
        let agent_id = AgentId::new("child").unwrap();
        AgentUpdateEnvelope {
            signal_id: signal_id.to_string(),
            parent_agent_id: AgentId::new("parent").unwrap(),
            agent_id: agent_id.clone(),
            agent_revision: 1,
            event_sequence: 1,
            occurred_at: 1,
            kind: AgentUpdateKind::ProgressReported,
            snapshot: AgentSnapshot {
                identity: super::super::super::AgentIdentity {
                    id: agent_id,
                    parent_id: Some(AgentId::new("parent").unwrap()),
                    role: AgentRoleId::new("worker").unwrap(),
                    depth: 1,
                },
                wake_policy: AgentWakePolicy::RuntimeTerminal,
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                active_session_id: None,
                pending_inputs: 0,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: 1,
            },
            summary: None,
        }
    }
}
