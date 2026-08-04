use std::collections::BTreeSet;

use pl_protocol::{
    SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
    SessionMessageRole, SessionMessageStatus, SessionPart, SessionPartContent, SessionPartDelta,
    SessionPartDeltaField, SessionPartStatus, SessionTextChannel, SessionTurn, SessionTurnActivity,
    SessionTurnState,
};
use pl_trace::{
    TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent, TraceTextChannel,
};

use crate::agent_runtime::{
    AgentRuntimeEvent, AgentRuntimeEventKind, MailboxDeliveryState, MailboxPresentation,
    PendingAgentInput, TurnOutcomeKind,
};

use super::{interaction::turn_activity_for_interaction, trace_part::session_part};

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionEventProjectionBatch {
    pub(crate) events: Vec<SessionEventEnvelope>,
    pub(crate) through_sequence: u64,
}

impl SessionEventProjectionBatch {
    pub(crate) fn durable_events(&self) -> Vec<SessionEventEnvelope> {
        self.events
            .iter()
            .filter(|event| matches!(event.position, SessionEventPosition::Durable { .. }))
            .cloned()
            .collect()
    }
}

pub(crate) fn project_trace_events(
    source_agent_id: &str,
    session_id: &str,
    through_sequence: u64,
    traces: &[TraceEvent],
) -> SessionEventProjectionBatch {
    let mut projector = Projector::new(source_agent_id, session_id, through_sequence);
    let mut messages = BTreeSet::new();
    for trace in traces {
        match &trace.kind {
            TraceEventKind::TracePartStarted { item } => {
                projector.project_turn_phase(item);
                projector.project_part(item, None, &mut messages);
            }
            TraceEventKind::TracePartCompleted { item } => {
                projector.project_part(item, None, &mut messages)
            }
            TraceEventKind::TracePartFailed { item, error } => {
                projector.project_part(item, Some(error), &mut messages)
            }
            TraceEventKind::TracePartDelta { event } => projector.project_delta(event),
            TraceEventKind::InteractionChanged { event } => {
                if let Some(activity) = turn_activity_for_interaction(&event.interaction) {
                    projector.turn_activity(
                        &event.interaction.scope.turn_id,
                        activity,
                        trace.timestamp,
                    );
                }
                projector.durable(
                    Some(event.interaction.scope.turn_id.clone()),
                    trace.timestamp,
                    SessionEventKind::InteractionChanged {
                        event: Box::new(event.clone()),
                    },
                );
            }
            TraceEventKind::SkillActivated { activation } => projector.durable(
                Some(activation.turn_id.clone()),
                trace.timestamp,
                SessionEventKind::SkillActivated {
                    activation: activation.clone(),
                },
            ),
            TraceEventKind::PlanLifecycleChanged { event } => projector.durable(
                event.turn_id.clone(),
                trace.timestamp,
                SessionEventKind::PlanChanged {
                    event: event.clone(),
                },
            ),
            TraceEventKind::EnabledToolsRecorded { .. } => {}
        }
    }
    projector.finish()
}

pub(crate) fn project_runtime_event(
    event: &AgentRuntimeEvent,
    through_sequence: u64,
) -> SessionEventProjectionBatch {
    let source_agent_id = event.agent_id.as_str();
    let (session_id, turn_id) = match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => {
            (input.session_id.as_str(), input.turn_id.as_str())
        }
        AgentRuntimeEventKind::TurnStarted {
            session_id,
            turn_id,
            ..
        } => (session_id.as_str(), turn_id.as_str()),
        AgentRuntimeEventKind::TurnFinished { outcome, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
            (outcome.session_id.as_str(), outcome.turn_id.as_str())
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::SessionOpened { .. }
        | AgentRuntimeEventKind::Faulted { .. } => return SessionEventProjectionBatch::default(),
    };
    let mut projector = Projector::new(source_agent_id, session_id, through_sequence);
    match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => {
            if matches!(input.delivery_state, MailboxDeliveryState::Claimed { .. }) {
                project_user_input(
                    &mut projector,
                    input,
                    turn_id,
                    &format!("{turn_id}:mail:{}", input.mail_id),
                    event.created_at,
                );
            } else {
                projector.durable(
                    Some(turn_id.to_string()),
                    event.created_at,
                    SessionEventKind::TurnChanged {
                        turn: SessionTurn {
                            turn_id: turn_id.to_string(),
                            session_id: session_id.to_string(),
                            state: SessionTurnState::Queued,
                            updated_at: event.created_at,
                        },
                    },
                );
                project_user_input(
                    &mut projector,
                    input,
                    turn_id,
                    &format!("{turn_id}:user"),
                    event.created_at,
                );
            }
        }
        AgentRuntimeEventKind::TurnStarted { claimed_inputs, .. } => {
            projector.durable(
                Some(turn_id.to_string()),
                event.created_at,
                SessionEventKind::TurnChanged {
                    turn: SessionTurn {
                        turn_id: turn_id.to_string(),
                        session_id: session_id.to_string(),
                        state: SessionTurnState::InProgress {
                            activity: SessionTurnActivity::Preparing,
                        },
                        updated_at: event.created_at,
                    },
                },
            );
            for input in claimed_inputs {
                if input.turn_id.as_str() != turn_id {
                    projector.durable(
                        Some(input.turn_id.to_string()),
                        event.created_at,
                        SessionEventKind::TurnChanged {
                            turn: SessionTurn {
                                turn_id: input.turn_id.to_string(),
                                session_id: session_id.to_string(),
                                state: SessionTurnState::Cancelled {
                                    reason: format!("coalesced_into_turn:{turn_id}"),
                                },
                                updated_at: event.created_at,
                            },
                        },
                    );
                }
                project_user_input(
                    &mut projector,
                    input,
                    turn_id,
                    &format!("{turn_id}:mail:{}", input.mail_id),
                    event.created_at,
                );
            }
        }
        AgentRuntimeEventKind::TurnFinished { outcome, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
            let (turn_state, message_status, terminal_part_status) = match outcome.kind {
                TurnOutcomeKind::Completed => (
                    SessionTurnState::Completed,
                    SessionMessageStatus::Completed,
                    None,
                ),
                TurnOutcomeKind::Cancelled => (
                    SessionTurnState::Cancelled {
                        reason: outcome
                            .reason
                            .clone()
                            .unwrap_or_else(|| "turn cancelled".to_string()),
                    },
                    SessionMessageStatus::Cancelled,
                    Some(SessionPartStatus::Interrupted),
                ),
                TurnOutcomeKind::Failed => (
                    SessionTurnState::Failed {
                        reason: outcome
                            .reason
                            .clone()
                            .unwrap_or_else(|| "turn failed".to_string()),
                    },
                    SessionMessageStatus::Failed,
                    Some(SessionPartStatus::Failed),
                ),
                TurnOutcomeKind::BudgetLimited => (
                    SessionTurnState::Cancelled {
                        reason: outcome
                            .reason
                            .clone()
                            .unwrap_or_else(|| "turn budget limited".to_string()),
                    },
                    SessionMessageStatus::Cancelled,
                    Some(SessionPartStatus::BudgetLimited),
                ),
            };
            projector.turn_activity(turn_id, SessionTurnActivity::Persisting, event.created_at);
            projector.durable(
                Some(turn_id.to_string()),
                event.created_at,
                SessionEventKind::MessageChanged {
                    message: Box::new(SessionMessage {
                        message_id: format!("{turn_id}:assistant"),
                        session_id: session_id.to_string(),
                        turn_id: turn_id.to_string(),
                        role: SessionMessageRole::Assistant,
                        status: message_status,
                        created_at: outcome.finished_at,
                        updated_at: outcome.finished_at,
                        completed_at: Some(outcome.finished_at),
                        error: outcome.reason.clone(),
                        metadata: serde_json::json!({}),
                    }),
                },
            );
            if let Some(status) = terminal_part_status {
                let reason = outcome
                    .reason
                    .clone()
                    .unwrap_or_else(|| "turn ended without a result".to_string());
                projector.durable(
                    Some(turn_id.to_string()),
                    event.created_at,
                    SessionEventKind::PartChanged {
                        part: Box::new(SessionPart {
                            part_id: format!("{turn_id}:terminal-result"),
                            message_id: format!("{turn_id}:assistant"),
                            session_id: session_id.to_string(),
                            turn_id: turn_id.to_string(),
                            order: 1_000_000,
                            revision: 0,
                            status,
                            created_at: outcome.finished_at,
                            updated_at: outcome.finished_at,
                            completed_at: Some(outcome.finished_at),
                            error: Some(reason.clone()),
                            content: SessionPartContent::Text {
                                channel: SessionTextChannel::Final,
                                text: reason,
                                attachments: Vec::new(),
                            },
                            usage: None,
                            synthetic: false,
                            ignored: false,
                        }),
                    },
                );
            }
            projector.durable(
                Some(turn_id.to_string()),
                event.created_at,
                SessionEventKind::TurnChanged {
                    turn: SessionTurn {
                        turn_id: turn_id.to_string(),
                        session_id: session_id.to_string(),
                        state: turn_state,
                        updated_at: outcome.finished_at,
                    },
                },
            );
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::SessionOpened { .. }
        | AgentRuntimeEventKind::Faulted { .. } => {}
    }
    projector.finish()
}

fn project_user_input(
    projector: &mut Projector,
    input: &PendingAgentInput,
    turn_id: &str,
    message_id: &str,
    emitted_at: i64,
) {
    let text = match &input.presentation {
        MailboxPresentation::User => input.message.as_str(),
        MailboxPresentation::Hidden => return,
    };
    let session_id = input.session_id.as_str();
    projector.durable(
        Some(turn_id.to_string()),
        emitted_at,
        SessionEventKind::MessageChanged {
            message: Box::new(SessionMessage {
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                role: SessionMessageRole::User,
                status: SessionMessageStatus::Completed,
                created_at: input.queued_at,
                updated_at: input.queued_at,
                completed_at: Some(input.queued_at),
                error: None,
                metadata: input.metadata.clone(),
            }),
        },
    );
    projector.durable(
        Some(turn_id.to_string()),
        emitted_at,
        SessionEventKind::PartChanged {
            part: Box::new(SessionPart {
                part_id: format!("{message_id}:text"),
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                order: 0,
                revision: 0,
                status: SessionPartStatus::Completed,
                created_at: input.queued_at,
                updated_at: input.queued_at,
                completed_at: Some(input.queued_at),
                error: None,
                content: SessionPartContent::Text {
                    channel: SessionTextChannel::User,
                    text: text.to_string(),
                    attachments: Vec::new(),
                },
                usage: None,
                synthetic: false,
                ignored: false,
            }),
        },
    );
}

pub(crate) fn runtime_event_session_id(event: &AgentRuntimeEvent) -> Option<&str> {
    match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => Some(input.session_id.as_str()),
        AgentRuntimeEventKind::TurnStarted { session_id, .. } => Some(session_id.as_str()),
        AgentRuntimeEventKind::TurnFinished { outcome, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
            Some(outcome.session_id.as_str())
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::SessionOpened { .. }
        | AgentRuntimeEventKind::Faulted { .. } => None,
    }
}

struct Projector<'a> {
    source_agent_id: &'a str,
    session_id: &'a str,
    sequence: u64,
    events: Vec<SessionEventEnvelope>,
}

impl<'a> Projector<'a> {
    fn new(source_agent_id: &'a str, session_id: &'a str, sequence: u64) -> Self {
        Self {
            source_agent_id,
            session_id,
            sequence,
            events: Vec::new(),
        }
    }

    fn durable(&mut self, turn_id: Option<String>, emitted_at: i64, kind: SessionEventKind) {
        self.sequence = self.sequence.saturating_add(1);
        self.events.push(SessionEventEnvelope {
            event_id: format!("{}:{}", self.session_id, self.sequence),
            session_id: self.session_id.to_string(),
            source_agent_id: Some(self.source_agent_id.to_string()),
            turn_id,
            emitted_at,
            position: SessionEventPosition::Durable {
                sequence: self.sequence,
            },
            kind,
        });
    }

    fn project_turn_phase(&mut self, item: &TracePart) {
        if item.kind == pl_trace::TracePartKind::Text
            && item.text_channel == Some(TraceTextChannel::User)
        {
            return;
        }
        let activity = match item.kind {
            pl_trace::TracePartKind::Turn => SessionTurnActivity::Preparing,
            pl_trace::TracePartKind::Inference | pl_trace::TracePartKind::Thinking => {
                SessionTurnActivity::Thinking
            }
            pl_trace::TracePartKind::Text => SessionTurnActivity::Responding,
            pl_trace::TracePartKind::Plan => SessionTurnActivity::Planning,
            pl_trace::TracePartKind::Tool | pl_trace::TracePartKind::Agent => {
                SessionTurnActivity::RunningTool
            }
        };
        self.turn_activity(&item.turn_id, activity, item.updated_at);
    }

    fn turn_activity(&mut self, turn_id: &str, activity: SessionTurnActivity, updated_at: i64) {
        self.durable(
            Some(turn_id.to_string()),
            updated_at,
            SessionEventKind::TurnChanged {
                turn: SessionTurn {
                    turn_id: turn_id.to_string(),
                    session_id: self.session_id.to_string(),
                    state: SessionTurnState::InProgress { activity },
                    updated_at,
                },
            },
        );
    }

    fn project_part(
        &mut self,
        item: &TracePart,
        failure: Option<&str>,
        messages: &mut BTreeSet<String>,
    ) {
        if item.text_channel == Some(TraceTextChannel::User) {
            return;
        }
        let message_id = format!("{}:assistant", item.turn_id);
        if messages.insert(message_id.clone()) {
            self.durable(
                Some(item.turn_id.clone()),
                item.created_at,
                SessionEventKind::MessageChanged {
                    message: Box::new(SessionMessage {
                        message_id: message_id.clone(),
                        session_id: self.session_id.to_string(),
                        turn_id: item.turn_id.clone(),
                        role: SessionMessageRole::Assistant,
                        status: SessionMessageStatus::Streaming,
                        created_at: item.created_at,
                        updated_at: item.updated_at,
                        completed_at: None,
                        error: None,
                        metadata: serde_json::json!({}),
                    }),
                },
            );
        }
        self.durable(
            Some(item.turn_id.clone()),
            item.updated_at,
            SessionEventKind::PartChanged {
                part: Box::new(session_part(self.session_id, &message_id, item, failure)),
            },
        );
    }

    fn project_delta(&mut self, event: &TracePartDeltaEvent) {
        if matches!(
            event.delta,
            TraceDelta::Text {
                text_channel: TraceTextChannel::User,
                ..
            }
        ) {
            return;
        }
        let (field, delta, chunk_index) = match &event.delta {
            TraceDelta::Text { delta, .. } => (SessionPartDeltaField::Text, delta.clone(), None),
            TraceDelta::Thinking { chunk_index, delta } => (
                SessionPartDeltaField::ReasoningSummary,
                delta.clone(),
                Some(*chunk_index),
            ),
            TraceDelta::ReasoningContent { chunk_index, delta } => (
                SessionPartDeltaField::ReasoningContent,
                delta.clone(),
                Some(*chunk_index),
            ),
            TraceDelta::ToolArguments { delta } => {
                (SessionPartDeltaField::ToolArguments, delta.clone(), None)
            }
            TraceDelta::ToolResult { delta } => {
                (SessionPartDeltaField::ToolResult, delta.clone(), None)
            }
            TraceDelta::Plan { delta } => (SessionPartDeltaField::PlanContent, delta.clone(), None),
        };
        self.events.push(SessionEventEnvelope {
            event_id: format!(
                "{}:{}:{}:{}",
                self.session_id, event.turn_id, event.item_id, event.revision
            ),
            session_id: self.session_id.to_string(),
            source_agent_id: Some(self.source_agent_id.to_string()),
            turn_id: Some(event.turn_id.clone()),
            emitted_at: event.updated_at,
            position: SessionEventPosition::Transient {
                revision: event.revision,
            },
            kind: SessionEventKind::PartDelta {
                delta: SessionPartDelta {
                    part_id: event.item_id.clone(),
                    revision: event.revision,
                    field,
                    delta,
                    chunk_index,
                },
            },
        });
    }

    fn finish(self) -> SessionEventProjectionBatch {
        SessionEventProjectionBatch {
            events: self.events,
            through_sequence: self.sequence,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_runtime::{
        AgentId, AgentIdentity, AgentRegistration, AgentTurnOutcome, SessionId, TurnId,
    };
    use pl_protocol::{
        InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
        InteractionScope, InteractionStatus,
    };
    use pl_trace::{TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartStatus};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn trace_part_starts_project_canonical_turn_phases() {
        let kinds = [
            TracePartKind::Turn,
            TracePartKind::Inference,
            TracePartKind::Thinking,
            TracePartKind::Tool,
        ];
        let traces = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| TraceEvent {
                session_id: "session".to_string(),
                sequence: index as u64,
                timestamp: index as i64 + 1,
                kind: TraceEventKind::TracePartStarted {
                    item: trace_part(kind, index as u64 + 1),
                },
            })
            .collect::<Vec<_>>();

        let batch = project_trace_events("agent", "session", 0, &traces);
        let states = batch
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                SessionEventKind::TurnChanged { turn } => Some(turn.state.clone()),
                SessionEventKind::MessageChanged { .. }
                | SessionEventKind::MessageRemoved { .. }
                | SessionEventKind::PartChanged { .. }
                | SessionEventKind::PartRemoved { .. }
                | SessionEventKind::PartDelta { .. }
                | SessionEventKind::InteractionChanged { .. }
                | SessionEventKind::AgentChanged { .. }
                | SessionEventKind::TimelineEventAppended { .. }
                | SessionEventKind::RuntimeChanged { .. }
                | SessionEventKind::SkillActivated { .. }
                | SessionEventKind::PlanChanged { .. }
                | SessionEventKind::ContextCompacted { .. }
                | SessionEventKind::ErrorOccurred { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            states,
            vec![
                SessionTurnState::InProgress {
                    activity: SessionTurnActivity::Preparing,
                },
                SessionTurnState::InProgress {
                    activity: SessionTurnActivity::Thinking,
                },
                SessionTurnState::InProgress {
                    activity: SessionTurnActivity::Thinking,
                },
                SessionTurnState::InProgress {
                    activity: SessionTurnActivity::RunningTool,
                },
            ]
        );
    }

    #[test]
    fn pending_interactions_project_typed_waiting_activity_before_interaction() {
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
            let trace = interaction_trace(kind, InteractionStatus::Pending, payload);
            let batch = project_trace_events("agent", "session", 0, &[trace]);
            assert!(matches!(
                &batch.events[0].kind,
                SessionEventKind::TurnChanged { turn }
                    if turn.state == SessionTurnState::InProgress {
                        activity: expected_activity,
                    }
            ));
            assert!(matches!(
                &batch.events[1].kind,
                SessionEventKind::InteractionChanged { .. }
            ));
        }
    }

    #[test]
    fn resolved_interaction_returns_the_turn_to_thinking() {
        let trace = interaction_trace(
            InteractionKind::UserInput,
            InteractionStatus::Resolved,
            InteractionPayload::UserInput {
                questions: Vec::new(),
            },
        );

        let batch = project_trace_events("agent", "session", 0, &[trace]);

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

    #[test]
    fn typed_mailbox_presentation_controls_user_timeline_projection() {
        let user = projected_mailbox_events(MailboxPresentation::User);
        assert_eq!(user.len(), 2);
        assert!(matches!(
            &user[0].kind,
            SessionEventKind::MessageChanged { message }
                if message.role == SessionMessageRole::User
        ));
        assert!(matches!(
            &user[1].kind,
            SessionEventKind::PartChanged { part }
                if matches!(
                    &part.content,
                    SessionPartContent::Text { text, .. } if text == "internal"
                ) && !part.synthetic
        ));

        let hidden = projected_mailbox_events(MailboxPresentation::Hidden);
        assert!(hidden.is_empty());
    }

    #[test]
    fn failed_turn_projects_persisting_terminal_result_then_failed_state() {
        let outcome = AgentTurnOutcome {
            turn_id: TurnId::new("turn").expect("turn id"),
            session_id: SessionId::new("session").expect("session id"),
            kind: TurnOutcomeKind::Failed,
            reason: Some("provider failed".to_string()),
            failure: None,
            usage: Default::default(),
            finished_at: 9,
        };
        let snapshot = AgentRegistration::with_session(
            AgentIdentity {
                id: AgentId::new("agent").expect("agent id"),
                parent_id: None,
                role: crate::AgentRoleId::new("planner").expect("role id"),
                depth: 0,
            },
            SessionId::new("session").expect("session id"),
        )
        .into_durable_state()
        .snapshot;
        let batch = project_runtime_event(
            &AgentRuntimeEvent {
                agent_id: snapshot.identity.id.clone(),
                sequence: 1,
                created_at: 9,
                kind: AgentRuntimeEventKind::TurnFinished {
                    outcome,
                    snapshot,
                    finalized_with_tool: None,
                },
            },
            0,
        );

        assert!(matches!(
            &batch.events[0].kind,
            SessionEventKind::TurnChanged { turn }
                if turn.state == SessionTurnState::InProgress {
                    activity: SessionTurnActivity::Persisting,
                }
        ));
        assert!(matches!(
            &batch.events[1].kind,
            SessionEventKind::MessageChanged { message }
                if message.status == SessionMessageStatus::Failed
        ));
        assert!(matches!(
            &batch.events[2].kind,
            SessionEventKind::PartChanged { part }
                if part.status == SessionPartStatus::Failed
                    && part.error.as_deref() == Some("provider failed")
                    && matches!(
                        &part.content,
                        SessionPartContent::Text { text, .. } if text == "provider failed"
                    )
        ));
        assert!(matches!(
            &batch.events[3].kind,
            SessionEventKind::TurnChanged { turn }
                if turn.state
                    == SessionTurnState::Failed {
                        reason: "provider failed".to_string(),
                    }
        ));
    }

    #[test]
    fn budget_limited_turn_projects_cancelled_state_without_generic_error() {
        let outcome = AgentTurnOutcome {
            turn_id: TurnId::new("turn").expect("turn id"),
            session_id: SessionId::new("session").expect("session id"),
            kind: TurnOutcomeKind::BudgetLimited,
            reason: Some("active wall-clock budget reached".to_string()),
            failure: None,
            usage: Default::default(),
            finished_at: 9,
        };
        let snapshot = AgentRegistration::with_session(
            AgentIdentity {
                id: AgentId::new("agent").expect("agent id"),
                parent_id: None,
                role: crate::AgentRoleId::new("planner").expect("role id"),
                depth: 0,
            },
            SessionId::new("session").expect("session id"),
        )
        .into_durable_state()
        .snapshot;
        let batch = project_runtime_event(
            &AgentRuntimeEvent {
                agent_id: snapshot.identity.id.clone(),
                sequence: 1,
                created_at: 9,
                kind: AgentRuntimeEventKind::TurnFinished {
                    outcome,
                    snapshot,
                    finalized_with_tool: None,
                },
            },
            0,
        );

        assert_eq!(batch.events.len(), 4);
        assert!(matches!(
            &batch.events[1].kind,
            SessionEventKind::MessageChanged { message }
                if message.status == SessionMessageStatus::Cancelled
        ));
        assert!(matches!(
            &batch.events[2].kind,
            SessionEventKind::PartChanged { part }
                if part.status == SessionPartStatus::BudgetLimited
                    && part.error.as_deref() == Some("active wall-clock budget reached")
        ));
        assert!(matches!(
            &batch.events[3].kind,
            SessionEventKind::TurnChanged { turn }
                if turn.state == SessionTurnState::Cancelled {
                    reason: "active wall-clock budget reached".to_string(),
                }
        ));
        assert!(
            !batch
                .events
                .iter()
                .any(|event| matches!(&event.kind, SessionEventKind::ErrorOccurred { .. }))
        );
    }

    fn interaction_trace(
        kind: InteractionKind,
        status: InteractionStatus,
        payload: InteractionPayload,
    ) -> TraceEvent {
        TraceEvent {
            session_id: "session".to_string(),
            sequence: 0,
            timestamp: 7,
            kind: TraceEventKind::InteractionChanged {
                event: InteractionChangedEvent {
                    interaction: InteractionRequest {
                        interaction_id: "interaction".to_string(),
                        kind,
                        status,
                        scope: InteractionScope {
                            session_id: "session".to_string(),
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
                },
            },
        }
    }

    fn projected_mailbox_events(presentation: MailboxPresentation) -> Vec<SessionEventEnvelope> {
        let input = PendingAgentInput {
            mail_id: "mail-1".to_string(),
            turn_id: TurnId::new("turn").expect("turn id"),
            session_id: SessionId::new("session").expect("session id"),
            message: "internal".to_string(),
            metadata: serde_json::json!({}),
            presentation,
            delivery_state: MailboxDeliveryState::Pending,
            queued_at: 1,
        };
        let mut projector = Projector::new("agent", "session", 0);
        project_user_input(&mut projector, &input, "turn", "message", 1);
        projector.finish().events
    }

    fn trace_part(kind: TracePartKind, timestamp: u64) -> TracePart {
        let mut part = TracePart::text(
            "turn",
            format!("part-{timestamp}"),
            timestamp,
            TraceTextChannel::Commentary,
            "",
            TracePartStatus::Started,
            timestamp as i64,
        );
        part.kind = kind;
        part.text_channel = None;
        part
    }
}
