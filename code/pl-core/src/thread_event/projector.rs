use pl_protocol::{
    AgentMessageChannel, ThreadItem, ThreadItemContent, ThreadItemDelta, ThreadItemDeltaField,
    ThreadItemStatus, ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot,
    ThreadToolCall, Turn, TurnPhase, TurnState,
};
use pl_trace::{
    TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent, TracePartKind,
    TracePartStatus, TraceTextChannel,
};

use crate::agent_runtime::{
    AgentRuntimeEvent, AgentRuntimeEventKind, DurableMailboxEnvelope, MailboxDeliveryState,
    MailboxPresentation, TurnOutcomeKind,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ThreadProjectionBatch {
    pub(crate) notifications: Vec<ThreadNotificationEnvelope>,
    pub(crate) through_revision: u64,
}

pub(crate) fn project_trace_events(
    thread_id: &str,
    current: &ThreadSnapshot,
    traces: &[TraceEvent],
) -> ThreadProjectionBatch {
    let mut projector = Projector::new(thread_id, current.revision);
    let mut active_turn_id = current.active_turn.as_ref().map(|turn| turn.id.clone());
    for trace in traces {
        match &trace.kind {
            TraceEventKind::TracePartStarted { item } => {
                if let Some(phase) = phase_for_item(item) {
                    projector.turn_updated(&item.turn_id, phase, trace.timestamp);
                }
                if let Some(item) = thread_item(thread_id, item, None) {
                    projector.push(
                        trace.timestamp,
                        ThreadNotification::ItemStarted {
                            item: Box::new(item),
                        },
                    );
                }
            }
            TraceEventKind::TracePartDelta { event } => {
                if let Some(delta) = item_delta(event) {
                    projector.push(trace.timestamp, ThreadNotification::ItemDelta { delta });
                }
            }
            TraceEventKind::TracePartCompleted { item } => {
                if let Some(item) = thread_item(thread_id, item, None) {
                    projector.push(
                        trace.timestamp,
                        ThreadNotification::ItemCompleted {
                            item: Box::new(item),
                        },
                    );
                }
            }
            TraceEventKind::TracePartFailed { item, error } => {
                if let Some(item) = thread_item(thread_id, item, Some(error)) {
                    projector.push(
                        trace.timestamp,
                        ThreadNotification::ItemCompleted {
                            item: Box::new(item),
                        },
                    );
                }
            }
            TraceEventKind::InteractionChanged { event } => {
                let mut interaction = event.interaction.clone();
                interaction.scope.thread_id = thread_id.to_string();
                if let Some(turn) = interaction_completion_turn(
                    thread_id,
                    active_turn_id.as_deref(),
                    &interaction,
                    trace.timestamp,
                ) {
                    projector.push(trace.timestamp, ThreadNotification::TurnCompleted { turn });
                    active_turn_id = None;
                }
                projector.push(
                    trace.timestamp,
                    ThreadNotification::InteractionChanged {
                        interaction: Box::new(interaction),
                    },
                );
            }
            TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => {}
        }
    }
    projector.finish()
}

pub(crate) fn project_runtime_event(
    event: &AgentRuntimeEvent,
    through_revision: u64,
) -> ThreadProjectionBatch {
    let Some(thread_id) = runtime_event_thread_id(event) else {
        return ThreadProjectionBatch {
            through_revision,
            ..ThreadProjectionBatch::default()
        };
    };
    let mut projector = Projector::new(thread_id, through_revision);
    match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => {
            if !matches!(input.delivery_state, MailboxDeliveryState::Claimed { .. }) {
                projector.push(
                    event.created_at,
                    ThreadNotification::TurnStarted {
                        turn: turn(
                            input.turn_id.as_str(),
                            thread_id,
                            TurnState::Queued,
                            None,
                            event.created_at,
                        ),
                    },
                );
            }
            project_user_input(
                &mut projector,
                input,
                input.turn_id.as_str(),
                event.created_at,
            );
        }
        AgentRuntimeEventKind::TurnStarted {
            turn_id,
            claimed_inputs,
            ..
        } => {
            projector.push(
                event.created_at,
                ThreadNotification::TurnUpdated {
                    turn: turn(
                        turn_id.as_str(),
                        thread_id,
                        TurnState::InProgress {
                            phase: TurnPhase::Preparing,
                        },
                        Some(event.created_at),
                        event.created_at,
                    ),
                },
            );
            for input in claimed_inputs {
                if input.turn_id.as_str() != turn_id.as_str() {
                    projector.push(
                        event.created_at,
                        ThreadNotification::TurnCompleted {
                            turn: turn(
                                input.turn_id.as_str(),
                                thread_id,
                                TurnState::Interrupted {
                                    reason: format!("coalescedIntoTurn:{turn_id}"),
                                },
                                None,
                                event.created_at,
                            ),
                        },
                    );
                }
                project_user_input(&mut projector, input, turn_id.as_str(), event.created_at);
            }
        }
        AgentRuntimeEventKind::TurnFinished { outcome, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
            projector.turn_updated(
                outcome.turn_id.as_str(),
                TurnPhase::Persisting,
                event.created_at,
            );
            let state = match outcome.kind {
                TurnOutcomeKind::Completed => TurnState::Completed,
                TurnOutcomeKind::Cancelled | TurnOutcomeKind::BudgetLimited => {
                    TurnState::Interrupted {
                        reason: outcome
                            .reason
                            .clone()
                            .unwrap_or_else(|| "turn interrupted".to_string()),
                    }
                }
                TurnOutcomeKind::Failed => TurnState::Failed {
                    reason: outcome
                        .reason
                        .clone()
                        .unwrap_or_else(|| "turn failed".to_string()),
                },
            };
            let mut completed_turn = turn(
                outcome.turn_id.as_str(),
                thread_id,
                state,
                None,
                outcome.finished_at,
            );
            completed_turn.failure = outcome.failure.clone();
            projector.push(
                event.created_at,
                ThreadNotification::TurnCompleted {
                    turn: completed_turn,
                },
            );
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::ThreadOpened { .. }
        | AgentRuntimeEventKind::TurnActivityChanged { .. }
        | AgentRuntimeEventKind::Faulted { .. } => {}
    }
    projector.finish()
}

pub(crate) fn runtime_event_thread_id(event: &AgentRuntimeEvent) -> Option<&str> {
    match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => Some(input.thread_id.as_str()),
        AgentRuntimeEventKind::TurnStarted { thread_id, .. } => Some(thread_id.as_str()),
        AgentRuntimeEventKind::TurnActivityChanged { thread_id, .. } => Some(thread_id.as_str()),
        AgentRuntimeEventKind::TurnFinished { outcome, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
            Some(outcome.thread_id.as_str())
        }
        AgentRuntimeEventKind::Registered { .. }
        | AgentRuntimeEventKind::StateChanged { .. }
        | AgentRuntimeEventKind::ThreadOpened { .. }
        | AgentRuntimeEventKind::Faulted { .. } => None,
    }
}

fn project_user_input(
    projector: &mut Projector<'_>,
    input: &DurableMailboxEnvelope,
    turn_id: &str,
    emitted_at: i64,
) {
    if input.payload.presentation != MailboxPresentation::User {
        return;
    }
    let item_id = if input.mail_id.is_empty() {
        format!("{turn_id}:user")
    } else {
        format!("{turn_id}:mail:{}", input.mail_id)
    };
    projector.push(
        emitted_at,
        ThreadNotification::ItemCompleted {
            item: Box::new(ThreadItem {
                id: item_id,
                thread_id: projector.thread_id.to_string(),
                turn_id: turn_id.to_string(),
                ordinal: 0,
                revision: 0,
                status: ThreadItemStatus::Completed,
                created_at: input.queued_at,
                updated_at: input.queued_at,
                completed_at: Some(input.queued_at),
                error: None,
                content: ThreadItemContent::UserMessage {
                    text: input.payload.message.clone(),
                    attachments: Vec::new(),
                },
                usage: None,
            }),
        },
    );
}

fn turn(
    id: &str,
    thread_id: &str,
    state: TurnState,
    started_at: Option<i64>,
    updated_at: i64,
) -> Turn {
    let completed_at = matches!(
        state,
        TurnState::Completed | TurnState::Failed { .. } | TurnState::Interrupted { .. }
    )
    .then_some(updated_at);
    Turn {
        id: id.to_string(),
        thread_id: thread_id.to_string(),
        state,
        failure: None,
        started_at,
        updated_at,
        completed_at,
    }
}

pub(super) fn interaction_completion_turn(
    thread_id: &str,
    active_turn_id: Option<&str>,
    interaction: &pl_protocol::InteractionRequest,
    emitted_at: i64,
) -> Option<Turn> {
    (interaction.status == pl_protocol::InteractionStatus::Pending
        && active_turn_id == Some(interaction.scope.turn_id.as_str()))
    .then(|| {
        turn(
            &interaction.scope.turn_id,
            thread_id,
            TurnState::Completed,
            None,
            emitted_at,
        )
    })
}

fn phase_for_item(item: &TracePart) -> Option<TurnPhase> {
    match item.kind {
        TracePartKind::Text => Some(match item.text_channel {
            Some(TraceTextChannel::Commentary | TraceTextChannel::Final) => TurnPhase::Responding,
            Some(TraceTextChannel::User) | None => TurnPhase::Thinking,
        }),
        TracePartKind::Thinking | TracePartKind::Inference => Some(TurnPhase::Thinking),
        TracePartKind::Tool | TracePartKind::Agent => Some(TurnPhase::RunningTool),
        TracePartKind::Plan => Some(TurnPhase::Planning),
        TracePartKind::Turn => None,
    }
}

fn thread_item(thread_id: &str, item: &TracePart, failure: Option<&str>) -> Option<ThreadItem> {
    let content = match item.kind {
        TracePartKind::Text => match item.text_channel.unwrap_or(TraceTextChannel::Final) {
            // User inputs are projected exactly once from the durable mailbox event.
            // Trace keeps the model-visible prompt for diagnostics, but is not a second
            // timeline source and must not reveal hidden control inputs.
            TraceTextChannel::User => return None,
            TraceTextChannel::Commentary => ThreadItemContent::AgentMessage {
                channel: AgentMessageChannel::Commentary,
                text: item.content.clone(),
            },
            TraceTextChannel::Final => ThreadItemContent::AgentMessage {
                channel: AgentMessageChannel::Final,
                text: item.content.clone(),
            },
        },
        TracePartKind::Thinking => ThreadItemContent::Reasoning {
            summary: item
                .thinking_chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
            content: item
                .reasoning_content_chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
        },
        TracePartKind::Tool => ThreadItemContent::ToolCall {
            tool: item.tool.as_ref().map_or_else(
                || empty_tool(&item.item_id, "tool"),
                |tool| ThreadToolCall {
                    tool_call_id: tool.tool_call_id.clone(),
                    call_id: tool.call_id.clone(),
                    provider_item_id: tool.provider_item_id.clone(),
                    name: tool.name.clone(),
                    arguments: tool.arguments.clone(),
                    result: tool.result.clone(),
                    output_artifacts: tool.output_artifacts.clone(),
                    exit_code: tool.exit_code,
                    timed_out: tool.timed_out,
                    working_directory: tool.working_directory.clone(),
                    denial_reason: tool.denial_reason.clone(),
                },
            ),
        },
        TracePartKind::Agent => ThreadItemContent::ToolCall {
            tool: ThreadToolCall {
                arguments: item
                    .agent
                    .as_ref()
                    .and_then(|agent| serde_json::to_string(agent).ok())
                    .unwrap_or_default(),
                result: item.agent.as_ref().and_then(|agent| agent.summary.clone()),
                ..empty_tool(&item.item_id, "agent")
            },
        },
        TracePartKind::Plan => ThreadItemContent::Plan {
            content: item.content.clone(),
        },
        TracePartKind::Turn | TracePartKind::Inference => return None,
    };
    Some(ThreadItem {
        id: item.item_id.clone(),
        thread_id: thread_id.to_string(),
        turn_id: item.turn_id.clone(),
        ordinal: item.started_sequence,
        revision: item.revision,
        status: if failure.is_some() && item.status != TracePartStatus::BudgetLimited {
            ThreadItemStatus::Failed
        } else {
            item_status(item.status)
        },
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at: is_terminal(item.status).then_some(item.updated_at),
        error: failure.map(str::to_string),
        content,
        usage: item.usage.clone(),
    })
}

fn empty_tool(item_id: &str, name: &str) -> ThreadToolCall {
    ThreadToolCall {
        tool_call_id: item_id.to_string(),
        call_id: None,
        provider_item_id: None,
        name: name.to_string(),
        arguments: String::new(),
        result: None,
        output_artifacts: Vec::new(),
        exit_code: None,
        timed_out: false,
        working_directory: None,
        denial_reason: None,
    }
}

fn item_delta(event: &TracePartDeltaEvent) -> Option<ThreadItemDelta> {
    if matches!(event.kind, TracePartKind::Turn | TracePartKind::Inference) {
        return None;
    }
    let (field, delta, chunk_index) = match &event.delta {
        TraceDelta::Text { delta, .. } => (ThreadItemDeltaField::Text, delta.clone(), None),
        TraceDelta::Thinking { chunk_index, delta } => (
            ThreadItemDeltaField::ReasoningSummary,
            delta.clone(),
            Some(*chunk_index),
        ),
        TraceDelta::ReasoningContent { chunk_index, delta } => (
            ThreadItemDeltaField::ReasoningContent,
            delta.clone(),
            Some(*chunk_index),
        ),
        TraceDelta::ToolArguments { delta } => {
            (ThreadItemDeltaField::ToolArguments, delta.clone(), None)
        }
        TraceDelta::ToolResult { delta } => (ThreadItemDeltaField::ToolResult, delta.clone(), None),
        TraceDelta::Plan { delta } => (ThreadItemDeltaField::PlanContent, delta.clone(), None),
    };
    Some(ThreadItemDelta {
        item_id: event.item_id.clone(),
        revision: event.revision,
        field,
        delta,
        chunk_index,
    })
}

fn item_status(status: TracePartStatus) -> ThreadItemStatus {
    match status {
        TracePartStatus::Started => ThreadItemStatus::Started,
        TracePartStatus::Streaming => ThreadItemStatus::Streaming,
        TracePartStatus::AwaitingApproval => ThreadItemStatus::AwaitingApproval,
        TracePartStatus::Approved => ThreadItemStatus::Approved,
        TracePartStatus::Denied => ThreadItemStatus::Denied,
        TracePartStatus::Running => ThreadItemStatus::Running,
        TracePartStatus::Completed => ThreadItemStatus::Completed,
        TracePartStatus::Failed => ThreadItemStatus::Failed,
        TracePartStatus::Interrupted => ThreadItemStatus::Interrupted,
        TracePartStatus::BudgetLimited => ThreadItemStatus::BudgetLimited,
    }
}

fn is_terminal(status: TracePartStatus) -> bool {
    matches!(
        status,
        TracePartStatus::Completed
            | TracePartStatus::Failed
            | TracePartStatus::Interrupted
            | TracePartStatus::BudgetLimited
            | TracePartStatus::Denied
    )
}

struct Projector<'a> {
    thread_id: &'a str,
    revision: u64,
    notifications: Vec<ThreadNotificationEnvelope>,
}

impl<'a> Projector<'a> {
    fn new(thread_id: &'a str, revision: u64) -> Self {
        Self {
            thread_id,
            revision,
            notifications: Vec::new(),
        }
    }

    fn turn_updated(&mut self, turn_id: &str, phase: TurnPhase, emitted_at: i64) {
        self.push(
            emitted_at,
            ThreadNotification::TurnUpdated {
                turn: turn(
                    turn_id,
                    self.thread_id,
                    TurnState::InProgress { phase },
                    None,
                    emitted_at,
                ),
            },
        );
    }

    fn push(&mut self, emitted_at: i64, notification: ThreadNotification) {
        self.revision = self.revision.saturating_add(1);
        self.notifications.push(ThreadNotificationEnvelope {
            thread_id: self.thread_id.to_string(),
            revision: self.revision,
            emitted_at,
            notification,
        });
    }

    fn finish(self) -> ThreadProjectionBatch {
        ThreadProjectionBatch {
            notifications: self.notifications,
            through_revision: self.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
        InteractionResolution, InteractionScope, InteractionStatus, THREAD_SCHEMA_VERSION, Thread,
        ThreadSnapshot,
    };

    use super::*;

    #[test]
    fn trace_text_is_one_item_not_message_and_part() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::TracePartCompleted {
                item: TracePart::text(
                    "turn-1",
                    "item-1",
                    1,
                    TraceTextChannel::Final,
                    "done",
                    TracePartStatus::Completed,
                    7,
                ),
            },
        };
        let batch = project_trace_events("thread-1", &snapshot(), &[trace]);
        assert_eq!(batch.notifications.len(), 1);
        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if matches!(item.content, ThreadItemContent::AgentMessage { .. })
        ));
    }

    #[test]
    fn user_trace_does_not_duplicate_or_reveal_mailbox_input() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::TracePartCompleted {
                item: TracePart::text(
                    "turn-1",
                    "internal-user-trace",
                    1,
                    TraceTextChannel::User,
                    "hidden control input",
                    TracePartStatus::Completed,
                    7,
                ),
            },
        };

        assert!(
            project_trace_events("thread-1", &snapshot(), &[trace])
                .notifications
                .is_empty()
        );
    }

    #[test]
    fn resolved_interaction_trace_does_not_update_unrelated_active_turn() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::InteractionChanged {
                event: InteractionChangedEvent {
                    interaction: InteractionRequest {
                        interaction_id: "ask-1".to_string(),
                        kind: InteractionKind::UserInput,
                        status: InteractionStatus::Resolved,
                        scope: InteractionScope {
                            thread_id: "thread-1".to_string(),
                            turn_id: "turn-origin".to_string(),
                            item_id: None,
                            tool_id: None,
                            agent_path: None,
                        },
                        payload: InteractionPayload::UserInput {
                            questions: Vec::new(),
                        },
                        created_at: 1,
                        updated_at: 7,
                        resolved_at: Some(7),
                        resolution: Some(InteractionResolution::UserInput {
                            answers: Default::default(),
                        }),
                    },
                },
            },
        };
        let mut current = snapshot();
        current.active_turn = Some(Turn {
            id: "turn-unrelated".to_string(),
            thread_id: "thread-1".to_string(),
            state: TurnState::InProgress {
                phase: TurnPhase::Thinking,
            },
            failure: None,
            started_at: Some(1),
            updated_at: 1,
            completed_at: None,
        });

        let projected = project_trace_events("thread-1", &current, &[trace]);

        assert_eq!(projected.notifications.len(), 1);
        assert!(matches!(
            projected.notifications[0].notification,
            ThreadNotification::InteractionChanged { .. }
        ));
    }

    fn snapshot() -> ThreadSnapshot {
        ThreadSnapshot {
            schema_version: THREAD_SCHEMA_VERSION,
            revision: 0,
            thread: Thread::placeholder("thread-1"),
            active_turn: None,
            items: Vec::new(),
            interactions: Vec::new(),
            runtime: None,
        }
    }
}
