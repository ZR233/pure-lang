use pl_protocol::{
    SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionTurn, SessionTurnState,
};

use super::{interaction::turn_activity_for_interaction, projector::SessionEventProjectionBatch};

/// 已由框架或产品适配层确认的会话事实。
///
/// 产品只能向当前 agent 拥有的 session 记录事实，不能分配 durable sequence、构造事件
/// envelope 或直接广播。所有排序与投影仍由 `pl-core` 完成。
#[derive(Debug, Clone)]
pub struct SessionEventFact {
    pub source_event_id: String,
    pub source_agent_id: Option<String>,
    pub turn_id: Option<String>,
    pub emitted_at: i64,
    pub position: SessionEventFactPosition,
    pub kind: SessionEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEventFactPosition {
    Durable,
    Transient { revision: u64 },
}

impl SessionEventFact {
    pub fn durable(
        source_agent_id: Option<String>,
        turn_id: Option<String>,
        emitted_at: i64,
        kind: SessionEventKind,
    ) -> Self {
        Self {
            source_event_id: format!("fact-{emitted_at}"),
            source_agent_id,
            turn_id,
            emitted_at,
            position: SessionEventFactPosition::Durable,
            kind,
        }
    }
}

pub(crate) fn project_session_facts(
    session_id: &str,
    through_sequence: u64,
    facts: Vec<SessionEventFact>,
) -> SessionEventProjectionBatch {
    let mut sequence = through_sequence;
    let mut events = Vec::with_capacity(facts.len().saturating_mul(2));
    for fact in facts {
        if let Some((turn_id, kind)) =
            interaction_turn_change(&fact.kind, session_id, fact.emitted_at)
        {
            append_projected_fact(
                session_id,
                &mut sequence,
                &mut events,
                SessionEventFact {
                    source_event_id: format!("{}:turn", fact.source_event_id),
                    source_agent_id: fact.source_agent_id.clone(),
                    turn_id: Some(turn_id),
                    emitted_at: fact.emitted_at,
                    position: fact.position,
                    kind,
                },
            );
        }
        append_projected_fact(session_id, &mut sequence, &mut events, fact);
    }
    SessionEventProjectionBatch {
        events,
        through_sequence: sequence,
    }
}

fn append_projected_fact(
    session_id: &str,
    sequence: &mut u64,
    events: &mut Vec<SessionEventEnvelope>,
    fact: SessionEventFact,
) {
    let position = match fact.position {
        SessionEventFactPosition::Durable => {
            *sequence = sequence.saturating_add(1);
            SessionEventPosition::Durable {
                sequence: *sequence,
            }
        }
        SessionEventFactPosition::Transient { revision } => {
            SessionEventPosition::Transient { revision }
        }
    };
    let event_id = match position {
        SessionEventPosition::Durable { sequence } => format!("{session_id}:{sequence}"),
        SessionEventPosition::Transient { revision } => {
            format!("{session_id}:transient:{}:{revision}", fact.source_event_id)
        }
    };
    events.push(SessionEventEnvelope {
        event_id,
        session_id: session_id.to_string(),
        source_agent_id: fact.source_agent_id,
        turn_id: fact.turn_id,
        emitted_at: fact.emitted_at,
        position,
        kind: rebind_kind(fact.kind, session_id),
    });
}

fn interaction_turn_change(
    kind: &SessionEventKind,
    session_id: &str,
    emitted_at: i64,
) -> Option<(String, SessionEventKind)> {
    let SessionEventKind::InteractionChanged { event } = kind else {
        return None;
    };
    let activity = turn_activity_for_interaction(&event.interaction)?;
    let turn_id = event.interaction.scope.turn_id.clone();
    Some((
        turn_id.clone(),
        SessionEventKind::TurnChanged {
            turn: SessionTurn {
                turn_id,
                session_id: session_id.to_string(),
                state: SessionTurnState::InProgress { activity },
                updated_at: emitted_at,
            },
        },
    ))
}

fn rebind_kind(mut kind: SessionEventKind, session_id: &str) -> SessionEventKind {
    match &mut kind {
        SessionEventKind::TurnChanged { turn } => turn.session_id = session_id.to_string(),
        SessionEventKind::MessageChanged { message } => message.session_id = session_id.to_string(),
        SessionEventKind::PartChanged { part } => part.session_id = session_id.to_string(),
        SessionEventKind::InteractionChanged { event } => {
            event.interaction.scope.session_id = session_id.to_string()
        }
        SessionEventKind::AgentChanged { agent } => agent.session_id = session_id.to_string(),
        SessionEventKind::TimelineEventAppended { event } => {
            event.session_id = session_id.to_string()
        }
        SessionEventKind::RuntimeChanged { runtime } => runtime.session_id = session_id.to_string(),
        SessionEventKind::MessageRemoved { .. }
        | SessionEventKind::PartRemoved { .. }
        | SessionEventKind::PartDelta { .. }
        | SessionEventKind::SkillActivated { .. }
        | SessionEventKind::PlanChanged { .. }
        | SessionEventKind::ContextCompacted { .. }
        | SessionEventKind::ErrorOccurred { .. } => {}
    }
    kind
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
        InteractionScope, InteractionStatus, SessionTurnActivity,
    };

    use super::*;

    #[test]
    fn pending_interaction_facts_project_typed_waiting_activity_before_interaction() {
        let cases = [
            (
                InteractionKind::ToolApproval,
                InteractionPayload::ToolApproval {
                    name: "exec".to_string(),
                    arguments: serde_json::json!({}),
                    working_directory: None,
                    parent_agent_id: None,
                },
                SessionTurnActivity::WaitingForApproval,
            ),
            (
                InteractionKind::UserInput,
                InteractionPayload::UserInput {
                    questions: Vec::new(),
                },
                SessionTurnActivity::WaitingForUserInput,
            ),
            (
                InteractionKind::PlanConfirmation,
                InteractionPayload::PlanConfirmation {
                    plan_id: "plan-1".to_string(),
                    content: "plan".to_string(),
                },
                SessionTurnActivity::WaitingForPlanConfirmation,
            ),
        ];

        for (kind, payload, expected_activity) in cases {
            let batch = project_session_facts(
                "session",
                0,
                vec![interaction_fact(kind, InteractionStatus::Pending, payload)],
            );

            assert_eq!(batch.through_sequence, 2);
            assert!(matches!(
                &batch.events[0].kind,
                SessionEventKind::TurnChanged { turn }
                    if turn.state == SessionTurnState::InProgress {
                        activity: expected_activity,
                    }
            ));
            assert!(matches!(
                &batch.events[1].kind,
                SessionEventKind::InteractionChanged { event }
                    if event.interaction.scope.session_id == "session"
            ));
        }
    }

    #[test]
    fn resolved_interaction_fact_returns_the_turn_to_thinking() {
        let batch = project_session_facts(
            "session",
            4,
            vec![interaction_fact(
                InteractionKind::UserInput,
                InteractionStatus::Resolved,
                InteractionPayload::UserInput {
                    questions: Vec::new(),
                },
            )],
        );

        assert_eq!(batch.through_sequence, 6);
        assert!(matches!(
            &batch.events[0].kind,
            SessionEventKind::TurnChanged { turn }
                if turn.state == SessionTurnState::InProgress {
                    activity: SessionTurnActivity::Thinking,
                }
        ));
        assert!(matches!(
            &batch.events[1].kind,
            SessionEventKind::InteractionChanged { .. }
        ));
    }

    fn interaction_fact(
        kind: InteractionKind,
        status: InteractionStatus,
        payload: InteractionPayload,
    ) -> SessionEventFact {
        SessionEventFact::durable(
            Some("agent".to_string()),
            Some("turn".to_string()),
            7,
            SessionEventKind::InteractionChanged {
                event: Box::new(InteractionChangedEvent {
                    interaction: InteractionRequest {
                        interaction_id: "interaction".to_string(),
                        kind,
                        status,
                        scope: InteractionScope {
                            session_id: "wrong-session".to_string(),
                            turn_id: "turn".to_string(),
                            item_id: None,
                            tool_id: None,
                            agent_path: None,
                        },
                        payload,
                        created_at: 7,
                        updated_at: 7,
                        resolved_at: None,
                        resolution: None,
                    },
                }),
            },
        )
    }
}
