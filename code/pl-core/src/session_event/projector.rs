use std::collections::BTreeSet;

use pl_protocol::{
    ErrorSeverity, SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
    SessionMessageRole, SessionMessageStatus, SessionPart, SessionPartContent, SessionPartDelta,
    SessionPartDeltaField, SessionPartStatus, SessionTextChannel, SessionTurn, SessionTurnStatus,
};
use pl_trace::{
    TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent, TraceTextChannel,
};

use crate::agent_runtime::{
    AgentRuntimeEvent, AgentRuntimeEventKind, MailboxDeliveryState, MailboxTurnTrigger,
    PendingAgentInput, TurnOutcomeKind,
};

use super::trace_part::session_part;

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
                if event.interaction.status == pl_protocol::InteractionStatus::Pending {
                    projector.turn_phase(
                        &event.interaction.scope.turn_id,
                        SessionTurnStatus::WaitingForInteraction,
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
            } else if input.trigger == MailboxTurnTrigger::StartIfIdle {
                projector.durable(
                    Some(turn_id.to_string()),
                    event.created_at,
                    SessionEventKind::TurnChanged {
                        turn: SessionTurn {
                            turn_id: turn_id.to_string(),
                            session_id: session_id.to_string(),
                            status: SessionTurnStatus::Queued,
                            reason: None,
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
                        status: SessionTurnStatus::ContextLoading,
                        reason: None,
                        updated_at: event.created_at,
                    },
                },
            );
            for input in claimed_inputs {
                if input.trigger == MailboxTurnTrigger::StartIfIdle
                    && input.turn_id.as_str() != turn_id
                {
                    projector.durable(
                        Some(input.turn_id.to_string()),
                        event.created_at,
                        SessionEventKind::TurnChanged {
                            turn: SessionTurn {
                                turn_id: input.turn_id.to_string(),
                                session_id: session_id.to_string(),
                                status: SessionTurnStatus::Cancelled,
                                reason: Some(format!("coalesced_into_turn:{turn_id}")),
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
            let (turn_status, message_status) = match outcome.kind {
                TurnOutcomeKind::Completed => (
                    SessionTurnStatus::Completed,
                    SessionMessageStatus::Completed,
                ),
                TurnOutcomeKind::Cancelled => (
                    SessionTurnStatus::Cancelled,
                    SessionMessageStatus::Cancelled,
                ),
                TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited => {
                    (SessionTurnStatus::Failed, SessionMessageStatus::Failed)
                }
            };
            projector.durable(
                Some(turn_id.to_string()),
                event.created_at,
                SessionEventKind::TurnChanged {
                    turn: SessionTurn {
                        turn_id: turn_id.to_string(),
                        session_id: session_id.to_string(),
                        status: turn_status,
                        reason: outcome.reason.clone(),
                        updated_at: outcome.finished_at,
                    },
                },
            );
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
            if outcome.kind == TurnOutcomeKind::BudgetLimited {
                projector.durable(
                    Some(turn_id.to_string()),
                    event.created_at,
                    SessionEventKind::ErrorOccurred {
                        message: outcome
                            .reason
                            .clone()
                            .unwrap_or_else(|| "turn budget limited".to_string()),
                        severity: ErrorSeverity::Recoverable,
                    },
                );
            }
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
                    text: input.message.clone(),
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
        let status = match item.kind {
            pl_trace::TracePartKind::Turn => SessionTurnStatus::ContextLoading,
            pl_trace::TracePartKind::Inference => SessionTurnStatus::WaitingForModel,
            pl_trace::TracePartKind::Text
            | pl_trace::TracePartKind::Thinking
            | pl_trace::TracePartKind::Plan => SessionTurnStatus::Streaming,
            pl_trace::TracePartKind::Tool | pl_trace::TracePartKind::Agent => {
                SessionTurnStatus::RunningTool
            }
        };
        self.turn_phase(&item.turn_id, status, item.updated_at);
    }

    fn turn_phase(&mut self, turn_id: &str, status: SessionTurnStatus, updated_at: i64) {
        self.durable(
            Some(turn_id.to_string()),
            updated_at,
            SessionEventKind::TurnChanged {
                turn: SessionTurn {
                    turn_id: turn_id.to_string(),
                    session_id: self.session_id.to_string(),
                    status,
                    reason: None,
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
        let statuses = batch
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                SessionEventKind::TurnChanged { turn } => Some(turn.status),
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
            statuses,
            vec![
                SessionTurnStatus::ContextLoading,
                SessionTurnStatus::WaitingForModel,
                SessionTurnStatus::Streaming,
                SessionTurnStatus::RunningTool,
            ]
        );
    }

    #[test]
    fn pending_interaction_projects_waiting_phase_before_interaction() {
        let trace = TraceEvent {
            session_id: "session".to_string(),
            sequence: 0,
            timestamp: 7,
            kind: TraceEventKind::InteractionChanged {
                event: InteractionChangedEvent {
                    interaction: InteractionRequest {
                        interaction_id: "interaction".to_string(),
                        kind: InteractionKind::UserInput,
                        status: InteractionStatus::Pending,
                        scope: InteractionScope {
                            session_id: "session".to_string(),
                            turn_id: "turn".to_string(),
                            item_id: None,
                            tool_id: None,
                            agent_path: None,
                        },
                        payload: InteractionPayload::UserInput {
                            questions: Vec::new(),
                        },
                        created_at: 7,
                        updated_at: 7,
                        resolved_at: None,
                        resolution: None,
                    },
                },
            },
        };

        let batch = project_trace_events("agent", "session", 0, &[trace]);
        assert!(matches!(
            &batch.events[0].kind,
            SessionEventKind::TurnChanged { turn }
                if turn.status == SessionTurnStatus::WaitingForInteraction
        ));
        assert!(matches!(
            &batch.events[1].kind,
            SessionEventKind::InteractionChanged { .. }
        ));
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
