use std::collections::BTreeMap;

use pl_protocol::{
    RuntimeCostAmount, ThreadItem, ThreadItemContent, ThreadItemStatus, ThreadNotification,
    ThreadNotificationEnvelope, ThreadRuntimeSnapshot, ThreadRuntimeUsage, ThreadSnapshot,
};
use pl_trace::AgentEvent;

use crate::ContextCompactionSnapshot;

use super::projector::ThreadProjectionBatch;

#[derive(Debug, Clone)]
pub(crate) struct ObservedTurnEvent {
    pub(crate) turn_id: String,
    pub(crate) thread_id: String,
    pub(crate) observation: TurnObservation,
}

#[derive(Debug, Clone)]
pub(crate) enum TurnObservation {
    DirectoryChanged,
    RuntimeDelta(pl_protocol::AgentRuntimeDelta),
    TodoList(pl_protocol::TodoListSnapshot),
    ContextCompacted {
        before_tokens: u64,
        after_tokens: u64,
        compacted_at: i64,
    },
    Diagnostic,
}

pub(crate) fn observation_from_agent_event(event: &AgentEvent) -> Option<TurnObservation> {
    match event {
        AgentEvent::AgentStateChanged { .. } | AgentEvent::SubAgentActivity { .. } => {
            Some(TurnObservation::DirectoryChanged)
        }
        AgentEvent::AgentRuntimeUpdated { delta } => {
            Some(TurnObservation::RuntimeDelta(delta.clone()))
        }
        AgentEvent::TodoListUpdated { snapshot } => {
            Some(TurnObservation::TodoList(snapshot.clone()))
        }
        AgentEvent::TurnInterrupted { .. } | AgentEvent::Error { .. } => {
            Some(TurnObservation::Diagnostic)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done => None,
    }
}

pub(crate) fn compaction_observation(
    snapshot: &ContextCompactionSnapshot,
    compacted_at: i64,
) -> TurnObservation {
    TurnObservation::ContextCompacted {
        before_tokens: snapshot.tokens_before,
        after_tokens: snapshot
            .replacement_tokens
            .unwrap_or(snapshot.estimated_request_tokens),
        compacted_at,
    }
}

pub(crate) fn project_observation(
    thread_id: &str,
    turn_id: &str,
    revision: u64,
    current: &ThreadSnapshot,
    observation: TurnObservation,
) -> ThreadProjectionBatch {
    let (emitted_at, notification) = match observation {
        TurnObservation::RuntimeDelta(delta) => {
            let emitted_at = delta.updated_at;
            (
                emitted_at,
                Some(ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime_snapshot(thread_id, current, delta)),
                }),
            )
        }
        TurnObservation::TodoList(todo) => {
            let emitted_at = unix_timestamp();
            let mut runtime = current
                .runtime
                .clone()
                .unwrap_or_else(|| empty_runtime(thread_id));
            runtime.todo = Some(todo);
            runtime.updated_at = emitted_at;
            (
                emitted_at,
                Some(ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime),
                }),
            )
        }
        TurnObservation::ContextCompacted {
            before_tokens,
            after_tokens,
            compacted_at,
        } => (
            compacted_at,
            Some(ThreadNotification::ItemCompleted {
                item: Box::new(ThreadItem {
                    id: format!("{turn_id}:context-compaction:{compacted_at}"),
                    thread_id: thread_id.to_string(),
                    turn_id: turn_id.to_string(),
                    ordinal: current
                        .items
                        .iter()
                        .map(|item| item.ordinal)
                        .max()
                        .unwrap_or_default()
                        .saturating_add(1),
                    revision: 0,
                    status: ThreadItemStatus::Completed,
                    created_at: compacted_at,
                    updated_at: compacted_at,
                    completed_at: Some(compacted_at),
                    error: None,
                    content: ThreadItemContent::ContextCompaction {
                        before_tokens,
                        after_tokens,
                        compacted_at,
                    },
                    usage: None,
                }),
            }),
        ),
        TurnObservation::DirectoryChanged | TurnObservation::Diagnostic => (unix_timestamp(), None),
    };
    let notifications = notification
        .map(|notification| {
            vec![ThreadNotificationEnvelope {
                thread_id: thread_id.to_string(),
                revision: revision.saturating_add(1),
                emitted_at,
                notification,
            }]
        })
        .unwrap_or_default();
    ThreadProjectionBatch {
        through_revision: revision.saturating_add(u64::from(!notifications.is_empty())),
        notifications,
    }
}

fn runtime_snapshot(
    thread_id: &str,
    current: &ThreadSnapshot,
    delta: pl_protocol::AgentRuntimeDelta,
) -> ThreadRuntimeSnapshot {
    let prior = current.runtime.as_ref().map(|runtime| &runtime.usage);
    let prompt_tokens = prior.map_or(0, |usage| usage.prompt_tokens) + delta.usage.prompt_tokens;
    let cached_prompt_tokens =
        prior.map_or(0, |usage| usage.cached_prompt_tokens) + delta.usage.cached_prompt_tokens;
    let completion_tokens =
        prior.map_or(0, |usage| usage.completion_tokens) + delta.usage.completion_tokens;
    let total_tokens = prior.map_or(0, |usage| usage.total_tokens) + delta.usage.total_tokens;
    let previous = current.runtime.as_ref();
    ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: delta.model,
            context_window: delta.context_window,
            latest_context_tokens: delta.usage.total_tokens,
            prompt_tokens,
            completion_tokens,
            cached_prompt_tokens,
            total_tokens,
            cache_hit_rate: (prompt_tokens > 0)
                .then_some(cached_prompt_tokens as f64 / prompt_tokens as f64),
            estimated_costs: merge_costs(
                prior.map_or(&[], |usage| usage.estimated_costs.as_slice()),
                &delta.estimated_costs,
            ),
            has_unpriced_usage: prior.is_some_and(|usage| usage.has_unpriced_usage)
                || delta.has_unpriced_usage,
            updated_at: delta.updated_at,
        },
        todo: previous.and_then(|runtime| runtime.todo.clone()),
        active_skills: previous.map_or_else(Vec::new, |runtime| runtime.active_skills.clone()),
        active_mcp_servers: previous
            .map_or_else(Vec::new, |runtime| runtime.active_mcp_servers.clone()),
        active_lsp_servers: previous
            .map_or_else(Vec::new, |runtime| runtime.active_lsp_servers.clone()),
        progress: previous.and_then(|runtime| runtime.progress.clone()),
        mcp_health: previous.and_then(|runtime| runtime.mcp_health.clone()),
        updated_at: delta.updated_at,
    }
}

fn empty_runtime(thread_id: &str) -> ThreadRuntimeSnapshot {
    ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: String::new(),
            context_window: None,
            latest_context_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_prompt_tokens: 0,
            total_tokens: 0,
            cache_hit_rate: None,
            estimated_costs: Vec::new(),
            has_unpriced_usage: false,
            updated_at: 0,
        },
        todo: None,
        active_skills: Vec::new(),
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        updated_at: 0,
    }
}

fn merge_costs(left: &[RuntimeCostAmount], right: &[RuntimeCostAmount]) -> Vec<RuntimeCostAmount> {
    let mut totals = BTreeMap::<String, f64>::new();
    for cost in left.iter().chain(right) {
        *totals.entry(cost.currency.clone()).or_default() += cost.amount;
    }
    totals
        .into_iter()
        .map(|(currency, amount)| RuntimeCostAmount { currency, amount })
        .collect()
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
