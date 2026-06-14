use pl_protocol::{
    AgentEvent, AgentEventSender, TimelineAgentItem, TimelineDelta, TimelineInferenceItem,
    TimelineItem, TimelineItemKind, TimelineItemStatus, TimelineTextChannel, TimelineToolItem,
    TokenUsageSnapshot, TraceEvent, TraceEventKind,
};

/// In-memory timeline recorder that captures structured lifecycle events during a turn.
///
/// Wraps an `AgentEventSender` and simultaneously:
/// - Passes `AgentEvent`s through to the broadcast channel (unchanged behavior)
/// - Appends item-first `TraceEvent`s to an in-memory log (flushed to DB after turn)
///
/// When tracing is not needed, use `TraceRecorder::disabled()` which still
/// forwards broadcasts but discards timeline events.
pub struct TraceRecorder {
    session_id: String,
    event_tx: AgentEventSender,
    events: Vec<TraceEvent>,
    sequence: u64,
    disabled: bool,
}

impl TraceRecorder {
    /// Create a recorder that captures timeline events.
    pub fn new(session_id: String, event_tx: AgentEventSender, starting_sequence: u64) -> Self {
        Self {
            session_id,
            event_tx,
            events: Vec::new(),
            sequence: starting_sequence,
            disabled: false,
        }
    }

    /// Create a no-op recorder that forwards broadcasts but discards timeline events.
    pub fn disabled(event_tx: AgentEventSender) -> Self {
        Self {
            session_id: String::new(),
            event_tx,
            events: Vec::new(),
            sequence: 0,
            disabled: true,
        }
    }

    /// Record a timeline event only (no corresponding AgentEvent broadcast).
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

    pub fn start_item(&mut self, item: TimelineItem) {
        self.record_and_broadcast_item_start(item);
    }

    pub fn complete_item(&mut self, item: TimelineItem) {
        if self.disabled {
            self.broadcast(AgentEvent::TimelineItemCompleted { sequence: 0, item });
            return;
        }
        let sequence = self.sequence;
        let timestamp = item.updated_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TimelineItemCompleted { item: item.clone() },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TimelineItemCompleted { sequence, item });
    }

    pub fn fail_item(&mut self, item: TimelineItem, error: String) {
        if self.disabled {
            self.broadcast(AgentEvent::TimelineItemFailed {
                sequence: 0,
                item,
                error,
            });
            return;
        }
        let sequence = self.sequence;
        let timestamp = item.updated_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: TraceEventKind::TimelineItemFailed {
                item: item.clone(),
                error: error.clone(),
            },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TimelineItemFailed {
            sequence,
            item,
            error,
        });
    }

    pub fn user_text_item(&mut self, turn_id: &str, content: String) {
        self.user_text_item_with_attachments(turn_id, content, Vec::new());
    }

    pub fn user_text_item_with_attachments(
        &mut self,
        turn_id: &str,
        content: String,
        attachments: Vec<pl_protocol::TimelineAttachment>,
    ) {
        let timestamp = unix_seconds();
        let mut item = TimelineItem::text(
            turn_id,
            format!("{turn_id}-user"),
            self.sequence,
            TimelineTextChannel::User,
            content,
            TimelineItemStatus::Completed,
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
        let item = TimelineItem::text(
            turn_id,
            format!("{turn_id}-assistant"),
            self.sequence,
            TimelineTextChannel::Final,
            content.to_string(),
            TimelineItemStatus::Completed,
            timestamp,
        );
        self.record_and_broadcast_item_start(item.clone());
        self.complete_item(item);
    }

    pub fn turn_item(&mut self, turn_id: &str, status: TimelineItemStatus) -> TimelineItem {
        let timestamp = unix_seconds();
        TimelineItem {
            turn_id: turn_id.to_string(),
            item_id: format!("{turn_id}-turn"),
            sequence: self.sequence,
            kind: pl_protocol::TimelineItemKind::Turn,
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
        let mut item = if let Some(item) = self.latest_timeline_item(item_id) {
            item
        } else {
            let item = TimelineItem {
                turn_id: turn_id.to_string(),
                item_id: item_id.to_string(),
                sequence: self.sequence,
                kind: TimelineItemKind::Plan,
                status: TimelineItemStatus::Started,
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
        item.kind = TimelineItemKind::Plan;
        item.status = TimelineItemStatus::Completed;
        item.content = content;
        item.updated_at = timestamp;
        self.complete_item(item);
    }

    pub fn inference_item(
        &mut self,
        turn_id: &str,
        inference_id: &str,
        model: &str,
    ) -> TimelineItem {
        let timestamp = unix_seconds();
        TimelineItem {
            turn_id: turn_id.to_string(),
            item_id: inference_id.to_string(),
            sequence: self.sequence,
            kind: pl_protocol::TimelineItemKind::Inference,
            status: TimelineItemStatus::Running,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: Some(TimelineInferenceItem {
                inference_id: inference_id.to_string(),
                model: model.to_string(),
            }),
            usage: None,
        }
    }

    pub fn complete_inference_item(&mut self, mut item: TimelineItem, usage: TokenUsageSnapshot) {
        item.status = TimelineItemStatus::Completed;
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
    ) -> TimelineItem {
        let timestamp = unix_seconds();
        TimelineItem {
            turn_id: turn_id.to_string(),
            item_id: tool_call_id.to_string(),
            sequence: self.sequence,
            kind: pl_protocol::TimelineItemKind::Tool,
            status: TimelineItemStatus::Started,
            created_at: timestamp,
            updated_at: timestamp,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TimelineToolItem {
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

    pub fn latest_timeline_item(&self, item_id: &str) -> Option<TimelineItem> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemStarted { item }
                | TraceEventKind::TimelineItemCompleted { item }
                | TraceEventKind::TimelineItemFailed { item, .. }
                    if item.item_id == item_id =>
                {
                    Some(item.clone())
                }
                TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. }
                | TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::TimelineItemFailed { .. } => None,
            })
    }

    pub fn agent_item(
        &mut self,
        turn_id: &str,
        item_id: String,
        agent: TimelineAgentItem,
        status: TimelineItemStatus,
    ) -> TimelineItem {
        let timestamp = unix_seconds();
        TimelineItem {
            turn_id: turn_id.to_string(),
            item_id,
            sequence: self.sequence,
            kind: pl_protocol::TimelineItemKind::Agent,
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

    /// Broadcast an AgentEvent without recording a timeline event.
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

    /// Drain all recorded timeline events. Called after turn completes.
    pub fn drain(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn advance_sequence(&mut self, next_sequence: u64) {
        self.sequence = self.sequence.max(next_sequence);
    }

    fn record_and_broadcast_item_start(&mut self, item: TimelineItem) {
        if self.disabled {
            self.broadcast(AgentEvent::TimelineItemStarted { item });
            return;
        }
        let timestamp = item.created_at;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind: TraceEventKind::TimelineItemStarted { item: item.clone() },
        };
        self.sequence += 1;
        self.events.push(event);
        self.broadcast(AgentEvent::TimelineItemStarted { item });
    }

    fn has_assistant_text_content(&self, turn_id: &str) -> bool {
        self.events.iter().any(|event| match &event.kind {
            TraceEventKind::TimelineItemStarted { item }
            | TraceEventKind::TimelineItemCompleted { item }
            | TraceEventKind::TimelineItemFailed { item, .. } => {
                item.turn_id == turn_id
                    && item.kind == TimelineItemKind::Text
                    && item.text_channel == Some(TimelineTextChannel::Final)
                    && !item.content.trim().is_empty()
            }
            TraceEventKind::TimelineItemDelta { event } => {
                event.turn_id == turn_id
                    && event.kind == TimelineItemKind::Text
                    && matches!(
                        &event.delta,
                        TimelineDelta::Text {
                            text_channel: TimelineTextChannel::Final,
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
        let item = recorder.turn_item("t1", TimelineItemStatus::Started);
        recorder.start_item(item);
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn disabled_recorder_still_broadcasts() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::disabled(tx);
        let item = recorder.turn_item("t1", TimelineItemStatus::Started);
        recorder.start_item(item);
        assert!(matches!(
            rx.try_recv(),
            Ok(AgentEvent::TimelineItemStarted { .. })
        ));
    }

    #[test]
    fn record_trace_only_increments_sequence() {
        let (mut recorder, _rx) = make_recorder();
        let item = recorder.turn_item("t1", TimelineItemStatus::Started);
        recorder.start_item(item.clone());
        let mut completed = item;
        completed.status = TimelineItemStatus::Completed;
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
        let item = recorder.turn_item("t1", TimelineItemStatus::Started);
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
                TraceEventKind::TimelineItemStarted { item }
                | TraceEventKind::TimelineItemCompleted { item }
                    if item.kind == TimelineItemKind::Text =>
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
                    Some(TimelineTextChannel::Final)
                ),
                (
                    "t1-assistant",
                    "final answer",
                    Some(TimelineTextChannel::Final)
                ),
            ],
        );
    }
}
