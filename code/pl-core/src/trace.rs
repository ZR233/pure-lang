use crate::time::unix_seconds;
use pl_protocol::{
    BudgetLimitedTurnState, CancelledTurnState, CompletedTurnState, FailedTurnState,
    RunningTurnState, TokenUsageSnapshot, TurnOutcome, TurnPhase, TurnState,
};
use pl_trace::{
    AgentEvent, AgentEventSender, TraceAgentPart, TraceDelta, TraceEvent, TraceEventKind,
    TracePart, TracePartAction, TracePartCompletion, TracePartKind, TracePartState,
    TraceTextChannel, TraceToolInvocation,
};

/// In-memory trace recorder that captures structured lifecycle events during a turn.
///
/// Wraps an `AgentEventSender` and simultaneously:
/// - Passes `AgentEvent`s through to the broadcast channel (unchanged behavior)
/// - Appends item-first `TraceEvent`s to an in-memory log
/// - Agent runtime 模式下同步送入 durable channel，由 actor 先持久化再广播
///
/// When tracing is not needed, use `TraceRecorder::disabled()` which still
/// forwards broadcasts but discards trace events.
pub struct TraceRecorder {
    session_id: String,
    event_tx: AgentEventSender,
    durable_tx: Option<tokio::sync::mpsc::UnboundedSender<TraceEvent>>,
    events: Vec<TraceEvent>,
    sequence: u64,
    disabled: bool,
}

impl TraceRecorder {
    /// Create a recorder that captures trace events.
    pub fn new(session_id: String, event_tx: AgentEventSender, starting_sequence: u64) -> Self {
        Self {
            session_id,
            event_tx,
            durable_tx: None,
            events: Vec::new(),
            sequence: starting_sequence,
            disabled: false,
        }
    }

    /// 创建同时把 trace 送入 agent runtime durable channel 的 recorder。
    pub(crate) fn streaming(
        session_id: String,
        event_tx: AgentEventSender,
        starting_sequence: u64,
        durable_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
    ) -> Self {
        Self {
            session_id,
            event_tx,
            durable_tx: Some(durable_tx),
            events: Vec::new(),
            sequence: starting_sequence,
            disabled: false,
        }
    }

    /// Create a no-op recorder that forwards broadcasts but discards trace events.
    pub fn disabled(event_tx: AgentEventSender) -> Self {
        Self {
            session_id: String::new(),
            event_tx,
            durable_tx: None,
            events: Vec::new(),
            sequence: 0,
            disabled: true,
        }
    }

    /// Record a trace event only (no corresponding AgentEvent broadcast).
    pub fn record_trace_only(&mut self, kind: TraceEventKind) {
        if self.disabled {
            return;
        }
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp: unix_seconds(),
            kind,
        };
        self.sequence += 1;
        self.push_event(event);
    }

    pub fn record_event(&mut self, mut event: TraceEvent) {
        if self.disabled {
            return;
        }
        // recorder 是 sequence 的唯一分配者：强制覆盖 event 自带的 sequence（pl-model
        // projection 每 turn 从 0，不再可信），保证 recorder.events 跨 turn 全局单调。
        event.sequence = self.sequence;
        event.session_id = self.session_id.clone();
        self.sequence += 1;
        self.push_event(event);
    }

    pub fn record_events(&mut self, events: Vec<TraceEvent>) {
        for event in events {
            self.record_event(event);
        }
    }

    pub fn start_item(&mut self, item: TracePart) {
        self.record_and_broadcast_item_start(item);
    }

    pub fn update_item_snapshot(&mut self, item: TracePart) {
        self.record_and_broadcast_item_start(item);
    }

    pub fn complete_item(&mut self, item: TracePart) {
        if self.disabled {
            self.broadcast(AgentEvent::TracePartCompleted { item });
            return;
        }
        let sequence = self.sequence;
        let timestamp = item.updated_at();
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TracePartCompleted { item: item.clone() },
        };
        self.sequence += 1;
        self.push_event(event);
        self.broadcast(AgentEvent::TracePartCompleted { item });
    }

    pub fn fail_item(&mut self, item: TracePart) {
        if self.disabled {
            self.broadcast(AgentEvent::TracePartFailed { item });
            return;
        }
        let sequence = self.sequence;
        let timestamp = item.updated_at();
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TracePartFailed { item: item.clone() },
        };
        self.sequence += 1;
        self.push_event(event);
        self.broadcast(AgentEvent::TracePartFailed { item });
    }

    pub fn user_text_item(&mut self, turn_id: &str, content: String) {
        self.user_text_item_with_attachments(turn_id, content, Vec::new());
    }

    pub fn user_text_item_with_attachments(
        &mut self,
        turn_id: &str,
        content: String,
        attachments: Vec<pl_trace::TraceAttachment>,
    ) {
        self.user_text_item_with_id(turn_id, format!("{turn_id}-user"), content, attachments);
    }

    pub(crate) fn user_text_item_with_id(
        &mut self,
        turn_id: &str,
        item_id: String,
        content: String,
        attachments: Vec<pl_trace::TraceAttachment>,
    ) {
        let timestamp = unix_seconds();
        let item = TracePart::completed_text(
            turn_id,
            item_id,
            self.sequence,
            TraceTextChannel::User,
            content,
            attachments,
            timestamp,
        );
        self.record_and_broadcast_item_start(item.clone());
        self.complete_item(item);
    }

    pub fn ensure_assistant_text_item(&mut self, turn_id: &str, content: &str) {
        if content.trim().is_empty() || self.has_assistant_text_content(turn_id) {
            return;
        }
        let timestamp = unix_seconds();
        let item = TracePart::completed_text(
            turn_id,
            format!("{turn_id}-assistant"),
            self.sequence,
            TraceTextChannel::Final,
            content.to_string(),
            Vec::new(),
            timestamp,
        );
        self.record_and_broadcast_item_start(item.clone());
        self.complete_item(item);
    }

    pub fn running_turn_item(&mut self, turn_id: &str) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::turn(
            turn_id.to_string(),
            format!("{turn_id}-turn"),
            self.sequence,
            timestamp,
            TurnState::Running(RunningTurnState::new(timestamp, TurnPhase::Preparing)),
        )
    }

    pub fn terminal_turn_item(&mut self, turn_id: &str, outcome: &TurnOutcome) -> TracePart {
        let timestamp = unix_seconds();
        let mut item = self
            .latest_trace_part(&format!("{turn_id}-turn"))
            .unwrap_or_else(|| self.running_turn_item(turn_id));
        let started_at = match item.state() {
            TracePartState::Turn(turn) => turn.state().started_at(),
            TracePartState::Text(_)
            | TracePartState::Thinking(_)
            | TracePartState::Tool(_)
            | TracePartState::Agent(_)
            | TracePartState::Inference(_)
            | TracePartState::Plan(_) => None,
        };
        let state = match outcome {
            TurnOutcome::Completed(completed) => TurnState::Completed(CompletedTurnState::new(
                started_at,
                timestamp,
                completed.completion(),
            )),
            TurnOutcome::Cancelled(cancelled) => TurnState::Cancelled(CancelledTurnState::new(
                started_at,
                timestamp,
                timestamp,
                cancelled.cause().clone(),
            )),
            TurnOutcome::Failed(failed) => TurnState::Failed(FailedTurnState::new(
                started_at,
                timestamp,
                failed.failure().clone(),
            )),
            TurnOutcome::BudgetLimited(limited) => {
                TurnState::BudgetLimited(BudgetLimitedTurnState::new(
                    started_at,
                    timestamp,
                    *limited.limit(),
                    limited.rollover().clone(),
                ))
            }
        };
        if let Err(error) =
            item.apply(item.command(timestamp, TracePartAction::TransitionTurn { state }))
        {
            tracing::error!(%error, "failed to terminalize turn trace item");
        }
        item
    }

    pub fn complete_plan_item(&mut self, turn_id: &str, item_id: &str, content: String) {
        if content.trim().is_empty() {
            return;
        }
        let timestamp = unix_seconds();
        let mut item = if let Some(item) = self.latest_trace_part(item_id) {
            item
        } else {
            let item = TracePart::started_plan(
                turn_id.to_string(),
                item_id.to_string(),
                self.sequence,
                timestamp,
            );
            self.start_item(item.clone());
            item
        };
        if let Err(error) = item.apply(item.command(
            timestamp,
            TracePartAction::Complete(TracePartCompletion::Plan {
                content: Some(content),
            }),
        )) {
            tracing::error!(%error, "failed to complete plan trace item");
            return;
        }
        self.complete_item(item);
    }

    pub fn inference_item(&mut self, turn_id: &str, inference_id: &str, model: &str) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::running_inference(
            turn_id.to_string(),
            inference_id.to_string(),
            self.sequence,
            timestamp,
            inference_id.to_string(),
            model.to_string(),
        )
    }

    pub fn complete_inference_item(&mut self, mut item: TracePart, usage: TokenUsageSnapshot) {
        let timestamp = unix_seconds();
        if let Err(error) = item.apply(item.command(
            timestamp,
            TracePartAction::Complete(TracePartCompletion::Inference { usage }),
        )) {
            tracing::error!(%error, "failed to complete inference trace item");
            return;
        }
        self.complete_item(item);
    }

    pub fn tool_item(
        &mut self,
        turn_id: &str,
        tool_call_id: &str,
        name: String,
        arguments: String,
        call_id: Option<String>,
        provider_item_id: Option<String>,
    ) -> TracePart {
        let timestamp = unix_seconds();
        let invocation = TraceToolInvocation::new(tool_call_id.to_string(), name, arguments)
            .with_provider_identity(call_id, provider_item_id);
        TracePart::started_tool(
            turn_id.to_string(),
            tool_call_id.to_string(),
            self.sequence,
            timestamp,
            invocation,
        )
    }

    pub fn latest_trace_part(&self, item_id: &str) -> Option<TracePart> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item }
                    if item.item_id() == item_id =>
                {
                    Some(item.clone())
                }
                TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. }
                | TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. } => None,
            })
    }

    pub fn latest_tool_trace_part(
        &self,
        item_id: &str,
        call_id: Option<&str>,
        provider_item_id: Option<&str>,
    ) -> Option<TracePart> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item }
                    if item.kind() == TracePartKind::Tool
                        && tool_item_matches(item, item_id, call_id, provider_item_id) =>
                {
                    Some(item.clone())
                }
                TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. }
                | TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. } => None,
            })
    }

    pub fn agent_item(
        &mut self,
        turn_id: &str,
        item_id: String,
        agent: TraceAgentPart,
    ) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::agent(
            turn_id.to_string(),
            item_id,
            self.sequence,
            timestamp,
            agent,
        )
    }

    /// Broadcast an AgentEvent without recording a trace event.
    pub fn broadcast(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event);
    }

    fn push_event(&mut self, event: TraceEvent) {
        if let Some(durable_tx) = &self.durable_tx {
            let _ = durable_tx.send(event.clone());
        }
        self.events.push(event);
    }

    /// Get the raw event sender for passing to providers.
    pub fn sender(&self) -> &AgentEventSender {
        &self.event_tx
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Drain all recorded trace events. Called after turn completes.
    pub fn drain(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn advance_sequence(&mut self, next_sequence: u64) {
        self.sequence = self.sequence.max(next_sequence);
    }

    fn record_and_broadcast_item_start(&mut self, item: TracePart) {
        if self.disabled {
            self.broadcast(AgentEvent::TracePartStarted { item });
            return;
        }
        let timestamp = item.created_at();
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind: TraceEventKind::TracePartStarted { item: item.clone() },
        };
        self.sequence += 1;
        self.push_event(event);
        self.broadcast(AgentEvent::TracePartStarted { item });
    }

    fn has_assistant_text_content(&self, turn_id: &str) -> bool {
        self.events.iter().any(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item } => {
                item.turn_id() == turn_id
                    && matches!(
                        item.state(),
                        TracePartState::Text(text)
                            if text.channel() == TraceTextChannel::Final
                                && !text.content().trim().is_empty()
                    )
            }
            TraceEventKind::TracePartDelta { event } => {
                event.turn_id == turn_id
                    && matches!(
                        &event.delta,
                        TraceDelta::Text {
                            channel: TraceTextChannel::Final,
                            delta,
                        } if !delta.trim().is_empty()
                    )
            }
            TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
    }
}

fn tool_item_matches(
    item: &TracePart,
    item_id: &str,
    call_id: Option<&str>,
    provider_item_id: Option<&str>,
) -> bool {
    if item.item_id() == item_id {
        return true;
    }
    let Some(tool) = item.tool() else {
        return false;
    };
    let invocation = tool.invocation();
    call_id
        .filter(|value| !value.is_empty())
        .zip(invocation.call_id())
        .is_some_and(|(left, right)| left == right)
        || provider_item_id
            .filter(|value| !value.is_empty())
            .zip(invocation.provider_item_id())
            .is_some_and(|(left, right)| left == right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_recorder() -> (TraceRecorder, tokio::sync::broadcast::Receiver<AgentEvent>) {
        let (tx, rx) = tokio::sync::broadcast::channel(16);
        let recorder = TraceRecorder::new("sess-1".to_string(), tx, 0);
        (recorder, rx)
    }

    #[test]
    fn disabled_recorder_does_not_record_trace_events() {
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::disabled(tx);
        let item = recorder.running_turn_item("t1");
        recorder.start_item(item);
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn disabled_recorder_still_broadcasts() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::disabled(tx);
        let item = recorder.running_turn_item("t1");
        recorder.start_item(item);
        assert!(matches!(
            rx.try_recv(),
            Ok(AgentEvent::TracePartStarted { .. })
        ));
    }

    #[test]
    fn record_trace_only_increments_sequence() {
        let (mut recorder, _rx) = make_recorder();
        let item = recorder.running_turn_item("t1");
        recorder.start_item(item.clone());
        let completed = recorder.terminal_turn_item(
            "t1",
            &TurnOutcome::completed(pl_protocol::TurnCompletion::Normal),
        );
        recorder.complete_item(completed);

        let events = recorder.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 0);
        assert_eq!(events[1].sequence, 1);
        assert_eq!(recorder.current_sequence(), 2);
    }

    #[test]
    fn drain_clears_events() {
        let (mut recorder, _rx) = make_recorder();
        let item = recorder.running_turn_item("t1");
        recorder.start_item(item);

        let first = recorder.drain();
        assert_eq!(first.len(), 1);

        let second = recorder.drain();
        assert!(second.is_empty());
    }

    #[test]
    fn ensure_assistant_text_item_records_fallback_text_once() {
        let (mut recorder, _rx) = make_recorder();

        recorder.ensure_assistant_text_item("t1", "final answer");
        recorder.ensure_assistant_text_item("t1", "final answer");

        let events = recorder.drain();
        let text_items = events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Text =>
                {
                    let text = item.text().expect("assistant text part");
                    Some((item.item_id(), text.content(), Some(text.channel())))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            text_items,
            vec![
                (
                    "t1-assistant",
                    "final answer",
                    Some(TraceTextChannel::Final)
                ),
                (
                    "t1-assistant",
                    "final answer",
                    Some(TraceTextChannel::Final)
                ),
            ],
        );
    }
}
