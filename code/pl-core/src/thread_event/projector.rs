use pl_protocol::{
    ApprovedThreadTool, AwaitingApprovalThreadTool, BudgetLimitedTurnState, CancelledThreadAgent,
    CancelledThreadTool, CancelledTurnState, CompletedThreadInference, CompletedTurnState,
    DeniedThreadAgent, DeniedThreadTool, FailedThreadAgent, FailedThreadInference,
    FailedThreadTool, FailedTurnState, QueuedThreadAgent, RunningThreadAgent,
    RunningThreadInference, RunningThreadTool, RunningTurnState, StartedThreadTool,
    StreamingThreadTool, SucceededThreadAgent, SucceededThreadTool, ThreadAgentIdentity,
    ThreadAgentItem, ThreadAgentState, ThreadAttachment, ThreadContentLifecycle,
    ThreadInferenceItem, ThreadInferenceState, ThreadItem, ThreadItemDelta, ThreadItemDeltaState,
    ThreadItemState, ThreadNotification, ThreadNotificationEnvelope, ThreadSkillItem,
    ThreadSnapshot, ThreadTextChannel, ThreadTextItem, ThreadThinkingItem, ThreadToolFailure,
    ThreadToolFailureKind, ThreadToolInvocation, ThreadToolItem, ThreadToolOutput, ThreadToolState,
    ThreadTurnItem, Turn, TurnCancellationCause, TurnCompletion, TurnOutcome, TurnPhase, TurnState,
};
use pl_trace::{
    TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent, TracePartState,
    TraceTextChannel, TraceToolFailureKind, TraceToolState,
};

use crate::agent_runtime::{
    AgentRuntimeEvent, AgentRuntimeEventKind, DurableMailboxEnvelope, MailboxDeliveryState,
    MailboxInputSource,
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
    let mut projector = Projector::new(thread_id, current);
    let mut active_turn = current.active_turn.clone();
    let mut runtime = current
        .runtime
        .clone()
        .unwrap_or_else(|| super::observation::empty_runtime(thread_id));
    for trace in traces {
        match &trace.kind {
            TraceEventKind::TracePartStarted { item } => {
                if let Some(phase) = phase_for_item(item) {
                    projector.turn_updated(item.turn_id(), phase, trace.timestamp);
                }
                if let Some(item) = thread_item(thread_id, item) {
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
                if let Some(item) = thread_item(thread_id, item) {
                    projector.push(
                        trace.timestamp,
                        ThreadNotification::ItemCompleted {
                            item: Box::new(item),
                        },
                    );
                }
            }
            TraceEventKind::TracePartFailed { item } => {
                if let Some(item) = thread_item(thread_id, item) {
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
                    active_turn.as_ref(),
                    &interaction,
                    trace.timestamp,
                ) {
                    projector.push(trace.timestamp, ThreadNotification::TurnCompleted { turn });
                    active_turn = None;
                }
                projector.push(
                    trace.timestamp,
                    ThreadNotification::InteractionChanged {
                        interaction: Box::new(interaction),
                    },
                );
            }
            TraceEventKind::SkillActivated { activation } => {
                projector.push(
                    activation.activated_at,
                    ThreadNotification::ItemCompleted {
                        item: Box::new(ThreadItem::new(
                            format!(
                                "{}:skill-activation:{}",
                                activation.turn_id,
                                activation.item_identity()
                            ),
                            thread_id.to_string(),
                            activation.turn_id.clone(),
                            0,
                            0,
                            activation.activated_at,
                            activation.activated_at,
                            ThreadItemState::Skill(ThreadSkillItem::new(activation.clone())),
                        )),
                    },
                );
                if !runtime.active_skills.contains(&activation.name) {
                    runtime.active_skills.push(activation.name.clone());
                    runtime.updated_at = activation.activated_at;
                    projector.push(
                        activation.activated_at,
                        ThreadNotification::ThreadRuntimeUpdated {
                            runtime: Box::new(runtime.clone()),
                        },
                    );
                }
            }
            TraceEventKind::EnabledToolsRecorded { .. } => {}
        }
    }
    projector.finish()
}

pub(crate) fn project_runtime_event(
    event: &AgentRuntimeEvent,
    current: &ThreadSnapshot,
) -> ThreadProjectionBatch {
    let Some(thread_id) = runtime_event_thread_id(event) else {
        return ThreadProjectionBatch {
            through_revision: current.revision,
            ..ThreadProjectionBatch::default()
        };
    };
    let mut projector = Projector::new(thread_id, current);
    match &event.kind {
        AgentRuntimeEventKind::TurnQueued { input, .. } => {
            if !matches!(input.delivery_state, MailboxDeliveryState::Claimed { .. }) {
                projector.push(
                    event.created_at,
                    ThreadNotification::TurnStarted {
                        turn: Turn::queued(input.turn_id.as_str(), thread_id, event.created_at),
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
            input,
            claimed_inputs,
            ..
        } => {
            projector.push(
                event.created_at,
                ThreadNotification::TurnUpdated {
                    turn: projected_turn(
                        turn_id.as_str(),
                        thread_id,
                        TurnState::Running(RunningTurnState::new(
                            event.created_at,
                            TurnPhase::Preparing,
                        )),
                        event.created_at,
                    ),
                },
            );
            let mut runtime = current
                .runtime
                .clone()
                .unwrap_or_else(|| super::observation::empty_runtime(thread_id));
            runtime.turn_completion_tokens = 0;
            runtime.turn_decode_millis = 0;
            runtime.updated_at = event.created_at;
            projector.push(
                event.created_at,
                ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime),
                },
            );
            if !has_projected_input(current, input, turn_id.as_str()) {
                project_user_input(&mut projector, input, turn_id.as_str(), event.created_at);
            }
            for input in claimed_inputs {
                if input.turn_id.as_str() != turn_id.as_str() {
                    projector.push(
                        event.created_at,
                        ThreadNotification::TurnCompleted {
                            turn: projected_turn(
                                input.turn_id.as_str(),
                                thread_id,
                                TurnState::Cancelled(CancelledTurnState::new(
                                    None,
                                    event.created_at,
                                    event.created_at,
                                    TurnCancellationCause::Coalesced {
                                        target_turn_id: turn_id.to_string(),
                                    },
                                )),
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
            if let TurnOutcome::Failed(result) = &outcome.outcome {
                // A worker can fail before its TraceRecorder emits terminal
                // events (for example a task join failure). TurnFinished is the
                // last ownership boundary: do not clear active_turn while a
                // model/tool/content item from that Turn is still streaming.
                for item in current
                    .items
                    .iter()
                    .filter(|item| item.turn_id == outcome.turn_id.as_str() && !item.is_terminal())
                {
                    let mut failed = item.clone();
                    if failed.fail_if_open(outcome.finished_at, result.failure().message.clone()) {
                        projector.push(
                            event.created_at,
                            ThreadNotification::ItemCompleted {
                                item: Box::new(failed),
                            },
                        );
                    }
                }
            }
            let state = match &outcome.outcome {
                TurnOutcome::Completed(result) => TurnState::Completed(CompletedTurnState::new(
                    outcome.started_at,
                    outcome.finished_at,
                    result.completion(),
                )),
                TurnOutcome::Cancelled(result) => TurnState::Cancelled(CancelledTurnState::new(
                    outcome.started_at,
                    outcome.finished_at,
                    outcome.finished_at,
                    result.cause().clone(),
                )),
                TurnOutcome::Failed(result) => TurnState::Failed(FailedTurnState::new(
                    outcome.started_at,
                    outcome.finished_at,
                    result.failure().clone(),
                )),
                TurnOutcome::BudgetLimited(result) => {
                    TurnState::BudgetLimited(BudgetLimitedTurnState::new(
                        outcome.started_at,
                        outcome.finished_at,
                        *result.limit(),
                        result.rollover().clone(),
                    ))
                }
            };
            let completed_turn = projected_turn(
                outcome.turn_id.as_str(),
                thread_id,
                state,
                outcome.finished_at,
            );
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
    if !input.payload.presentation.is_visible() {
        return;
    }
    let item_id = input_item_id(input, turn_id);
    let item = match input.payload.source {
        MailboxInputSource::User => ThreadItem::completed_user_message(
            item_id,
            projector.thread_id.to_string(),
            turn_id.to_string(),
            input.payload.message.clone(),
            input.payload.attachments.clone(),
            input.queued_at,
        ),
        MailboxInputSource::ParentAgent => ThreadItem::completed_parent_agent_message(
            item_id,
            projector.thread_id.to_string(),
            turn_id.to_string(),
            input.payload.message.clone(),
            input.payload.attachments.clone(),
            input.queued_at,
        ),
    };
    projector.push(
        emitted_at,
        ThreadNotification::ItemCompleted {
            item: Box::new(item),
        },
    );
}

fn has_projected_input(
    current: &ThreadSnapshot,
    input: &DurableMailboxEnvelope,
    turn_id: &str,
) -> bool {
    let item_id = input_item_id(input, turn_id);
    current.items.iter().any(|item| item.id == item_id)
}

fn input_item_id(input: &DurableMailboxEnvelope, turn_id: &str) -> String {
    if input.mail_id.is_empty() {
        format!("{turn_id}:user")
    } else {
        format!("{turn_id}:mail:{}", input.mail_id)
    }
}

fn projected_turn(id: &str, thread_id: &str, state: TurnState, updated_at: i64) -> Turn {
    Turn {
        id: id.to_string(),
        thread_id: thread_id.to_string(),
        revision: 0,
        state,
        updated_at,
    }
}

pub(super) fn interaction_completion_turn(
    thread_id: &str,
    active_turn: Option<&Turn>,
    interaction: &pl_protocol::InteractionRequest,
    emitted_at: i64,
) -> Option<Turn> {
    let active_turn = active_turn.filter(|turn| {
        interaction.status() == pl_protocol::InteractionStatus::Pending
            && turn.id == interaction.scope.turn_id
    })?;
    Some({
        projected_turn(
            &interaction.scope.turn_id,
            thread_id,
            TurnState::Completed(CompletedTurnState::new(
                active_turn.started_at(),
                emitted_at,
                TurnCompletion::InteractionRequested,
            )),
            emitted_at,
        )
    })
}

fn phase_for_item(item: &TracePart) -> Option<TurnPhase> {
    match item.state() {
        // User inputs are projected exactly once from the durable mailbox event (see
        // thread_item below). The internal User trace is model-visible diagnostics only
        // and must not advance an active Turn out of Preparing.
        TracePartState::Text(text) => match text.channel() {
            TraceTextChannel::Commentary | TraceTextChannel::Final => Some(TurnPhase::Responding),
            TraceTextChannel::User => None,
        },
        TracePartState::Thinking(_) | TracePartState::Inference(_) => Some(TurnPhase::Thinking),
        TracePartState::Tool(_) | TracePartState::Agent(_) => Some(TurnPhase::RunningTool),
        TracePartState::Turn(_) => None,
    }
}

fn thread_item(thread_id: &str, item: &TracePart) -> Option<ThreadItem> {
    let state = match item.state() {
        TracePartState::Text(text) => match text.channel() {
            // User inputs are projected exactly once from the durable mailbox event.
            // Trace keeps the model-visible prompt for diagnostics, but is not a second
            // timeline source and must not reveal hidden control inputs.
            TraceTextChannel::User => return None,
            TraceTextChannel::Commentary | TraceTextChannel::Final => {
                ThreadItemState::Text(ThreadTextItem::new(
                    match text.channel() {
                        TraceTextChannel::Commentary => ThreadTextChannel::Commentary,
                        TraceTextChannel::Final => ThreadTextChannel::Final,
                        TraceTextChannel::User => unreachable!("user trace returned above"),
                    },
                    text.content().to_string(),
                    text.attachments().iter().map(thread_attachment).collect(),
                    text_lifecycle(text.state(), item.updated_at()),
                ))
            }
        },
        TracePartState::Thinking(thinking) => ThreadItemState::Thinking(ThreadThinkingItem::new(
            thinking
                .summary()
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
            thinking
                .content()
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
            thinking_lifecycle(thinking.state(), item.updated_at()),
        )),
        TracePartState::Tool(tool) => {
            ThreadItemState::Tool(thread_tool_item(tool, item.updated_at()))
        }
        TracePartState::Agent(agent) => {
            ThreadItemState::Agent(thread_agent_item(agent, item.updated_at()))
        }
        TracePartState::Turn(_) => {
            item.failure()?;
            let TracePartState::Turn(turn) = item.state() else {
                unreachable!("matched turn state")
            };
            ThreadItemState::Turn(ThreadTurnItem::new(turn.state().clone()))
        }
        TracePartState::Inference(inference) => {
            ThreadItemState::Inference(thread_inference_item(inference, item.updated_at()))
        }
    };
    Some(ThreadItem::new(
        item.item_id().to_string(),
        thread_id.to_string(),
        item.turn_id().to_string(),
        // ordinal 由 ThreadEventBus 首次应用时分配（到达序）；started_sequence 仅是
        // trace 事件自身的去重/批内排序键，不再兼任 timeline 顺序。
        0,
        item.revision(),
        item.created_at(),
        item.updated_at(),
        state,
    ))
}

fn thread_tool_item(tool: &pl_trace::TraceToolPart, updated_at: i64) -> ThreadToolItem {
    let invocation = tool.invocation();
    let state =
        match tool.state() {
            TraceToolState::Started(_) => ThreadToolState::Started(StartedThreadTool),
            TraceToolState::Streaming(_) => ThreadToolState::Streaming(StreamingThreadTool),
            TraceToolState::AwaitingApproval(_) => {
                ThreadToolState::AwaitingApproval(AwaitingApprovalThreadTool)
            }
            TraceToolState::Approved(_) => ThreadToolState::Approved(ApprovedThreadTool),
            TraceToolState::Running(value) => ThreadToolState::Running(RunningThreadTool::new(
                value.streamed_output().to_string(),
            )),
            TraceToolState::Succeeded(value) => ThreadToolState::Succeeded(
                SucceededThreadTool::new(updated_at, thread_tool_output(value.output())),
            ),
            TraceToolState::Failed(value) => ThreadToolState::Failed(FailedThreadTool::new(
                updated_at,
                ThreadToolFailure::new(
                    match value.failure().kind() {
                        TraceToolFailureKind::Execution => ThreadToolFailureKind::Execution,
                        TraceToolFailureKind::TimedOut => ThreadToolFailureKind::TimedOut,
                        TraceToolFailureKind::BudgetLimited => ThreadToolFailureKind::BudgetLimited,
                    },
                    value.failure().message().to_string(),
                ),
                value.output().map(thread_tool_output),
            )),
            TraceToolState::Denied(value) => ThreadToolState::Denied(DeniedThreadTool::new(
                updated_at,
                value.reason().to_string(),
            )),
            TraceToolState::Cancelled(value) => ThreadToolState::Cancelled(
                CancelledThreadTool::new(updated_at, format!("{:?}", value.cause())),
            ),
        };
    ThreadToolItem::new(
        ThreadToolInvocation::new(
            invocation.tool_call_id().to_string(),
            invocation.name().to_string(),
            invocation.arguments().to_string(),
        )
        .with_provider_identity(
            invocation.call_id().map(str::to_string),
            invocation.provider_item_id().map(str::to_string),
        )
        .with_working_directory(invocation.working_directory().map(str::to_string)),
        state,
    )
}

fn thread_tool_output(output: &pl_trace::TraceToolOutput) -> ThreadToolOutput {
    ThreadToolOutput::new(
        output.result().to_string(),
        output.attachments().iter().map(thread_attachment).collect(),
        output.output_artifacts().to_vec(),
        output.exit_code(),
    )
}

fn thread_attachment(value: &pl_trace::TraceAttachment) -> ThreadAttachment {
    ThreadAttachment {
        id: value.id.clone(),
        modality: match value.modality {
            pl_trace::TraceAttachmentModality::Image => pl_protocol::AttachmentModality::Image,
            pl_trace::TraceAttachmentModality::Video => pl_protocol::AttachmentModality::Video,
            pl_trace::TraceAttachmentModality::File => pl_protocol::AttachmentModality::File,
        },
        media_type: value.media_type.clone(),
        filename: value.filename.clone(),
        width: value.width,
        height: value.height,
        byte_size: value.byte_size,
    }
}

fn text_lifecycle(state: &pl_trace::TraceTextState, updated_at: i64) -> ThreadContentLifecycle {
    match state {
        pl_trace::TraceTextState::Streaming(_) => ThreadContentLifecycle::streaming(),
        pl_trace::TraceTextState::Completed(_) => ThreadContentLifecycle::completed(updated_at),
        pl_trace::TraceTextState::Failed(value) => {
            ThreadContentLifecycle::failed(updated_at, value.error().to_string())
        }
        pl_trace::TraceTextState::Cancelled(value) => {
            ThreadContentLifecycle::cancelled(updated_at, value.reason().to_string())
        }
    }
}

fn thinking_lifecycle(
    state: &pl_trace::TraceThinkingState,
    updated_at: i64,
) -> ThreadContentLifecycle {
    match state {
        pl_trace::TraceThinkingState::Streaming(_) => ThreadContentLifecycle::streaming(),
        pl_trace::TraceThinkingState::Completed(_) => ThreadContentLifecycle::completed(updated_at),
        pl_trace::TraceThinkingState::Failed(value) => {
            ThreadContentLifecycle::failed(updated_at, value.error().to_string())
        }
        pl_trace::TraceThinkingState::Cancelled(value) => {
            ThreadContentLifecycle::cancelled(updated_at, value.reason().to_string())
        }
    }
}

fn thread_agent_item(agent: &pl_trace::TraceAgentPart, updated_at: i64) -> ThreadAgentItem {
    let identity = agent.identity();
    let state = match agent.state() {
        pl_trace::TraceAgentState::Queued(_) => ThreadAgentState::Queued(QueuedThreadAgent),
        pl_trace::TraceAgentState::Running(_) => ThreadAgentState::Running(RunningThreadAgent),
        pl_trace::TraceAgentState::Succeeded(value) => ThreadAgentState::Succeeded(
            SucceededThreadAgent::new(updated_at, value.summary().to_string()),
        ),
        pl_trace::TraceAgentState::Denied(value) => ThreadAgentState::Denied(
            DeniedThreadAgent::new(updated_at, value.reason().to_string()),
        ),
        pl_trace::TraceAgentState::Cancelled(value) => ThreadAgentState::Cancelled(
            CancelledThreadAgent::new(updated_at, value.reason().to_string()),
        ),
        pl_trace::TraceAgentState::Failed(value) => ThreadAgentState::Failed(
            FailedThreadAgent::new(updated_at, value.error().to_string()),
        ),
    };
    ThreadAgentItem::new(
        ThreadAgentIdentity::new(
            identity.id().to_string(),
            identity.path().to_string(),
            identity.role().to_string(),
            identity.task().to_string(),
            identity.depth(),
        )
        .with_parent_path(identity.parent_path().map(str::to_string)),
        state,
    )
}

fn thread_inference_item(
    inference: &pl_trace::TraceInferencePart,
    updated_at: i64,
) -> ThreadInferenceItem {
    let state = match inference.state() {
        pl_trace::TraceInferenceState::Running(_) => {
            ThreadInferenceState::Running(RunningThreadInference)
        }
        pl_trace::TraceInferenceState::Completed(value) => ThreadInferenceState::Completed(
            CompletedThreadInference::new(updated_at, value.usage().clone()),
        ),
        pl_trace::TraceInferenceState::Failed(value) => ThreadInferenceState::Failed(
            FailedThreadInference::new(updated_at, value.error().to_string()),
        ),
        pl_trace::TraceInferenceState::Cancelled(value) => ThreadInferenceState::Cancelled(
            pl_protocol::CancelledThreadInference::new(updated_at, value.reason().to_string()),
        ),
    };
    ThreadInferenceItem::new(
        inference.inference_id().to_string(),
        inference.model().to_string(),
        state,
    )
}

fn item_delta(event: &TracePartDeltaEvent) -> Option<ThreadItemDelta> {
    let delta = match &event.delta {
        TraceDelta::Text { delta, .. } => ThreadItemDeltaState::Text {
            delta: delta.clone(),
        },
        TraceDelta::Thinking { chunk_index, delta } => ThreadItemDeltaState::ThinkingSummary {
            chunk_index: *chunk_index,
            delta: delta.clone(),
        },
        TraceDelta::ReasoningContent { chunk_index, delta } => {
            ThreadItemDeltaState::ThinkingContent {
                chunk_index: *chunk_index,
                delta: delta.clone(),
            }
        }
        TraceDelta::ToolArguments { delta } => ThreadItemDeltaState::ToolArguments {
            delta: delta.clone(),
        },
        TraceDelta::ToolResult { delta } => ThreadItemDeltaState::ToolResult {
            delta: delta.clone(),
        },
    };
    Some(ThreadItemDelta {
        item_id: event.item_id.clone(),
        revision: event.revision,
        delta,
    })
}

struct Projector<'a> {
    thread_id: &'a str,
    revision: u64,
    active_turn_started_at: Option<i64>,
    notifications: Vec<ThreadNotificationEnvelope>,
}

impl<'a> Projector<'a> {
    fn new(thread_id: &'a str, current: &ThreadSnapshot) -> Self {
        Self {
            thread_id,
            revision: current.revision,
            active_turn_started_at: current.active_turn.as_ref().and_then(Turn::started_at),
            notifications: Vec::new(),
        }
    }

    fn turn_updated(&mut self, turn_id: &str, phase: TurnPhase, emitted_at: i64) {
        let started_at = *self.active_turn_started_at.get_or_insert(emitted_at);
        self.push(
            emitted_at,
            ThreadNotification::TurnUpdated {
                turn: projected_turn(
                    turn_id,
                    self.thread_id,
                    TurnState::Running(RunningTurnState::new(started_at, phase)),
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
        AttachmentModality, InteractionChangedEvent, InteractionCommand, InteractionRequest,
        InteractionScope, ResolveUserInput, RunningTurnState, SkillActivation,
        THREAD_SCHEMA_VERSION, Thread, ThreadAttachment, ThreadItemState, ThreadSnapshot,
    };
    use pl_trace::{
        RunningTraceAgent, TraceAgentIdentity, TraceAgentPart, TraceAgentState, TracePartAction,
        TracePartCompletion, TraceToolInvocation,
    };

    use super::*;

    #[test]
    fn turn_started_clears_only_the_selected_agents_turn_throughput() {
        let mut current = snapshot();
        let mut runtime = super::super::observation::empty_runtime("thread-1");
        runtime.turn_completion_tokens = 150;
        runtime.turn_decode_millis = 1_000;
        runtime.usage.completion_tokens = 600;
        current.runtime = Some(runtime);
        let agent_snapshot = crate::AgentRegistration {
            identity: crate::AgentIdentity {
                id: crate::ThreadId::new("thread-1").unwrap(),
                parent_id: None,
                role: crate::AgentRoleId::new("planner").unwrap(),
                depth: 0,
            },
            session: crate::ThreadContextState::empty(),
            runtime_revision: 1,
            event_sequence: 1,
        }
        .into_durable_state()
        .snapshot;
        let input = DurableMailboxEnvelope {
            mail_id: "mail-1".to_string(),
            turn_id: crate::TurnId::new("turn-2").unwrap(),
            thread_id: crate::ThreadId::new("thread-1").unwrap(),
            payload: crate::MailboxInputPayload {
                message: "continue".to_string(),
                attachments: Vec::new(),
                source: Default::default(),
                presentation: Default::default(),
                metadata: crate::MailboxMetadata::default(),
            },
            queue_coalescing_key: None,
            budget_action: crate::MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 8,
        };
        let event = AgentRuntimeEvent {
            agent_id: crate::ThreadId::new("thread-1").unwrap(),
            sequence: 2,
            created_at: 9,
            kind: AgentRuntimeEventKind::TurnStarted {
                turn_id: crate::TurnId::new("turn-2").unwrap(),
                thread_id: crate::ThreadId::new("thread-1").unwrap(),
                input,
                claimed_inputs: Vec::new(),
                snapshot: Box::new(agent_snapshot),
            },
        };

        let batch = project_runtime_event(&event, &current);
        assert!(batch.notifications.iter().any(|notification| matches!(
            &notification.notification,
            ThreadNotification::ThreadRuntimeUpdated { runtime }
                if runtime.turn_completion_tokens == 0
                    && runtime.turn_decode_millis == 0
                    && runtime.usage.completion_tokens == 600
        )));
    }

    #[test]
    fn durable_user_input_projects_its_typed_attachment_manifest() {
        let current = snapshot();
        let mut projector = Projector::new("thread-1", &current);
        let attachment = ThreadAttachment {
            id: "attachment-1".to_string(),
            modality: AttachmentModality::Image,
            media_type: "image/png".to_string(),
            filename: Some("marker.png".to_string()),
            width: Some(1200),
            height: Some(800),
            byte_size: 80_000,
        };
        let input = DurableMailboxEnvelope {
            mail_id: "mail-1".to_string(),
            turn_id: crate::TurnId::new("turn-1").unwrap(),
            thread_id: crate::ThreadId::new("thread-1").unwrap(),
            payload: crate::MailboxInputPayload {
                message: "inspect".to_string(),
                attachments: vec![attachment.clone()],
                source: Default::default(),
                presentation: Default::default(),
                metadata: crate::MailboxMetadata::default(),
            },
            queue_coalescing_key: None,
            budget_action: crate::MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 7,
        };

        project_user_input(&mut projector, &input, "turn-1", 7);
        let batch = projector.finish();

        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if item.text().is_some_and(|text| text.attachments() == [attachment])
        ));
    }

    #[test]
    fn hidden_message_is_not_emitted_to_thread_gui_projection() {
        let current = snapshot();
        let mut projector = Projector::new("thread-1", &current);
        let input = DurableMailboxEnvelope {
            mail_id: "mail-hidden".to_string(),
            turn_id: crate::TurnId::new("turn-hidden").unwrap(),
            thread_id: crate::ThreadId::new("thread-1").unwrap(),
            payload: crate::MailboxInputPayload {
                message: "# Approved Plan\n\nInternal continuation.".to_string(),
                attachments: Vec::new(),
                source: Default::default(),
                presentation: pl_protocol::MessagePresentation::Hidden,
                metadata: crate::MailboxMetadata::default(),
            },
            queue_coalescing_key: None,
            budget_action: crate::MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 7,
        };

        project_user_input(&mut projector, &input, "turn-hidden", 7);
        assert!(projector.finish().notifications.is_empty());
    }

    #[test]
    fn parent_agent_input_projects_a_distinct_text_channel() {
        let current = snapshot();
        let mut projector = Projector::new("thread-1", &current);
        let input = DurableMailboxEnvelope {
            mail_id: "mail-parent".to_string(),
            turn_id: crate::TurnId::new("turn-parent").unwrap(),
            thread_id: crate::ThreadId::new("thread-1").unwrap(),
            payload: crate::MailboxInputPayload::parent_agent("check the latest result"),
            queue_coalescing_key: None,
            budget_action: crate::MailboxBudgetAction::Refresh,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 7,
        };

        project_user_input(&mut projector, &input, "turn-parent", 7);
        let batch = projector.finish();

        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if item.text().is_some_and(|text|
                    text.channel() == ThreadTextChannel::ParentAgent
                        && text.text() == "check the latest result")
        ));
    }

    #[test]
    fn turn_started_projects_an_unseen_primary_input_without_duplicating_an_existing_item() {
        let input = DurableMailboxEnvelope {
            mail_id: "mail-initial".to_string(),
            turn_id: crate::TurnId::new("turn-initial").unwrap(),
            thread_id: crate::ThreadId::new("thread-1").unwrap(),
            payload: crate::MailboxInputPayload::parent_agent("initial assignment"),
            queue_coalescing_key: None,
            budget_action: crate::MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 7,
        };
        let agent_snapshot = crate::AgentRegistration {
            identity: crate::AgentIdentity {
                id: crate::ThreadId::new("thread-1").unwrap(),
                parent_id: Some(crate::ThreadId::new("root").unwrap()),
                role: crate::AgentRoleId::new("executor").unwrap(),
                depth: 1,
            },
            session: crate::ThreadContextState::empty(),
            runtime_revision: 1,
            event_sequence: 1,
        }
        .into_durable_state()
        .snapshot;
        let event = AgentRuntimeEvent {
            agent_id: crate::ThreadId::new("thread-1").unwrap(),
            sequence: 2,
            created_at: 8,
            kind: AgentRuntimeEventKind::TurnStarted {
                turn_id: crate::TurnId::new("turn-initial").unwrap(),
                thread_id: crate::ThreadId::new("thread-1").unwrap(),
                input,
                claimed_inputs: Vec::new(),
                snapshot: Box::new(agent_snapshot),
            },
        };

        let first = project_runtime_event(&event, &snapshot());
        let projected_item = first
            .notifications
            .iter()
            .find_map(|notification| match &notification.notification {
                ThreadNotification::ItemCompleted { item } => Some(item.as_ref().clone()),
                _ => None,
            })
            .expect("unseen primary input must be projected");
        assert_eq!(projected_item.id, "turn-initial:mail:mail-initial");

        let mut current = snapshot();
        current.items.push(projected_item);
        let repeated = project_runtime_event(&event, &current);
        assert!(!repeated.notifications.iter().any(|notification| matches!(
            &notification.notification,
            ThreadNotification::ItemCompleted { .. }
        )));
    }

    #[test]
    fn trace_text_is_one_item_not_message_and_part() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::TracePartCompleted {
                item: TracePart::completed_text(
                    "turn-1",
                    "item-1",
                    1,
                    TraceTextChannel::Final,
                    "done",
                    Vec::new(),
                    7,
                ),
            },
        };
        let batch = project_trace_events("thread-1", &snapshot(), &[trace]);
        assert_eq!(batch.notifications.len(), 1);
        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if matches!(item.state(), ThreadItemState::Text(_))
        ));
    }

    #[test]
    fn every_trace_delta_projects_to_an_independent_thread_notification() {
        let mut item = TracePart::streaming_text("turn-1", "item-1", 1, TraceTextChannel::Final, 7);
        let started = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::TracePartStarted { item: item.clone() },
        };
        let mut traces = vec![started];
        for (sequence, delta) in [(2, "a"), (3, "b")] {
            item.apply(item.command(
                sequence as i64 + 6,
                TracePartAction::Append(TraceDelta::Text {
                    channel: TraceTextChannel::Final,
                    delta: delta.to_string(),
                }),
            ))
            .unwrap();
            traces.push(TraceEvent {
                session_id: "thread-1".to_string(),
                sequence,
                timestamp: sequence as i64 + 6,
                kind: TraceEventKind::TracePartDelta {
                    event: TracePartDeltaEvent {
                        turn_id: "turn-1".to_string(),
                        item_id: "item-1".to_string(),
                        started_sequence: 1,
                        revision: item.revision(),
                        created_at: 7,
                        updated_at: sequence as i64 + 6,
                        delta: TraceDelta::Text {
                            channel: TraceTextChannel::Final,
                            delta: delta.to_string(),
                        },
                    },
                },
            });
        }
        item.apply(item.command(
            10,
            TracePartAction::Complete(TracePartCompletion::Text {
                authoritative_content: Some("ab".to_string()),
            }),
        ))
        .unwrap();
        traces.push(TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 4,
            timestamp: 10,
            kind: TraceEventKind::TracePartCompleted { item },
        });

        let batch = project_trace_events("thread-1", &snapshot(), &traces);
        let item_notifications = batch
            .notifications
            .iter()
            .filter_map(|notification| match &notification.notification {
                ThreadNotification::ItemStarted { .. } => Some("started"),
                ThreadNotification::ItemDelta { delta } => match &delta.delta {
                    ThreadItemDeltaState::Text { delta } => Some(delta.as_str()),
                    _ => None,
                },
                ThreadNotification::ItemCompleted { .. } => Some("completed"),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(item_notifications, ["started", "a", "b", "completed"]);
    }

    #[test]
    fn user_trace_does_not_duplicate_or_reveal_mailbox_input() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::TracePartCompleted {
                item: TracePart::completed_text(
                    "turn-1",
                    "internal-user-trace",
                    1,
                    TraceTextChannel::User,
                    "hidden control input",
                    Vec::new(),
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
    fn failed_turn_trace_projects_a_durable_timeline_error() {
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 3,
            timestamp: 7,
            kind: TraceEventKind::TracePartFailed {
                item: TracePart::turn(
                    "turn-1".to_string(),
                    "turn-1-turn".to_string(),
                    3,
                    7,
                    TurnState::Failed(FailedTurnState::new(
                        Some(6),
                        7,
                        pl_protocol::TurnFailure::permanent(
                            pl_protocol::TurnFailureCategory::Provider,
                            "provider rejected tool schema",
                        ),
                    )),
                ),
            },
        };

        let batch = project_trace_events("thread-1", &snapshot(), &[trace]);

        assert_eq!(batch.notifications.len(), 1);
        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if item.failure() == Some("provider rejected tool schema")
                    && matches!(item.state(), ThreadItemState::Turn(_))
        ));
    }

    #[test]
    fn failed_turn_terminalizes_a_running_inference_when_worker_emits_no_final_trace() {
        let mut current = snapshot();
        current.active_turn = Some(Turn {
            id: "turn-1".to_string(),
            thread_id: "thread-1".to_string(),
            revision: 0,
            state: TurnState::Running(RunningTurnState::new(6, TurnPhase::Thinking)),
            updated_at: 6,
        });
        current.items.push(ThreadItem::new(
            "turn-1-inf-0".to_string(),
            "thread-1".to_string(),
            "turn-1".to_string(),
            1,
            0,
            6,
            6,
            ThreadItemState::Inference(ThreadInferenceItem::new(
                "turn-1-inf-0".to_string(),
                "model".to_string(),
                ThreadInferenceState::Running(RunningThreadInference),
            )),
        ));
        let agent_snapshot = crate::AgentRegistration {
            identity: crate::AgentIdentity {
                id: crate::ThreadId::new("thread-1").unwrap(),
                parent_id: None,
                role: crate::AgentRoleId::new("planner").unwrap(),
                depth: 0,
            },
            session: crate::ThreadContextState::empty(),
            runtime_revision: 1,
            event_sequence: 1,
        }
        .into_durable_state()
        .snapshot;
        let event = AgentRuntimeEvent {
            agent_id: crate::ThreadId::new("thread-1").unwrap(),
            sequence: 2,
            created_at: 7,
            kind: AgentRuntimeEventKind::TurnFinished {
                outcome: pl_protocol::AgentTurnOutcome {
                    turn_id: crate::TurnId::new("turn-1").unwrap(),
                    thread_id: crate::ThreadId::new("thread-1").unwrap(),
                    outcome: TurnOutcome::failed(pl_protocol::TurnFailure::permanent(
                        pl_protocol::TurnFailureCategory::Provider,
                        "turn task join failed",
                    )),
                    usage: pl_protocol::InferenceTokenUsage::default(),
                    started_at: Some(6),
                    finished_at: 7,
                },
                snapshot: Box::new(agent_snapshot),
            },
        };

        let batch = project_runtime_event(&event, &current);

        assert!(batch.notifications.iter().any(|notification| matches!(
            &notification.notification,
            ThreadNotification::ItemCompleted { item }
                if item.id == "turn-1-inf-0"
                    && item.failure() == Some("turn task join failed")
                    && matches!(item.state(), ThreadItemState::Inference(inference)
                        if matches!(inference.state(), ThreadInferenceState::Failed(_)))
        )));
        assert!(matches!(
            &batch.notifications.last().unwrap().notification,
            ThreadNotification::TurnCompleted { turn }
                if turn.id == "turn-1" && matches!(turn.state, TurnState::Failed(_))
        ));
    }

    #[test]
    fn repeated_skill_activations_keep_items_and_dedupe_runtime_names() {
        let traces = [
            skill_trace(1, "tool-1", "pdf", 7),
            skill_trace(2, "tool-2", "pdf", 8),
        ];

        let batch = project_trace_events("thread-1", &snapshot(), &traces);

        assert_eq!(batch.notifications.len(), 3);
        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if item.id == "turn-1:skill-activation:tool-1"
                    && matches!(item.state(), ThreadItemState::Skill(skill)
                        if skill.activation().name == "pdf")
        ));
        assert!(matches!(
            &batch.notifications[1].notification,
            ThreadNotification::ThreadRuntimeUpdated { runtime }
                if runtime.active_skills == ["pdf"]
        ));
        assert!(matches!(
            &batch.notifications[2].notification,
            ThreadNotification::ItemCompleted { item }
                if item.id == "turn-1:skill-activation:tool-2"
        ));
    }

    #[test]
    fn different_skill_activations_preserve_first_activation_order() {
        let traces = [
            skill_trace(1, "tool-1", "doc", 7),
            skill_trace(2, "tool-2", "pdf", 8),
        ];

        let batch = project_trace_events("thread-1", &snapshot(), &traces);

        assert!(matches!(
            &batch.notifications[3].notification,
            ThreadNotification::ThreadRuntimeUpdated { runtime }
                if runtime.active_skills == ["doc", "pdf"]
        ));
    }

    #[test]
    fn resolved_interaction_trace_does_not_update_unrelated_active_turn() {
        let mut interaction = InteractionRequest::user_input(
            "ask-1",
            InteractionScope {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-origin".to_string(),
                item_id: None,
                tool_id: None,
                agent_path: None,
                purpose: pl_protocol::InteractionPurpose::General,
            },
            Vec::new(),
            1,
        );
        let decision = interaction
            .decide(InteractionCommand::ResolveUserInput(ResolveUserInput {
                interaction_id: interaction.interaction_id.clone(),
                expected_revision: interaction.revision,
                operation_id: "resolve-1".to_string(),
                resolved_at: 7,
                answers: Default::default(),
            }))
            .unwrap();
        interaction.apply(decision, 7);
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::InteractionChanged {
                event: InteractionChangedEvent { interaction },
            },
        };
        let mut current = snapshot();
        current.active_turn = Some(Turn {
            id: "turn-unrelated".to_string(),
            thread_id: "thread-1".to_string(),
            revision: 0,
            state: TurnState::Running(RunningTurnState::new(1, TurnPhase::Thinking)),
            updated_at: 1,
        });

        let projected = project_trace_events("thread-1", &current, &[trace]);

        assert_eq!(projected.notifications.len(), 1);
        assert!(matches!(
            projected.notifications[0].notification,
            ThreadNotification::InteractionChanged { .. }
        ));
    }

    #[test]
    fn inference_and_tool_starts_publish_wait_phases_before_items() {
        let inference = started_trace(TracePart::running_inference(
            "turn-1".to_string(),
            "inference-1".to_string(),
            1,
            7,
            "inf-1".to_string(),
            "claude".to_string(),
        ));
        let tool = started_trace(TracePart::started_tool(
            "turn-1".to_string(),
            "tool-1".to_string(),
            2,
            8,
            TraceToolInvocation::new(
                "call-1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            ),
        ));
        let agent = started_trace(TracePart::agent(
            "turn-1".to_string(),
            "agent-1".to_string(),
            3,
            9,
            TraceAgentPart::new(
                TraceAgentIdentity::new(
                    "agent-1".to_string(),
                    "/sub".to_string(),
                    "executor".to_string(),
                    "do work".to_string(),
                    1,
                ),
                TraceAgentState::Running(RunningTraceAgent),
            ),
        ));

        // 每个外部等待 start 都必须先投影 canonical wait phase，再发布对应 ItemStarted；
        // 客户端在请求发起到终态之间始终有 typed 活动状态，没有无状态窗口。
        for (trace, expected_phase) in [
            (inference, TurnPhase::Thinking),
            (tool, TurnPhase::RunningTool),
            (agent, TurnPhase::RunningTool),
        ] {
            let batch = project_trace_events("thread-1", &snapshot(), &[trace]);
            assert_wait_phase_then_item(&batch, expected_phase);
        }
    }

    #[test]
    fn user_trace_keeps_preparing_until_inference_starts() {
        // 真实 Turn 先 start/complete 内部 User trace，再建立 Inference item。User 输入
        // 只从 durable mailbox 投影一次（见 thread_item），其 trace 必须对展示 phase 完全
        // 中性：active Turn 保持 Preparing，直到 Inference start 才切到 Thinking。
        let user_item = TracePart::completed_text(
            "turn-1",
            "turn-1-user",
            1,
            TraceTextChannel::User,
            "hello".to_string(),
            Vec::new(),
            1,
        );
        let user_start = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 1,
            kind: TraceEventKind::TracePartStarted {
                item: user_item.clone(),
            },
        };
        let user_complete = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 2,
            timestamp: 2,
            kind: TraceEventKind::TracePartCompleted { item: user_item },
        };

        // User trace 单独投影时零 notification：既不产生 TurnUpdated，也不产生 Item。
        let user_only = project_trace_events(
            "thread-1",
            &snapshot(),
            &[user_start.clone(), user_complete.clone()],
        );
        assert!(
            user_only.notifications.is_empty(),
            "user trace must not publish TurnUpdated or Item, got {:?}",
            user_only.notifications
        );

        // 随后 Inference start 才先投影 TurnUpdated(Thinking)，再发布 ItemStarted。
        let inference = started_trace(TracePart::running_inference(
            "turn-1".to_string(),
            "inference-1".to_string(),
            3,
            7,
            "inf-1".to_string(),
            "claude".to_string(),
        ));
        let batch = project_trace_events(
            "thread-1",
            &snapshot(),
            &[user_start, user_complete, inference],
        );
        assert_wait_phase_then_item(&batch, TurnPhase::Thinking);
    }

    fn started_trace(item: TracePart) -> TraceEvent {
        TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: item.started_sequence(),
            timestamp: item.created_at(),
            kind: TraceEventKind::TracePartStarted { item },
        }
    }

    fn assert_wait_phase_then_item(batch: &ThreadProjectionBatch, expected_phase: TurnPhase) {
        assert_eq!(
            batch.notifications.len(),
            2,
            "expected exactly TurnUpdated then ItemStarted, got {:?}",
            batch
                .notifications
                .iter()
                .map(|envelope| &envelope.notification)
                .collect::<Vec<_>>()
        );
        assert!(
            matches!(
                &batch.notifications[0].notification,
                ThreadNotification::TurnUpdated { turn }
                    if turn.phase() == Some(expected_phase)
            ),
            "expected first notification to be TurnUpdated({expected_phase:?}), got {:?}",
            batch.notifications[0].notification
        );
        assert!(
            matches!(
                &batch.notifications[1].notification,
                ThreadNotification::ItemStarted { .. }
            ),
            "expected second notification to be ItemStarted, got {:?}",
            batch.notifications[1].notification
        );
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

    fn skill_trace(sequence: u64, tool_call_id: &str, name: &str, activated_at: i64) -> TraceEvent {
        TraceEvent {
            session_id: "thread-1".to_string(),
            sequence,
            timestamp: activated_at,
            kind: TraceEventKind::SkillActivated {
                activation: SkillActivation {
                    name: name.to_string(),
                    source: "system".to_string(),
                    provider_id: "local-filesystem".to_string(),
                    resource_base: pl_protocol::SkillActivationResourceBase::Directory {
                        path: format!("/skills/{name}"),
                    },
                    turn_id: "turn-1".to_string(),
                    cause: pl_protocol::SkillActivationCause::Tool {
                        tool_call_id: tool_call_id.to_string(),
                    },
                    activated_at,
                },
            },
        }
    }

    #[test]
    fn user_gesture_activation_uses_invocation_identity_and_shared_runtime_projection() {
        let snapshot = snapshot();
        let trace = TraceEvent {
            session_id: "thread-1".to_string(),
            sequence: 1,
            timestamp: 7,
            kind: TraceEventKind::SkillActivated {
                activation: SkillActivation {
                    name: "doc".to_string(),
                    source: "user".to_string(),
                    provider_id: "local-filesystem".to_string(),
                    resource_base: pl_protocol::SkillActivationResourceBase::Directory {
                        path: "/skills/doc".to_string(),
                    },
                    turn_id: "turn-1".to_string(),
                    cause: pl_protocol::SkillActivationCause::UserGesture {
                        invocation_id: "user-skill-0".to_string(),
                    },
                    activated_at: 7,
                },
            },
        };

        let batch = project_trace_events("thread-1", &snapshot, &[trace]);

        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ItemCompleted { item }
                if item.id == "turn-1:skill-activation:user-skill-0"
        ));
        assert!(matches!(
            &batch.notifications[1].notification,
            ThreadNotification::ThreadRuntimeUpdated { runtime }
                if runtime.active_skills == ["doc"]
        ));
    }
}
