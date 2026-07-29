mod entities;
mod interaction;
mod runtime;

use anyhow::Result;
use pl_protocol::{
    ErrorSeverity, SessionEventEnvelope, SessionEventKind, SessionEventPosition,
    SessionResyncReason, SessionStreamFrame,
};

use crate::api::studio::types::{
    BridgeErrorSeverity, BridgeSessionEventEnvelope, BridgeSessionEventKind,
    BridgeSessionEventPosition, BridgeSessionResyncReason, BridgeSessionStreamFrame,
};

pub(crate) fn bridge_session_frame(frame: SessionStreamFrame) -> Result<BridgeSessionStreamFrame> {
    Ok(match frame {
        SessionStreamFrame::Snapshot { snapshot } => BridgeSessionStreamFrame::Snapshot {
            snapshot: entities::session_snapshot(*snapshot)?,
        },
        SessionStreamFrame::Event { event } => BridgeSessionStreamFrame::Event {
            event: session_event(*event)?,
        },
        SessionStreamFrame::ResyncRequired { reason } => BridgeSessionStreamFrame::ResyncRequired {
            reason: resync_reason(reason),
        },
    })
}

fn session_event(event: SessionEventEnvelope) -> Result<BridgeSessionEventEnvelope> {
    Ok(BridgeSessionEventEnvelope {
        event_id: event.event_id,
        session_id: event.session_id,
        source_agent_id: event.source_agent_id,
        turn_id: event.turn_id,
        emitted_at: event.emitted_at,
        position: match event.position {
            SessionEventPosition::Durable { sequence } => {
                BridgeSessionEventPosition::Durable { sequence }
            }
            SessionEventPosition::Transient { revision } => {
                BridgeSessionEventPosition::Transient { revision }
            }
        },
        kind: event_kind(event.kind)?,
    })
}

fn event_kind(kind: SessionEventKind) -> Result<BridgeSessionEventKind> {
    Ok(match kind {
        SessionEventKind::TurnChanged { turn } => BridgeSessionEventKind::TurnChanged {
            turn: entities::turn(turn),
        },
        SessionEventKind::MessageChanged { message } => BridgeSessionEventKind::MessageChanged {
            message: entities::message(*message)?,
        },
        SessionEventKind::MessageRemoved { message_id } => {
            BridgeSessionEventKind::MessageRemoved { message_id }
        }
        SessionEventKind::PartChanged { part } => BridgeSessionEventKind::PartChanged {
            part: entities::part(*part)?,
        },
        SessionEventKind::PartRemoved {
            message_id,
            part_id,
        } => BridgeSessionEventKind::PartRemoved {
            message_id,
            part_id,
        },
        SessionEventKind::PartDelta { delta } => BridgeSessionEventKind::PartDelta {
            delta: entities::part_delta(delta),
        },
        SessionEventKind::InteractionChanged { event } => {
            BridgeSessionEventKind::InteractionChanged {
                interaction: interaction::interaction(event.interaction)?,
            }
        }
        SessionEventKind::AgentChanged { agent } => BridgeSessionEventKind::AgentChanged {
            agent: entities::agent_snapshot(agent),
        },
        SessionEventKind::TimelineEventAppended { event } => {
            BridgeSessionEventKind::TimelineEventAppended {
                event: runtime::timeline_event(event),
            }
        }
        SessionEventKind::RuntimeChanged { runtime: value } => {
            BridgeSessionEventKind::RuntimeChanged {
                runtime: runtime::runtime_snapshot(*value),
            }
        }
        SessionEventKind::SkillActivated { activation } => BridgeSessionEventKind::SkillActivated {
            activation: runtime::skill_activation(activation),
        },
        SessionEventKind::PlanChanged { event } => BridgeSessionEventKind::PlanChanged {
            event: runtime::plan_event(event),
        },
        SessionEventKind::ContextCompacted { compaction } => {
            BridgeSessionEventKind::ContextCompacted {
                compaction: entities::context_compaction(compaction),
            }
        }
        SessionEventKind::ErrorOccurred { message, severity } => {
            BridgeSessionEventKind::ErrorOccurred {
                message,
                severity: match severity {
                    ErrorSeverity::Transient => BridgeErrorSeverity::Transient,
                    ErrorSeverity::Recoverable => BridgeErrorSeverity::Recoverable,
                    ErrorSeverity::Fatal => BridgeErrorSeverity::Fatal,
                },
            }
        }
    })
}

fn resync_reason(reason: SessionResyncReason) -> BridgeSessionResyncReason {
    match reason {
        SessionResyncReason::Lagged { events } => BridgeSessionResyncReason::Lagged { events },
        SessionResyncReason::CursorExpired {
            requested,
            oldest_available,
        } => BridgeSessionResyncReason::CursorExpired {
            requested,
            oldest_available,
        },
        SessionResyncReason::ReplayLimitExceeded { available, limit } => {
            BridgeSessionResyncReason::ReplayLimitExceeded { available, limit }
        }
        SessionResyncReason::RevisionGap {
            part_id,
            expected,
            actual,
        } => BridgeSessionResyncReason::RevisionGap {
            part_id,
            expected,
            actual,
        },
        SessionResyncReason::ProjectionInvariant { message } => {
            BridgeSessionResyncReason::ProjectionInvariant { message }
        }
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        AgentStatus, PlanLifecycleEvent, PlanLifecycleState, SessionAgentPart, SessionPart,
        SessionPartContent, SessionPartStatus, SessionStreamFrame, SessionTextChannel,
        SessionTimelineEvent, SessionTimelineEventKind, SessionToolPart, SubAgentActivityKind,
    };
    use pretty_assertions::assert_eq;

    use crate::api::studio::types::{
        BridgePlanLifecycleState, BridgeSessionPartContent, BridgeSessionResyncReason,
        BridgeSessionStreamFrame, BridgeSessionTimelineEventKind, BridgeSubAgentActivityKind,
    };

    use super::{bridge_session_frame, entities, runtime};

    #[test]
    fn every_resync_reason_converts_without_losing_fields() {
        let cases = [
            pl_protocol::SessionResyncReason::Lagged { events: 3 },
            pl_protocol::SessionResyncReason::CursorExpired {
                requested: 4,
                oldest_available: 7,
            },
            pl_protocol::SessionResyncReason::ReplayLimitExceeded {
                available: 9,
                limit: 5,
            },
            pl_protocol::SessionResyncReason::RevisionGap {
                part_id: "part-1".to_string(),
                expected: 2,
                actual: 4,
            },
            pl_protocol::SessionResyncReason::ProjectionInvariant {
                message: "invalid projection".to_string(),
            },
        ];

        let converted = cases
            .into_iter()
            .map(|reason| {
                bridge_session_frame(SessionStreamFrame::ResyncRequired { reason }).unwrap()
            })
            .collect::<Vec<_>>();

        assert!(matches!(
            &converted[0],
            BridgeSessionStreamFrame::ResyncRequired {
                reason: BridgeSessionResyncReason::Lagged { events: 3 }
            }
        ));
        assert!(matches!(
            &converted[1],
            BridgeSessionStreamFrame::ResyncRequired {
                reason: BridgeSessionResyncReason::CursorExpired {
                    requested: 4,
                    oldest_available: 7
                }
            }
        ));
        assert!(matches!(
            &converted[2],
            BridgeSessionStreamFrame::ResyncRequired {
                reason: BridgeSessionResyncReason::ReplayLimitExceeded {
                    available: 9,
                    limit: 5
                }
            }
        ));
        assert!(matches!(
            &converted[3],
            BridgeSessionStreamFrame::ResyncRequired {
                reason: BridgeSessionResyncReason::RevisionGap {
                    part_id,
                    expected: 2,
                    actual: 4
                }
            } if part_id == "part-1"
        ));
        assert!(matches!(
            &converted[4],
            BridgeSessionStreamFrame::ResyncRequired {
                reason: BridgeSessionResyncReason::ProjectionInvariant { message }
            } if message == "invalid projection"
        ));
    }

    #[test]
    fn every_part_content_variant_converts_and_json_leaves_are_structural() {
        let contents = vec![
            SessionPartContent::Text {
                channel: SessionTextChannel::Final,
                text: "answer".to_string(),
                attachments: Vec::new(),
            },
            SessionPartContent::Reasoning {
                text: "reasoning".to_string(),
            },
            SessionPartContent::Tool {
                tool: SessionToolPart {
                    tool_call_id: "tool-1".to_string(),
                    call_id: None,
                    provider_item_id: None,
                    name: "exec".to_string(),
                    arguments: r#"{"b":2,"a":1}"#.to_string(),
                    result: Some("ok".to_string()),
                    output_artifacts: vec![serde_json::json!({"kind": "file", "path": "a.txt"})],
                    exit_code: Some(0),
                    timed_out: false,
                    working_directory: None,
                    denial_reason: None,
                    activity_group_id: None,
                },
            },
            SessionPartContent::Agent {
                agent: SessionAgentPart {
                    id: "agent-1".to_string(),
                    path: "root/agent-1".to_string(),
                    parent_path: Some("root".to_string()),
                    role: "explorer".to_string(),
                    task: "inspect".to_string(),
                    status: AgentStatus::Running,
                    summary: None,
                    depth: 1,
                    error: None,
                    reason: None,
                },
            },
            SessionPartContent::Turn,
            SessionPartContent::Inference {
                inference_id: "inference-1".to_string(),
                model: "model-1".to_string(),
            },
            SessionPartContent::Plan {
                content: "plan".to_string(),
            },
            SessionPartContent::File {
                path: "a.txt".to_string(),
                media_type: Some("text/plain".to_string()),
            },
        ];

        let converted = contents
            .into_iter()
            .enumerate()
            .map(|(index, content)| {
                entities::part(SessionPart {
                    part_id: format!("part-{index}"),
                    message_id: "message-1".to_string(),
                    session_id: "session-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    order: index as u64,
                    revision: 0,
                    status: SessionPartStatus::Completed,
                    created_at: 1,
                    updated_at: 1,
                    completed_at: Some(1),
                    error: None,
                    content,
                    usage: None,
                    synthetic: false,
                    ignored: false,
                })
                .unwrap()
                .content
            })
            .collect::<Vec<_>>();

        assert!(matches!(
            converted[0],
            BridgeSessionPartContent::Text { .. }
        ));
        assert!(matches!(
            converted[1],
            BridgeSessionPartContent::Reasoning { .. }
        ));
        let BridgeSessionPartContent::Tool { tool } = &converted[2] else {
            panic!("tool content must remain typed");
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&tool.arguments_json).unwrap(),
            serde_json::json!({"a": 1, "b": 2})
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&tool.output_artifacts_json[0]).unwrap(),
            serde_json::json!({"kind": "file", "path": "a.txt"})
        );
        assert!(matches!(
            converted[3],
            BridgeSessionPartContent::Agent { .. }
        ));
        assert!(matches!(converted[4], BridgeSessionPartContent::Turn));
        assert!(matches!(
            converted[5],
            BridgeSessionPartContent::Inference { .. }
        ));
        assert!(matches!(
            converted[6],
            BridgeSessionPartContent::Plan { .. }
        ));
        assert!(matches!(
            converted[7],
            BridgeSessionPartContent::File { .. }
        ));
    }

    #[test]
    fn every_subagent_activity_and_plan_state_converts_exhaustively() {
        let activity_cases = [
            SubAgentActivityKind::Spawned,
            SubAgentActivityKind::MessageQueued,
            SubAgentActivityKind::FollowupStarted,
            SubAgentActivityKind::WaitCompleted,
            SubAgentActivityKind::Closed,
        ];
        let activities = activity_cases.map(|kind| {
            let event = runtime::timeline_event(SessionTimelineEvent {
                event_id: "event-1".to_string(),
                session_id: "session-1".to_string(),
                sequence: 1,
                created_at: 1,
                kind: SessionTimelineEventKind::SubAgentActivity {
                    call_id: "call-1".to_string(),
                    agent_id: None,
                    path: None,
                    parent_path: None,
                    kind,
                    status: None,
                    message: None,
                    timed_out: None,
                    error: None,
                },
            });
            let BridgeSessionTimelineEventKind::SubAgentActivity { kind, .. } = event.kind else {
                panic!("subagent activity must remain typed");
            };
            kind
        });
        assert_eq!(
            activities,
            [
                BridgeSubAgentActivityKind::Spawned,
                BridgeSubAgentActivityKind::MessageQueued,
                BridgeSubAgentActivityKind::FollowupStarted,
                BridgeSubAgentActivityKind::WaitCompleted,
                BridgeSubAgentActivityKind::Closed,
            ]
        );

        let plan_cases = [
            PlanLifecycleState::PendingConfirmation,
            PlanLifecycleState::Accepted,
            PlanLifecycleState::Implementing,
            PlanLifecycleState::Implemented,
            PlanLifecycleState::ImplementationFailed,
            PlanLifecycleState::ContinuedPlanning,
            PlanLifecycleState::Dismissed,
            PlanLifecycleState::Cancelled,
        ];
        let plan_states = plan_cases.map(|state| {
            runtime::plan_event(PlanLifecycleEvent {
                plan_id: "plan-1".to_string(),
                state,
                turn_id: None,
                reason: None,
                updated_at: 1,
            })
            .state
        });
        assert_eq!(
            plan_states,
            [
                BridgePlanLifecycleState::PendingConfirmation,
                BridgePlanLifecycleState::Accepted,
                BridgePlanLifecycleState::Implementing,
                BridgePlanLifecycleState::Implemented,
                BridgePlanLifecycleState::ImplementationFailed,
                BridgePlanLifecycleState::ContinuedPlanning,
                BridgePlanLifecycleState::Dismissed,
                BridgePlanLifecycleState::Cancelled,
            ]
        );
    }
}
