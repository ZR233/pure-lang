use super::agent::{bridge_agent_snapshot, bridge_agent_timeline_event};
use super::interaction::bridge_interaction_changed;
use super::message::{bridge_message, bridge_part, bridge_part_delta, bridge_turn};
use super::records::session_summary_dto;
use super::runtime::{
    bridge_lsp_health, bridge_mcp_health, bridge_session_runtime, bridge_skill_activation,
};
use crate::api::studio::types::{BridgeEventEnvelope, BridgeEventPayload, BridgePlanLifecycleDto};
use pl_protocol::{StudioEventEnvelope, StudioEventKind};
pub(crate) fn bridge_event_envelope(event: StudioEventEnvelope) -> Option<BridgeEventEnvelope> {
    if !bridge_visible_event(&event) {
        return None;
    }
    Some(BridgeEventEnvelope {
        event_id: event.event_id,
        session_id: event.session_id,
        turn_id: event.turn_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: bridge_event_payload(event.kind),
    })
}

pub(crate) fn is_session_state_event(event: &StudioEventEnvelope) -> bool {
    match &event.kind {
        StudioEventKind::MessageUpdated { .. }
        | StudioEventKind::MessageRemoved { .. }
        | StudioEventKind::MessagePartUpdated { .. }
        | StudioEventKind::MessagePartRemoved { .. }
        | StudioEventKind::MessagePartDelta { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::TurnChanged { .. }
        | StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::SessionRuntimeChanged { .. }
        | StudioEventKind::AgentChanged { .. }
        | StudioEventKind::AgentTimelineChanged { .. }
        | StudioEventKind::SkillActivated { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. }
        | StudioEventKind::Stale { .. } => true,
    }
}

pub(crate) fn bridge_visible_event(event: &StudioEventEnvelope) -> bool {
    !matches!(event.kind, StudioEventKind::SessionHandoffChanged { .. })
}

// ── Event payload converters ──

pub(crate) fn bridge_event_payload(kind: StudioEventKind) -> BridgeEventPayload {
    match kind {
        StudioEventKind::TurnChanged { turn } => BridgeEventPayload::TurnChanged {
            turn: bridge_turn(turn),
        },
        StudioEventKind::MessageUpdated { message } => BridgeEventPayload::MessageUpdated {
            message: bridge_message(*message),
        },
        StudioEventKind::MessageRemoved { message_id } => {
            BridgeEventPayload::MessageRemoved { message_id }
        }
        StudioEventKind::MessagePartUpdated { part } => BridgeEventPayload::MessagePartUpdated {
            part: Box::new(bridge_part(*part)),
        },
        StudioEventKind::MessagePartRemoved {
            message_id,
            part_id,
        } => BridgeEventPayload::MessagePartRemoved {
            message_id,
            part_id,
        },
        StudioEventKind::MessagePartDelta { delta } => BridgeEventPayload::MessagePartDelta {
            delta: bridge_part_delta(delta),
        },
        StudioEventKind::InteractionChanged { event } => BridgeEventPayload::InteractionChanged {
            event: bridge_interaction_changed(*event),
        },
        StudioEventKind::AgentChanged { agent } => BridgeEventPayload::AgentChanged {
            agent: Box::new(bridge_agent_snapshot(agent)),
        },
        StudioEventKind::AgentTimelineChanged { event } => {
            BridgeEventPayload::AgentTimelineChanged {
                event: bridge_agent_timeline_event(event),
            }
        }
        StudioEventKind::SessionRuntimeChanged { runtime } => {
            BridgeEventPayload::SessionRuntimeChanged {
                runtime: bridge_session_runtime(runtime),
            }
        }
        StudioEventKind::SkillActivated { activation } => BridgeEventPayload::SkillActivated {
            activation: bridge_skill_activation(activation),
        },
        StudioEventKind::PlanLifecycleChanged { event } => {
            BridgeEventPayload::PlanLifecycleChanged {
                event: BridgePlanLifecycleDto {
                    plan_id: event.plan_id,
                    state: event.state.as_str().to_string(),
                    turn_id: event.turn_id,
                    reason: event.reason,
                    updated_at: event.updated_at,
                },
            }
        }
        StudioEventKind::SessionHandoffChanged { .. } => {
            unreachable!("session handoff events are not bridge-visible")
        }
        StudioEventKind::SessionListChanged {
            project_id,
            sessions,
        } => BridgeEventPayload::SessionListChanged {
            project_id,
            sessions: sessions.into_iter().map(session_summary_dto).collect(),
        },
        StudioEventKind::McpHealthChanged { health } => BridgeEventPayload::McpHealthChanged {
            health: bridge_mcp_health(health),
        },
        StudioEventKind::LspHealthChanged { health } => BridgeEventPayload::LspHealthChanged {
            health: bridge_lsp_health(health),
        },
        StudioEventKind::Stale { lagged_events } => BridgeEventPayload::Stale { lagged_events },
    }
}
