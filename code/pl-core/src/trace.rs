use pl_protocol::{AgentEvent, AgentEventSender, TraceEvent, TraceEventKind};

/// In-memory trace recorder that captures structured lifecycle events during a turn.
///
/// Wraps an `AgentEventSender` and simultaneously:
/// - Passes `AgentEvent`s through to the broadcast channel (unchanged behavior)
/// - Appends `TraceEvent`s to an in-memory log (flushed to DB after turn)
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

    /// Create a no-op recorder that forwards broadcasts but discards traces.
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

    /// Broadcast an AgentEvent without recording a trace (for streaming deltas).
    pub fn broadcast(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Get the raw event sender for passing to providers.
    pub fn sender(&self) -> &AgentEventSender {
        &self.event_tx
    }

    /// Drain all recorded trace events. Called after turn completes.
    pub fn drain(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence
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
        recorder.record_trace_only(TraceEventKind::TurnStarted {
            turn_id: "t1".to_string(),
        });
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn disabled_recorder_still_broadcasts() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let recorder = TraceRecorder::disabled(tx);
        recorder.broadcast(AgentEvent::TurnStarted);
        assert!(matches!(rx.try_recv(), Ok(AgentEvent::TurnStarted)));
    }

    #[test]
    fn record_trace_only_increments_sequence() {
        let (mut recorder, _rx) = make_recorder();
        recorder.record_trace_only(TraceEventKind::TurnStarted {
            turn_id: "t1".to_string(),
        });
        recorder.record_trace_only(TraceEventKind::TurnCompleted {
            turn_id: "t1".to_string(),
            content: "done".to_string(),
            model: "gpt-4".to_string(),
            usage: pl_protocol::TokenUsageSnapshot::default(),
        });

        let events = recorder.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 0);
        assert_eq!(events[1].sequence, 1);
        assert_eq!(recorder.current_sequence(), 2);
    }

    #[test]
    fn drain_clears_events() {
        let (mut recorder, _rx) = make_recorder();
        recorder.record_trace_only(TraceEventKind::TurnStarted {
            turn_id: "t1".to_string(),
        });

        let first = recorder.drain();
        assert_eq!(first.len(), 1);

        let second = recorder.drain();
        assert!(second.is_empty());
    }
}
