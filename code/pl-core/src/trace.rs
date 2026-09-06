use crate::time::unix_seconds;
use pl_protocol::{
    BudgetLimitedTurnState, CancelledTurnState, CompletedTurnState, FailedTurnState,
    RunningTurnState, TokenUsageSnapshot, TurnOutcome, TurnPhase, TurnState,
};
use pl_trace::{
    AgentEvent, AgentEventSender, TraceAgentPart, TraceEvent, TraceEventDraft, TraceEventKind,
    TraceEventSink, TraceEventSinkError, TracePart, TracePartAction, TracePartCompletion,
    TracePartKind, TracePartState, TraceTextChannel, TraceToolFailureKind, TraceToolInvocation,
};
use std::sync::{Arc, Mutex};

/// In-memory trace recorder that captures structured lifecycle events during a turn.
///
/// Wraps an `AgentEventSender` and simultaneously:
/// - Passes `AgentEvent`s through to the broadcast channel (unchanged behavior)
/// - Appends item-first `TraceEvent`s to an in-memory turn capture
/// - Agent runtime 模式下同步送入 owner channel，由 actor 先提交内存再冷持久化
///
/// When tracing is not needed, use `TraceRecorder::disabled()` which still
/// forwards broadcasts but discards trace events.
pub struct TraceRecorder {
    session_id: String,
    event_tx: AgentEventSender,
    sink: Option<Arc<RecorderTraceEventSink>>,
    publication_error: Option<TraceEventSinkError>,
}

#[derive(Debug)]
struct RecorderTraceEventSink {
    session_id: String,
    durable_tx: Option<tokio::sync::mpsc::UnboundedSender<TraceEvent>>,
    discard_events: bool,
    state: Mutex<RecorderTraceState>,
}

#[derive(Debug)]
struct RecorderTraceState {
    events: Vec<TraceEvent>,
    next_sequence: u64,
    ledger: pl_trace::TraceEventLedger,
}

impl RecorderTraceEventSink {
    fn new(
        session_id: String,
        starting_sequence: u64,
        durable_tx: Option<tokio::sync::mpsc::UnboundedSender<TraceEvent>>,
    ) -> Self {
        Self {
            session_id,
            durable_tx,
            discard_events: false,
            state: Mutex::new(RecorderTraceState {
                events: Vec::new(),
                next_sequence: starting_sequence,
                ledger: pl_trace::TraceEventLedger::default(),
            }),
        }
    }

    fn discarding() -> Self {
        Self {
            session_id: String::new(),
            durable_tx: None,
            discard_events: true,
            state: Mutex::new(RecorderTraceState {
                events: Vec::new(),
                next_sequence: 0,
                ledger: pl_trace::TraceEventLedger::default(),
            }),
        }
    }

    fn drain(&self) -> Vec<TraceEvent> {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }
}

impl TraceEventSink for RecorderTraceEventSink {
    fn emit(&self, draft: TraceEventDraft) -> Result<TraceEvent, TraceEventSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| TraceEventSinkError::new("trace sink state is poisoned"))?;
        let sequence = state.next_sequence;
        let timestamp = draft.timestamp;
        let prepared = state.ledger.prepare(sequence, draft)?;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: prepared.kind().clone(),
        };
        if let Some(durable_tx) = &self.durable_tx {
            durable_tx.send(event.clone()).map_err(|_| {
                TraceEventSinkError::new("canonical trace owner is no longer available")
            })?;
        }
        state.ledger.accept(prepared);
        state.next_sequence = state.next_sequence.saturating_add(1);
        if !self.discard_events {
            state.events.push(event.clone());
        }
        Ok(event)
    }

    fn next_sequence(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.next_sequence)
            .unwrap_or_default()
    }
}

impl TraceRecorder {
    /// Create a recorder that captures trace events.
    pub fn new(session_id: String, event_tx: AgentEventSender, starting_sequence: u64) -> Self {
        let sink = Arc::new(RecorderTraceEventSink::new(
            session_id.clone(),
            starting_sequence,
            None,
        ));
        Self {
            session_id,
            event_tx,
            sink: Some(sink),
            publication_error: None,
        }
    }

    /// 创建同时把 trace 送入 agent runtime durable channel 的 recorder。
    pub(crate) fn streaming(
        session_id: String,
        event_tx: AgentEventSender,
        starting_sequence: u64,
        durable_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
    ) -> Self {
        let sink = Arc::new(RecorderTraceEventSink::new(
            session_id.clone(),
            starting_sequence,
            Some(durable_tx),
        ));
        Self {
            session_id,
            event_tx,
            sink: Some(sink),
            publication_error: None,
        }
    }

    /// Create a no-op recorder that forwards broadcasts but discards trace events.
    pub fn disabled(event_tx: AgentEventSender) -> Self {
        Self {
            session_id: String::new(),
            event_tx,
            sink: Some(Arc::new(RecorderTraceEventSink::discarding())),
            publication_error: None,
        }
    }

    /// Record a trace event only (no corresponding AgentEvent broadcast).
    pub fn record_trace_only(&mut self, operation: pl_trace::TraceOperation) {
        self.emit_trace(TraceEventDraft::new(unix_seconds(), operation));
    }

    /// Starts an item using only its identity, source and initial state.
    pub fn start_item(&mut self, item: TracePart) {
        self.record_and_broadcast_item_start(item);
    }

    /// Applies against the publication owner's latest state, including concurrent tool output.
    pub fn apply_item(&mut self, item: &TracePart, action: TracePartAction) -> Option<TracePart> {
        let event = self.emit_trace(TraceEventDraft::apply(
            unix_seconds(),
            item.turn_id().to_owned(),
            item.item_id().to_owned(),
            action,
        ))?;
        let part = match event.kind {
            TraceEventKind::TracePartStarted { item } => {
                self.broadcast(AgentEvent::TracePartStarted { item: item.clone() });
                item
            }
            TraceEventKind::TracePartCompleted { item } => {
                self.broadcast(AgentEvent::TracePartCompleted { item: item.clone() });
                item
            }
            TraceEventKind::TracePartFailed { item } => {
                self.broadcast(AgentEvent::TracePartFailed { item: item.clone() });
                item
            }
            TraceEventKind::TracePartDelta { event } => {
                self.broadcast(AgentEvent::TracePartDelta { event });
                return None;
            }
            TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => return None,
        };
        Some(part)
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
            0,
            TraceTextChannel::User,
            content,
            attachments,
            timestamp,
        );
        self.record_and_broadcast_item_start(item);
    }

    pub(crate) fn final_text_item(&mut self, turn_id: &str, content: String) {
        let item = TracePart::streaming_text(
            turn_id,
            format!("{turn_id}-runtime-final"),
            0,
            TraceTextChannel::Final,
            unix_seconds(),
        );
        self.record_and_broadcast_item_start(item.clone());
        self.apply_item(
            &item,
            TracePartAction::Complete(TracePartCompletion::Text {
                authoritative_content: Some(content),
            }),
        );
    }

    pub fn running_turn_item(&mut self, turn_id: &str) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::turn(
            turn_id.to_string(),
            format!("{turn_id}-turn"),
            0,
            timestamp,
            TurnState::Running(RunningTurnState::new(timestamp, TurnPhase::Preparing)),
        )
    }

    pub fn finish_turn_item(&mut self, turn_id: &str, outcome: &TurnOutcome) {
        let timestamp = unix_seconds();
        let item = self
            .latest_trace_part(&format!("{turn_id}-turn"))
            .unwrap_or_else(|| self.running_turn_item(turn_id));
        let started_at = match item.state() {
            TracePartState::Turn(turn) => turn.state().started_at(),
            TracePartState::Text(_)
            | TracePartState::Thinking(_)
            | TracePartState::Tool(_)
            | TracePartState::Agent(_)
            | TracePartState::Inference(_) => None,
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
        self.apply_item(&item, TracePartAction::TransitionTurn { state });
    }

    /// Cancel every non-terminal item owned by `turn_id`, except the turn item itself.
    pub(crate) fn cancel_open_items(&mut self, turn_id: &str, reason: &str) -> Vec<String> {
        self.terminalize_open_items(
            turn_id,
            TracePartAction::Cancel {
                reason: reason.to_string(),
            },
        )
    }

    /// Fail every non-terminal item owned by `turn_id`, except the turn item itself.
    pub(crate) fn fail_open_items(&mut self, turn_id: &str, error: &str) -> Vec<String> {
        self.terminalize_open_items(
            turn_id,
            TracePartAction::Fail {
                error: error.to_string(),
                tool_kind: TraceToolFailureKind::Execution,
            },
        )
    }

    pub fn inference_item(&mut self, turn_id: &str, inference_id: &str, model: &str) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::running_inference(
            turn_id.to_string(),
            inference_id.to_string(),
            0,
            timestamp,
            inference_id.to_string(),
            model.to_string(),
        )
    }

    pub fn complete_inference_item(&mut self, item: TracePart, usage: TokenUsageSnapshot) {
        self.apply_item(
            &item,
            TracePartAction::Complete(TracePartCompletion::Inference { usage }),
        );
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
            0,
            timestamp,
            invocation,
        )
    }

    pub fn latest_trace_part(&self, item_id: &str) -> Option<TracePart> {
        self.sink.as_ref()?.state.lock().ok()?.ledger.item(item_id)
    }

    pub fn latest_tool_trace_part(
        &self,
        item_id: &str,
        call_id: Option<&str>,
        provider_item_id: Option<&str>,
    ) -> Option<TracePart> {
        self.latest_trace_parts()
            .into_iter()
            .filter(|item| {
                item.kind() == TracePartKind::Tool
                    && tool_item_matches(item, item_id, call_id, provider_item_id)
            })
            .max_by_key(TracePart::started_sequence)
    }

    pub fn agent_item(
        &mut self,
        turn_id: &str,
        item_id: String,
        agent: TraceAgentPart,
    ) -> TracePart {
        let timestamp = unix_seconds();
        TracePart::agent(turn_id.to_string(), item_id, 0, timestamp, agent)
    }

    /// Broadcast an AgentEvent without recording a trace event.
    pub fn broadcast(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event);
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
        self.sink
            .as_ref()
            .map_or_else(Vec::new, |sink| sink.drain())
    }

    pub fn current_sequence(&self) -> u64 {
        self.sink.as_ref().map_or(0, |sink| sink.next_sequence())
    }

    /// Returns the shared canonical trace sink for model and tool producers.
    pub fn trace_sink(&self) -> Option<Arc<dyn TraceEventSink>> {
        self.sink
            .as_ref()
            .map(|sink| Arc::clone(sink) as Arc<dyn TraceEventSink>)
    }

    /// Returns the first typed failure observed while publishing recorder-owned events.
    pub(crate) fn publication_error(&self) -> Option<&TraceEventSinkError> {
        self.publication_error.as_ref()
    }

    fn record_and_broadcast_item_start(&mut self, item: TracePart) {
        if let Some(event) = self.emit_trace(TraceEventDraft::start(
            item.created_at(),
            item.turn_id().to_owned(),
            item.item_id().to_owned(),
            item.source(),
            item.state().clone(),
        )) && let TraceEventKind::TracePartStarted { item } = event.kind
        {
            self.broadcast(AgentEvent::TracePartStarted { item });
        }
    }

    fn terminalize_open_items(&mut self, turn_id: &str, action: TracePartAction) -> Vec<String> {
        let mut terminalized = Vec::new();
        for item in self.latest_trace_parts() {
            if item.turn_id() != turn_id || item.kind() == TracePartKind::Turn || item.is_terminal()
            {
                continue;
            }
            let item_id = item.item_id().to_string();
            if self.apply_item(&item, action.clone()).is_none() {
                continue;
            }
            terminalized.push(item_id);
        }
        terminalized
    }

    fn latest_trace_parts(&self) -> Vec<TracePart> {
        self.sink
            .as_ref()
            .and_then(|sink| sink.state.lock().ok().map(|state| state.ledger.items()))
            .unwrap_or_default()
    }

    fn emit_trace(&mut self, draft: TraceEventDraft) -> Option<TraceEvent> {
        let sink = self.sink.as_ref()?;
        match sink.emit(draft) {
            Ok(event) => Some(event),
            Err(error) => {
                self.publication_error.get_or_insert(error);
                None
            }
        }
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
        recorder.finish_turn_item(
            "t1",
            &TurnOutcome::completed(pl_protocol::TurnCompletion::Normal),
        );

        let events = recorder.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 0);
        assert_eq!(events[1].sequence, 1);
        assert_eq!(recorder.current_sequence(), 2);
    }

    #[test]
    fn closed_turn_channel_rejects_publication_without_advancing_memory() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = RecorderTraceEventSink::new("thread".into(), 12, Some(sender));
        drop(receiver);
        let item = TracePart::streaming_text("old-turn", "item", 0, TraceTextChannel::Final, 1);
        assert!(
            sink.emit(TraceEventDraft::start(
                1,
                item.turn_id().into(),
                item.item_id().into(),
                item.source(),
                item.state().clone()
            ))
            .is_err()
        );
        assert_eq!(sink.next_sequence(), 12);
        assert!(sink.drain().is_empty());
        assert!(sink.state.lock().unwrap().ledger.item("item").is_none());
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
}
