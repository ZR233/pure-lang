use pl_protocol::TokenUsageSnapshot;
use pl_trace::{
    AgentEvent, AgentEventSender, TraceAgentPart, TraceDelta, TraceEvent, TraceEventKind,
    TraceInferencePart, TracePart, TracePartKind, TracePartStatus, TraceTextChannel, TraceToolPart,
};

/// In-memory trace recorder that captures structured lifecycle events during a turn.
///
/// Wraps an `AgentEventSender` and simultaneously:
/// - Passes `AgentEvent`s through to the broadcast channel (unchanged behavior)
/// - Appends item-first `TraceEvent`s to an in-memory log (flushed to DB after turn)
///
/// When tracing is not needed, use `TraceRecorder::disabled()` which still
/// forwards broadcasts but discards trace events.
pub struct TraceRecorder {
    session_id: String,
    event_tx: AgentEventSender,
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
        self.events.push(event);
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
        self.events.push(event);
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
        let timestamp = item.updated_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TracePartCompleted { item: item.clone() },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TracePartCompleted { item });
    }

    pub fn fail_item(&mut self, item: TracePart, error: String) {
        if self.disabled {
            self.broadcast(AgentEvent::TracePartFailed { item, error });
            return;
        }
        let sequence = self.sequence;
        let timestamp = item.updated_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TracePartFailed {
                item: item.clone(),
                error: error.clone(),
            },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TracePartFailed { item, error });
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
        let timestamp = unix_seconds();
        let mut item = TracePart::text(
            turn_id,
            format!("{turn_id}-user"),
            self.sequence,
            TraceTextChannel::User,
            content,
            TracePartStatus::Completed,
            timestamp,
        );
        item.attachments = attachments;
        self.record_and_broadcast_item_start(item.clone());
        self.complete_item(item);
    }

    pub fn ensure_assistant_text_item(&mut self, turn_id: &str, content: &str) {
        if content.trim().is_empty() || self.has_assistant_text_content(turn_id) {
            return;
        }
        let timestamp = unix_seconds();
        let item = TracePart::text(
            turn_id,
            format!("{turn_id}-assistant"),
            self.sequence,
            TraceTextChannel::Final,
            content.to_string(),
            TracePartStatus::Completed,
            timestamp,
        );
        self.record_and_broadcast_item_start(item.clone());
        self.complete_item(item);
    }

    pub fn turn_item(&mut self, turn_id: &str, status: TracePartStatus) -> TracePart {
        let timestamp = unix_seconds();
        TracePart {
            turn_id: turn_id.to_string(),
            item_id: format!("{turn_id}-turn"),
            started_sequence: self.sequence,
            kind: pl_trace::TracePartKind::Turn,
            status,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        }
    }

    pub fn complete_plan_item(&mut self, turn_id: &str, item_id: &str, content: String) {
        if content.trim().is_empty() {
            return;
        }
        let timestamp = unix_seconds();
        let mut item = if let Some(item) = self.latest_trace_part(item_id) {
            item
        } else {
            let item = TracePart {
                turn_id: turn_id.to_string(),
                item_id: item_id.to_string(),
                started_sequence: self.sequence,
                kind: TracePartKind::Plan,
                status: TracePartStatus::Started,
                created_at: timestamp,
                updated_at: timestamp,
                text_channel: None,
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.start_item(item.clone());
            item
        };
        item.kind = TracePartKind::Plan;
        item.status = TracePartStatus::Completed;
        item.content = content;
        item.updated_at = timestamp;
        self.complete_item(item);
    }

    pub fn inference_item(&mut self, turn_id: &str, inference_id: &str, model: &str) -> TracePart {
        let timestamp = unix_seconds();
        TracePart {
            turn_id: turn_id.to_string(),
            item_id: inference_id.to_string(),
            started_sequence: self.sequence,
            kind: pl_trace::TracePartKind::Inference,
            status: TracePartStatus::Running,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: Some(TraceInferencePart {
                inference_id: inference_id.to_string(),
                model: model.to_string(),
            }),
            usage: None,
        }
    }

    pub fn complete_inference_item(&mut self, mut item: TracePart, usage: TokenUsageSnapshot) {
        item.status = TracePartStatus::Completed;
        item.updated_at = unix_seconds();
        item.usage = Some(usage);
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
        TracePart {
            turn_id: turn_id.to_string(),
            item_id: tool_call_id.to_string(),
            started_sequence: self.sequence,
            kind: pl_trace::TracePartKind::Tool,
            status: TracePartStatus::Started,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TraceToolPart {
                tool_call_id: tool_call_id.to_string(),
                call_id,
                provider_item_id,
                name,
                arguments,
                result: None,
                exit_code: None,
                timed_out: false,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        }
    }

    pub fn latest_trace_part(&self, item_id: &str) -> Option<TracePart> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item, .. }
                    if item.item_id == item_id =>
                {
                    Some(item.clone())
                }
                TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
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
        status: TracePartStatus,
    ) -> TracePart {
        let timestamp = unix_seconds();
        TracePart {
            turn_id: turn_id.to_string(),
            item_id,
            started_sequence: self.sequence,
            kind: pl_trace::TracePartKind::Agent,
            status,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: Some(agent),
            inference: None,
            usage: None,
        }
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
        let timestamp = item.created_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind: TraceEventKind::TracePartStarted { item: item.clone() },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TracePartStarted { item });
    }

    fn has_assistant_text_content(&self, turn_id: &str) -> bool {
        self.events.iter().any(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. } => {
                item.turn_id == turn_id
                    && item.kind == TracePartKind::Text
                    && item.text_channel == Some(TraceTextChannel::Final)
                    && !item.content.trim().is_empty()
            }
            TraceEventKind::TracePartDelta { event } => {
                event.turn_id == turn_id
                    && event.kind == TracePartKind::Text
                    && matches!(
                        &event.delta,
                        TraceDelta::Text {
                            text_channel: TraceTextChannel::Final,
                            delta,
                        } if !delta.trim().is_empty()
                    )
            }
            TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
        let item = recorder.turn_item("t1", TracePartStatus::Started);
        recorder.start_item(item);
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn disabled_recorder_still_broadcasts() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::disabled(tx);
        let item = recorder.turn_item("t1", TracePartStatus::Started);
        recorder.start_item(item);
        assert!(matches!(
            rx.try_recv(),
            Ok(AgentEvent::TracePartStarted { .. })
        ));
    }

    #[test]
    fn record_trace_only_increments_sequence() {
        let (mut recorder, _rx) = make_recorder();
        let item = recorder.turn_item("t1", TracePartStatus::Started);
        recorder.start_item(item.clone());
        let mut completed = item;
        completed.status = TracePartStatus::Completed;
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
        let item = recorder.turn_item("t1", TracePartStatus::Started);
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
                    if item.kind == TracePartKind::Text =>
                {
                    Some((
                        item.item_id.as_str(),
                        item.content.as_str(),
                        item.text_channel,
                    ))
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
