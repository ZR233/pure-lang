use std::collections::BTreeMap;

use pl_protocol::{
    ErrorSeverity, RuntimeCostAmount, SessionAgentSnapshot, SessionContextCompaction,
    SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionRuntimeSnapshot,
    SessionRuntimeUsage, SessionTimelineEvent, SessionTimelineEventKind, SessionViewSnapshot,
};
use pl_trace::AgentEvent;

use crate::ContextCompactionSnapshot;

use super::projector::SessionEventProjectionBatch;

#[derive(Debug, Clone)]
pub(crate) struct ObservedTurnEvent {
    pub(crate) turn_id: String,
    pub(crate) session_id: String,
    pub(crate) observation: TurnObservation,
}

#[derive(Debug, Clone)]
pub(crate) enum TurnObservation {
    AgentState {
        id: String,
        path: String,
        parent_path: Option<String>,
        role: String,
        task: String,
        status: pl_protocol::AgentStatus,
        summary: Option<String>,
        depth: u32,
        error: Option<String>,
        reason: Option<String>,
        budget_limit_kind: Option<pl_protocol::BudgetLimitKind>,
        budget_usage: Option<pl_protocol::BudgetUsage>,
        updated_at: i64,
    },
    RuntimeDelta(pl_protocol::AgentRuntimeDelta),
    SubAgentActivity {
        call_id: String,
        occurred_at: i64,
        agent_id: Option<String>,
        path: Option<String>,
        parent_path: Option<String>,
        kind: pl_protocol::SubAgentActivityKind,
        status: Option<pl_protocol::AgentStatus>,
        message: Option<String>,
        timed_out: Option<bool>,
        error: Option<String>,
    },
    TodoList(pl_protocol::TodoListSnapshot),
    ContextCompacted(SessionContextCompaction),
    Error {
        message: String,
        severity: ErrorSeverity,
    },
}

pub(crate) fn observation_from_agent_event(event: &AgentEvent) -> Option<TurnObservation> {
    match event {
        AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            role,
            task,
            status,
            summary,
            depth,
            error,
            reason,
            budget_limit_kind,
            budget_usage,
            updated_at,
        } => Some(TurnObservation::AgentState {
            id: id.clone(),
            path: path.clone(),
            parent_path: parent_path.clone(),
            role: role.clone(),
            task: task.clone(),
            status: *status,
            summary: summary.clone(),
            depth: *depth,
            error: error.clone(),
            reason: reason.clone(),
            budget_limit_kind: *budget_limit_kind,
            budget_usage: *budget_usage,
            updated_at: *updated_at,
        }),
        AgentEvent::AgentRuntimeUpdated { delta } => {
            Some(TurnObservation::RuntimeDelta(delta.clone()))
        }
        AgentEvent::SubAgentActivity {
            call_id,
            occurred_at,
            agent_id,
            path,
            parent_path,
            kind,
            status,
            message,
            timed_out,
            error,
        } => Some(TurnObservation::SubAgentActivity {
            call_id: call_id.clone(),
            occurred_at: *occurred_at,
            agent_id: agent_id.clone(),
            path: path.clone(),
            parent_path: parent_path.clone(),
            kind: *kind,
            status: *status,
            message: message.clone(),
            timed_out: *timed_out,
            error: error.clone(),
        }),
        AgentEvent::TodoListUpdated { snapshot } => {
            Some(TurnObservation::TodoList(snapshot.clone()))
        }
        AgentEvent::TurnInterrupted { reason } => Some(TurnObservation::Error {
            message: reason.clone(),
            severity: ErrorSeverity::Recoverable,
        }),
        AgentEvent::Error { message, severity } => Some(TurnObservation::Error {
            message: message.clone(),
            severity: *severity,
        }),
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
    TurnObservation::ContextCompacted(SessionContextCompaction {
        before_tokens: snapshot.tokens_before,
        after_tokens: snapshot
            .replacement_tokens
            .unwrap_or(snapshot.estimated_request_tokens),
        compacted_at,
    })
}

pub(crate) fn project_observation(
    source_agent_id: &str,
    session_id: &str,
    turn_id: &str,
    sequence: u64,
    current: &SessionViewSnapshot,
    observation: TurnObservation,
) -> SessionEventProjectionBatch {
    let next_sequence = sequence.saturating_add(1);
    let emitted_at = observation.emitted_at();
    let kind = match observation {
        TurnObservation::AgentState {
            id,
            path,
            parent_path,
            role,
            task,
            status,
            summary,
            depth,
            error,
            reason,
            budget_limit_kind,
            budget_usage,
            updated_at,
        } => SessionEventKind::AgentChanged {
            agent: SessionAgentSnapshot {
                id,
                session_id: session_id.to_string(),
                path,
                parent_path,
                role,
                task,
                status,
                summary,
                depth,
                error,
                reason,
                budget_limit_kind,
                budget_usage,
                runtime_usage: None,
                updated_at,
            },
        },
        TurnObservation::RuntimeDelta(delta) => SessionEventKind::RuntimeChanged {
            runtime: Box::new(runtime_snapshot(session_id, current, delta)),
        },
        TurnObservation::SubAgentActivity {
            call_id,
            occurred_at,
            agent_id,
            path,
            parent_path,
            kind,
            status,
            message,
            timed_out,
            error,
        } => SessionEventKind::TimelineEventAppended {
            event: SessionTimelineEvent {
                event_id: format!("{session_id}:activity:{next_sequence}"),
                session_id: session_id.to_string(),
                sequence: next_sequence,
                created_at: occurred_at,
                kind: SessionTimelineEventKind::SubAgentActivity {
                    call_id,
                    agent_id,
                    path,
                    parent_path,
                    kind,
                    status,
                    message,
                    timed_out,
                    error,
                },
            },
        },
        TurnObservation::TodoList(snapshot) => SessionEventKind::TimelineEventAppended {
            event: SessionTimelineEvent {
                event_id: format!("{session_id}:todo:{next_sequence}"),
                session_id: session_id.to_string(),
                sequence: next_sequence,
                created_at: emitted_at,
                kind: SessionTimelineEventKind::TodoListChanged { snapshot },
            },
        },
        TurnObservation::ContextCompacted(compaction) => {
            SessionEventKind::ContextCompacted { compaction }
        }
        TurnObservation::Error { message, severity } => {
            SessionEventKind::ErrorOccurred { message, severity }
        }
    };
    SessionEventProjectionBatch {
        events: vec![SessionEventEnvelope {
            event_id: format!("{session_id}:{next_sequence}"),
            session_id: session_id.to_string(),
            source_agent_id: Some(source_agent_id.to_string()),
            turn_id: Some(turn_id.to_string()),
            emitted_at,
            position: SessionEventPosition::Durable {
                sequence: next_sequence,
            },
            kind,
        }],
        through_sequence: next_sequence,
    }
}

impl TurnObservation {
    fn emitted_at(&self) -> i64 {
        match self {
            Self::AgentState { updated_at, .. } => *updated_at,
            Self::RuntimeDelta(delta) => delta.updated_at,
            Self::SubAgentActivity { occurred_at, .. } => *occurred_at,
            Self::TodoList(_) => unix_timestamp(),
            Self::ContextCompacted(compaction) => compaction.compacted_at,
            Self::Error { .. } => unix_timestamp(),
        }
    }
}

fn runtime_snapshot(
    session_id: &str,
    current: &SessionViewSnapshot,
    delta: pl_protocol::AgentRuntimeDelta,
) -> SessionRuntimeSnapshot {
    let prior = current.runtime.as_ref().map(|runtime| &runtime.usage);
    let prompt_tokens = prior.map_or(0, |usage| usage.prompt_tokens) + delta.usage.prompt_tokens;
    let cached_prompt_tokens =
        prior.map_or(0, |usage| usage.cached_prompt_tokens) + delta.usage.cached_prompt_tokens;
    let completion_tokens =
        prior.map_or(0, |usage| usage.completion_tokens) + delta.usage.completion_tokens;
    let total_tokens = prior.map_or(0, |usage| usage.total_tokens) + delta.usage.total_tokens;
    SessionRuntimeSnapshot {
        session_id: session_id.to_string(),
        usage: SessionRuntimeUsage {
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
        active_skills: current
            .runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.active_skills.clone()),
        active_mcp_servers: current
            .runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.active_mcp_servers.clone()),
        active_lsp_servers: current
            .runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.active_lsp_servers.clone()),
        agent_count: current
            .runtime
            .as_ref()
            .map_or(1, |runtime| runtime.agent_count),
        mcp_health: current
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.mcp_health.clone()),
        updated_at: delta.updated_at,
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

#[cfg(test)]
mod tests {
    use pl_protocol::{BudgetLimitKind, BudgetUsage};

    use super::*;

    #[test]
    fn budget_limited_control_event_does_not_project_generic_error() {
        let observation = observation_from_agent_event(&AgentEvent::TurnBudgetLimited {
            reason: "budget reached".to_string(),
            limit_kind: BudgetLimitKind::WallClock,
            usage: BudgetUsage::default(),
        });

        assert!(observation.is_none());
    }
}
