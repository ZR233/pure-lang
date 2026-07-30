use std::collections::BTreeMap;

use super::ParentWaitState;
use super::timer::invalidate_timers;
use crate::agent_runtime::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentId, AgentLifecycleState,
    AgentRuntimeHandle, AgentSnapshot, AgentUpdateKind, AgentWakeBatch, AgentWakeContext,
    AgentWakeId, AgentWakePolicy, AgentWakeReason, InputDelivery,
};

const MAX_SEEN_SIGNALS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WakeParentOutcome {
    Accepted,
    NotAccepted,
}

pub(super) async fn wake_parent(
    runtime: &AgentRuntimeHandle,
    parents: &mut BTreeMap<AgentId, ParentWaitState>,
    parent_id: AgentId,
    reason: AgentWakeReason,
) -> WakeParentOutcome {
    let Ok(parent) = runtime.agent_events.snapshot(&parent_id) else {
        return WakeParentOutcome::NotAccepted;
    };
    if parent.lifecycle != AgentLifecycleState::Active
        || !matches!(
            parent.activity,
            AgentActivityState::Idle | AgentActivityState::WaitingAgents
        )
    {
        return WakeParentOutcome::NotAccepted;
    }
    let state = parents.entry(parent_id.clone()).or_default();
    if state.wake_in_flight {
        return WakeParentOutcome::NotAccepted;
    }
    let updates = state.pending.values().cloned().collect::<Vec<_>>();
    let wake_key = match &reason {
        AgentWakeReason::Updates => updates
            .iter()
            .map(|update| update.signal_id.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        AgentWakeReason::InactivityDiagnostic {
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
        return WakeParentOutcome::NotAccepted;
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
            state.remove_pending(&update.signal_id);
        }
        return WakeParentOutcome::Accepted;
    }
    let children = runtime.agent_events.children(&parent_id);
    let diagnostic_only =
        matches!(&reason, AgentWakeReason::InactivityDiagnostic { .. }) && updates.is_empty();
    let terminal_facts = updates
        .iter()
        .filter(|update| {
            matches!(
                update.kind,
                AgentUpdateKind::RuntimeTerminal { .. }
                    | AgentUpdateKind::ProductPhaseChanged { .. }
            )
        })
        .cloned()
        .collect();
    let context = AgentWakeContext {
        current_agent_states: children.clone(),
        wake_reason: reason.clone(),
        last_activity_at: state.last_activity_at.clone(),
        recent_progress: state.recent_progress.iter().cloned().collect(),
        latest_commentary: state.latest_commentary.clone(),
        terminal_facts,
        user_stop_requested: false,
        signal_revision: parent.revision,
        lag_reconciled: state.lag_reconciled,
        diagnostic_only,
    };
    let batch = AgentWakeBatch {
        wake_id: wake_id.clone(),
        parent_agent_id: parent_id.clone(),
        reason,
        updates: updates.clone(),
        children,
        context,
    };
    let Ok(batch_json) = serde_json::to_string_pretty(&batch) else {
        return WakeParentOutcome::NotAccepted;
    };
    let request = AgentCurrentSessionSubmitRequest::start(format!(
        "子代理状态已更新。只把 needsAttention、真实终态或 durable product phase 视为可执行事实。\
普通 progress/commentary 与 inactivityDiagnostic 都不表示失败、完成或停止授权，不得仅据此\
中断执行者或调用 task_stop。请使用 typed wake context 判断；若仍无可执行工作，结束本轮并\
等待下一次订阅通知。\n\n<agentWakeBatch>\n{batch_json}\n</agentWakeBatch>"
    ))
    .with_delivery(InputDelivery::Start)
    .with_presentation(super::super::super::MailboxPresentation::SyntheticHidden)
    .with_wake_id(wake_id.clone())
    .with_wake_signal_ids(signal_ids)
    .with_metadata(serde_json::json!({
        "agentWakeBatch": batch,
        "agentWakeId": wake_id,
        "attachmentIds": [],
        "historyPolicy": "ephemeral",
    }));
    match runtime
        .submit_current_session(parent_id.clone(), request)
        .await
    {
        Ok(_) => {
            for update in updates {
                state.remove_pending(&update.signal_id);
            }
            state.wake_in_flight = true;
            state.lag_reconciled = false;
            invalidate_timers(state);
            WakeParentOutcome::Accepted
        }
        Err(error) => {
            tracing::warn!(parent_agent_id = %parent_id, %error, "failed to wake parent agent");
            WakeParentOutcome::NotAccepted
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
