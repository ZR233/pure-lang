use pl_protocol::{
    McpHealthSnapshot, PlanLifecycleEvent, PlanLifecycleState, RuntimeCostAmount,
    SessionRuntimeSnapshot, SessionRuntimeUsage, SessionTimelineEvent, SessionTimelineEventKind,
    SkillActivation, SubAgentActivityKind, TodoListSnapshot, TodoStatus,
};

use crate::api::studio::types::*;

pub(super) fn timeline_event(value: SessionTimelineEvent) -> BridgeSessionTimelineEvent {
    BridgeSessionTimelineEvent {
        event_id: value.event_id,
        session_id: value.session_id,
        sequence: value.sequence,
        created_at: value.created_at,
        kind: match value.kind {
            SessionTimelineEventKind::SubAgentActivity {
                call_id,
                agent_id,
                path,
                parent_path,
                kind,
                status,
                message,
                timed_out,
                error,
            } => BridgeSessionTimelineEventKind::SubAgentActivity {
                call_id,
                agent_id,
                path,
                parent_path,
                kind: activity_kind(kind),
                status: status.map(super::entities::agent_status),
                message,
                timed_out,
                error,
            },
            SessionTimelineEventKind::TodoListChanged { snapshot } => {
                BridgeSessionTimelineEventKind::TodoListChanged {
                    snapshot: todo_snapshot(snapshot),
                }
            }
        },
    }
}

pub(super) fn runtime_snapshot(value: SessionRuntimeSnapshot) -> BridgeSessionRuntimeSnapshot {
    BridgeSessionRuntimeSnapshot {
        session_id: value.session_id,
        usage: runtime_usage(value.usage),
        active_skills: value.active_skills,
        active_mcp_servers: value.active_mcp_servers,
        active_lsp_servers: value.active_lsp_servers,
        agent_count: value.agent_count,
        mcp_health: value.mcp_health.map(mcp_health),
        updated_at: value.updated_at,
    }
}

pub(super) fn runtime_usage(value: SessionRuntimeUsage) -> BridgeSessionRuntimeUsage {
    BridgeSessionRuntimeUsage {
        model: value.model,
        context_window: value.context_window,
        latest_context_tokens: value.latest_context_tokens,
        prompt_tokens: value.prompt_tokens,
        completion_tokens: value.completion_tokens,
        cached_prompt_tokens: value.cached_prompt_tokens,
        total_tokens: value.total_tokens,
        cache_hit_rate: value.cache_hit_rate,
        estimated_costs: value
            .estimated_costs
            .into_iter()
            .map(runtime_cost)
            .collect(),
        has_unpriced_usage: value.has_unpriced_usage,
        updated_at: value.updated_at,
    }
}

pub(super) fn skill_activation(value: SkillActivation) -> BridgeSkillActivation {
    BridgeSkillActivation {
        name: value.name,
        source: value.source,
        path: value.path,
        turn_id: value.turn_id,
        tool_call_id: value.tool_call_id,
        activated_at: value.activated_at,
    }
}

pub(super) fn plan_event(value: PlanLifecycleEvent) -> BridgePlanLifecycleEvent {
    BridgePlanLifecycleEvent {
        plan_id: value.plan_id,
        state: match value.state {
            PlanLifecycleState::PendingConfirmation => {
                BridgePlanLifecycleState::PendingConfirmation
            }
            PlanLifecycleState::Accepted => BridgePlanLifecycleState::Accepted,
            PlanLifecycleState::Implementing => BridgePlanLifecycleState::Implementing,
            PlanLifecycleState::Implemented => BridgePlanLifecycleState::Implemented,
            PlanLifecycleState::ImplementationFailed => {
                BridgePlanLifecycleState::ImplementationFailed
            }
            PlanLifecycleState::ContinuedPlanning => BridgePlanLifecycleState::ContinuedPlanning,
            PlanLifecycleState::Dismissed => BridgePlanLifecycleState::Dismissed,
            PlanLifecycleState::Cancelled => BridgePlanLifecycleState::Cancelled,
        },
        turn_id: value.turn_id,
        reason: value.reason,
        updated_at: value.updated_at,
    }
}

fn todo_snapshot(value: TodoListSnapshot) -> BridgeTodoListSnapshot {
    BridgeTodoListSnapshot {
        call_id: value.call_id,
        agent_id: value.agent_id,
        path: value.path,
        parent_path: value.parent_path,
        explanation: value.explanation,
        items: value
            .items
            .into_iter()
            .map(|item| BridgeTodoItem {
                step: item.step,
                status: match item.status {
                    TodoStatus::Pending => BridgeTodoStatus::Pending,
                    TodoStatus::InProgress => BridgeTodoStatus::InProgress,
                    TodoStatus::Completed => BridgeTodoStatus::Completed,
                },
            })
            .collect(),
    }
}

fn activity_kind(value: SubAgentActivityKind) -> BridgeSubAgentActivityKind {
    match value {
        SubAgentActivityKind::Spawned => BridgeSubAgentActivityKind::Spawned,
        SubAgentActivityKind::MessageQueued => BridgeSubAgentActivityKind::MessageQueued,
        SubAgentActivityKind::FollowupStarted => BridgeSubAgentActivityKind::FollowupStarted,
        SubAgentActivityKind::WaitCompleted => BridgeSubAgentActivityKind::WaitCompleted,
        SubAgentActivityKind::Closed => BridgeSubAgentActivityKind::Closed,
    }
}

fn runtime_cost(value: RuntimeCostAmount) -> BridgeRuntimeCostAmount {
    BridgeRuntimeCostAmount {
        currency: value.currency,
        amount: value.amount,
    }
}

fn mcp_health(value: McpHealthSnapshot) -> BridgeSessionMcpHealthSnapshot {
    BridgeSessionMcpHealthSnapshot {
        generation: value.generation,
        servers: value
            .servers
            .into_iter()
            .map(|availability| BridgeSessionMcpAvailabilityDescriptor {
                server: BridgeSessionMcpServerDescriptor {
                    id: availability.server.id,
                    source: availability.server.source,
                    transport: availability.server.transport,
                    endpoint: availability.server.endpoint,
                    built_in: availability.server.built_in,
                },
                availability: availability.availability,
                message: availability.message,
                last_checked_at: availability.last_checked_at,
                tool_count: availability.tool_count.map(|count| count as u64),
            })
            .collect(),
    }
}
