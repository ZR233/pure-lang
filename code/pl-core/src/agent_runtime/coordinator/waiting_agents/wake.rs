use std::collections::BTreeMap;

use super::ParentWaitState;
use super::timer::invalidate_timers;
use crate::agent_runtime::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentId, AgentLifecycleState,
    AgentRuntimeHandle, AgentSnapshot, AgentWakeBatch, AgentWakeId, AgentWakePolicy,
    AgentWakeReason, InputDelivery,
};

const MAX_SEEN_SIGNALS: usize = 4_096;

pub(super) async fn wake_parent(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    parent_id: AgentId,
    reason: AgentWakeReason,
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
    let state = parents.entry(parent_id.clone()).or_default();
    if state.wake_in_flight {
        return;
    }
    let updates = state.pending.values().cloned().collect::<Vec<_>>();
    let wake_key = match &reason {
        AgentWakeReason::Updates => updates
            .iter()
            .map(|update| update.signal_id.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        AgentWakeReason::InactivityTimeout {
            timed_out_agent_ids,
        } => format!(
            "timeout:{}:{}:{}",
            parent.revision,
            state.waiting_epoch,
            timed_out_agent_ids
                .iter()
                .map(AgentId::as_str)
                .collect::<Vec<_>>()
                .join("|")
        ),
    };
    let Ok(wake_id) = AgentWakeId::new(format!("agent-wake:{parent_id}:{wake_key}")) else {
        return;
    };
    let signal_ids = updates
        .iter()
        .map(|update| update.signal_id.clone())
        .collect::<Vec<_>>();
    if runtime
        .wake_accepted(parent_id.clone(), Some(wake_id.clone()), signal_ids.clone())
        .await
        .unwrap_or(false)
    {
        for update in updates {
            state.pending.remove(&update.signal_id);
        }
        return;
    }
    let batch = AgentWakeBatch {
        wake_id: wake_id.clone(),
        parent_agent_id: parent_id.clone(),
        reason,
        updates: updates.clone(),
        children: runtime.agent_events.children(&parent_id),
    };
    let Ok(batch_json) = serde_json::to_string_pretty(&batch) else {
        return;
    };
    let request = AgentCurrentSessionSubmitRequest::start(format!(
        "子代理状态已更新。请根据以下规范快照继续协调；若仍无可执行工作，结束本轮并等待下一次订阅通知。\n\n<agentWakeBatch>\n{batch_json}\n</agentWakeBatch>"
    ))
    .with_delivery(InputDelivery::Start)
    .with_wake_id(wake_id.clone())
    .with_wake_signal_ids(signal_ids)
    .with_metadata(serde_json::json!({
        "agentWakeBatch": batch,
        "agentWakeId": wake_id,
        "attachmentIds": [],
        "historyPolicy": "ephemeral",
        "userPrompt": {
            "visiblePrompt": "子代理状态更新",
            "synthetic": true,
            "ignored": true,
        },
    }));
    match runtime
        .submit_current_session(parent_id.clone(), request)
        .await
    {
        Ok(_) => {
            for update in updates {
                state.pending.remove(&update.signal_id);
            }
            state.wake_in_flight = true;
            invalidate_timers(state);
        }
        Err(error) => {
            tracing::warn!(parent_agent_id = %parent_id, %error, "failed to wake parent agent");
        }
    }
}

pub(super) fn remember_signal(state: &mut ParentWaitState, signal_id: &str) -> bool {
    if !state.seen_signals.insert(signal_id.to_string()) {
        return false;
    }
    state.seen_order.push_back(signal_id.to_string());
    while state.seen_order.len() > MAX_SEEN_SIGNALS {
        if let Some(expired) = state.seen_order.pop_front() {
            state.seen_signals.remove(&expired);
        }
    }
    true
}

pub(super) fn has_live_children(
    runtime: &AgentRuntimeHandle,
    state: &ParentWaitState,
    parent_id: &AgentId,
) -> bool {
    runtime
        .agent_events
        .children(parent_id)
        .iter()
        .any(|child| child_needs_wait(state, child))
}

pub(super) fn child_needs_wait(state: &ParentWaitState, snapshot: &AgentSnapshot) -> bool {
    snapshot.lifecycle == AgentLifecycleState::Active
        && match snapshot.wake_policy {
            AgentWakePolicy::RuntimeTerminal => snapshot.activity != AgentActivityState::Idle,
            AgentWakePolicy::ProductGated => {
                !state.product_terminal.contains(&snapshot.identity.id)
            }
        }
}
