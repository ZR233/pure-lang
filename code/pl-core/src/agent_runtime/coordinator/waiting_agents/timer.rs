use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::future::pending;
use std::time::Duration;

use tokio::time::Instant;

use super::ParentWaitState;
use super::wake::{WakeParentOutcome, child_needs_wait, wake_parent};
use crate::agent_runtime::{AgentActivityState, AgentId, AgentRuntimeHandle, AgentWakeReason};

pub(super) type TimerEntry = Reverse<(Instant, u64, AgentId, AgentId, u64)>;

pub(super) async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

pub(super) fn arm_all_child_timers(
    runtime: &AgentRuntimeHandle,
    state: &mut ParentWaitState,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
    parent_id: &AgentId,
) {
    for child in runtime.agent_events.children(parent_id) {
        if child_needs_wait(state, &child) {
            arm_child_timer(
                runtime,
                state,
                timers,
                inactivity_timeout,
                parent_id,
                &child.identity.id,
            );
        }
    }
}

pub(super) fn arm_child_timer(
    runtime: &AgentRuntimeHandle,
    state: &mut ParentWaitState,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
    parent_id: &AgentId,
    child_id: &AgentId,
) {
    let Some(child) = runtime
        .agent_events
        .children(parent_id)
        .into_iter()
        .find(|child| child.identity.id == *child_id)
    else {
        return;
    };
    if !child_needs_wait(state, &child) {
        return;
    }
    if state.diagnosed_in_epoch.contains(child_id) {
        return;
    }
    let generation = state
        .timer_generations
        .entry(child_id.clone())
        .and_modify(|value| *value = value.saturating_add(1))
        .or_insert(1);
    let last_update = *state
        .last_meaningful_updates
        .entry(child_id.clone())
        .or_insert_with(Instant::now);
    timers.push(Reverse((
        last_update + inactivity_timeout,
        state.waiting_epoch,
        parent_id.clone(),
        child_id.clone(),
        *generation,
    )));
}

pub(super) async fn handle_due_timers(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
) {
    let now = Instant::now();
    let mut timed_out = BTreeMap::<AgentId, Vec<AgentId>>::new();
    while let Some(Reverse((deadline, epoch, parent_id, child_id, generation))) = timers.peek() {
        if *deadline > now {
            break;
        }
        let epoch = *epoch;
        let parent_id = parent_id.clone();
        let child_id = child_id.clone();
        let generation = *generation;
        timers.pop();
        if runtime.agent_events.parent_wait_is_suspended(&parent_id) {
            parents.remove(&parent_id);
            continue;
        }
        let Some(state) = parents.get(&parent_id) else {
            continue;
        };
        if state.waiting_epoch != epoch
            || state.timer_generations.get(&child_id) != Some(&generation)
            || state.wake_in_flight
            || state.diagnosed_in_epoch.contains(&child_id)
        {
            continue;
        }
        let Ok(parent) = runtime.agent_events.snapshot(&parent_id) else {
            continue;
        };
        let child = runtime
            .agent_events
            .children(&parent_id)
            .into_iter()
            .find(|child| child.identity.id == child_id);
        if parent.activity == AgentActivityState::WaitingAgents
            && child
                .as_ref()
                .is_some_and(|child| child_needs_wait(state, child))
        {
            timed_out.entry(parent_id).or_default().push(child_id);
        }
    }
    for (parent_id, timed_out_agent_ids) in timed_out {
        let wake_outcome = wake_parent(
            runtime,
            parents,
            parent_id.clone(),
            AgentWakeReason::InactivityDiagnostic {
                timed_out_agent_ids: timed_out_agent_ids.clone(),
            },
        )
        .await;
        let Some(state) = parents.get_mut(&parent_id) else {
            continue;
        };
        match wake_outcome {
            WakeParentOutcome::Accepted => {
                state.diagnosed_in_epoch.extend(timed_out_agent_ids);
            }
            WakeParentOutcome::NotAccepted => {
                for child_id in timed_out_agent_ids {
                    rearm_diagnostic_retry(state, timers, inactivity_timeout, &parent_id, child_id);
                }
            }
        }
    }
}

fn rearm_diagnostic_retry(
    state: &mut ParentWaitState,
    timers: &mut BinaryHeap<TimerEntry>,
    inactivity_timeout: Duration,
    parent_id: &AgentId,
    child_id: AgentId,
) {
    let generation = state
        .timer_generations
        .entry(child_id.clone())
        .and_modify(|value| *value = value.saturating_add(1))
        .or_insert(1);
    timers.push(Reverse((
        Instant::now() + inactivity_timeout,
        state.waiting_epoch,
        parent_id.clone(),
        child_id,
        *generation,
    )));
}

pub(super) fn invalidate_timers(state: &mut ParentWaitState) {
    for generation in state.timer_generations.values_mut() {
        *generation = generation.saturating_add(1);
    }
}
