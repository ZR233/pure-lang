//! Canonical trace event publication boundary.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use crate::{TraceEvent, TraceEventKind, TracePart, TracePartAction};

/// A trace event before its owner assigns the canonical session and sequence.
#[derive(Debug, Clone)]
pub struct TraceEventDraft {
    pub timestamp: i64,
    pub kind: TraceEventKind,
}

impl TraceEventDraft {
    pub fn new(timestamp: i64, kind: TraceEventKind) -> Self {
        Self { timestamp, kind }
    }
}

/// Failure to publish a trace event to its canonical owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEventSinkError {
    message: String,
}

impl TraceEventSinkError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TraceEventSinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TraceEventSinkError {}

/// Single publication boundary for canonical trace events.
///
/// Implementations assign session and sequence atomically and make the event
/// visible to the owning runtime before returning it to the producer.
pub trait TraceEventSink: fmt::Debug + Send + Sync {
    /// Publishes one draft and returns the canonical event.
    fn emit(&self, draft: TraceEventDraft) -> Result<TraceEvent, TraceEventSinkError>;

    /// Returns the next sequence that will be assigned by this sink.
    fn next_sequence(&self) -> u64;
}

/// In-memory canonical sink for direct runtime use and deterministic tests.
#[derive(Debug)]
pub struct InMemoryTraceEventSink {
    session_id: String,
    state: Mutex<InMemoryTraceState>,
}

#[derive(Debug)]
struct InMemoryTraceState {
    events: Vec<TraceEvent>,
    next_sequence: u64,
    ledger: TraceEventLedger,
}

/// Synchronous lifecycle ledger shared by canonical sink implementations.
#[derive(Debug, Default)]
pub struct TraceEventLedger {
    parts: BTreeMap<String, TracePart>,
}

impl TraceEventLedger {
    /// Validates one draft against the latest canonical parts and advances the ledger.
    pub fn validate(
        &mut self,
        sequence: u64,
        kind: &TraceEventKind,
    ) -> Result<(), TraceEventSinkError> {
        match kind {
            TraceEventKind::TracePartStarted { item } => {
                self.accept_snapshot(sequence, item, false)
            }
            TraceEventKind::TracePartDelta { event } => {
                let item = self.parts.get_mut(&event.item_id).ok_or_else(|| {
                    TraceEventSinkError::new(format!(
                        "trace delta targets missing item {}",
                        event.item_id
                    ))
                })?;
                validate_identity(item, &event.turn_id, &event.item_id, event.started_sequence)?;
                if item.is_terminal() {
                    return Err(TraceEventSinkError::new(format!(
                        "trace delta targets terminal item {}",
                        event.item_id
                    )));
                }
                let expected = item.revision().saturating_add(1);
                if event.revision != expected {
                    return Err(TraceEventSinkError::new(format!(
                        "trace item {} revision conflict: expected {}, got {}",
                        event.item_id, expected, event.revision
                    )));
                }
                item.apply(item.command(
                    event.updated_at,
                    TracePartAction::Append(event.delta.clone()),
                ))
                .map_err(|error| {
                    TraceEventSinkError::new(format!(
                        "trace delta is invalid for item {}: {error}",
                        event.item_id
                    ))
                })?;
                Ok(())
            }
            TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item } => {
                self.accept_snapshot(sequence, item, true)
            }
            TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => Ok(()),
        }
    }

    fn accept_snapshot(
        &mut self,
        sequence: u64,
        item: &TracePart,
        terminal_event: bool,
    ) -> Result<(), TraceEventSinkError> {
        if terminal_event && !item.is_terminal() {
            return Err(TraceEventSinkError::new(format!(
                "terminal trace event carries open item {}",
                item.item_id()
            )));
        }
        let Some(current) = self.parts.get(item.item_id()) else {
            if item.started_sequence() != sequence {
                return Err(TraceEventSinkError::new(format!(
                    "trace item {} started sequence conflict: expected {sequence}, got {}",
                    item.item_id(),
                    item.started_sequence()
                )));
            }
            if terminal_event {
                return Err(TraceEventSinkError::new(format!(
                    "terminal trace event targets missing item {}",
                    item.item_id()
                )));
            }
            self.parts.insert(item.item_id().to_string(), item.clone());
            return Ok(());
        };
        validate_identity(
            current,
            item.turn_id(),
            item.item_id(),
            item.started_sequence(),
        )?;
        if current == item {
            return Ok(());
        }
        if current.is_terminal() {
            return Err(TraceEventSinkError::new(format!(
                "trace item {} changed after terminal",
                item.item_id()
            )));
        }
        if item.revision() < current.revision()
            || terminal_event && item.revision() == current.revision()
        {
            return Err(TraceEventSinkError::new(format!(
                "trace item {} revision regressed or failed to advance: current {}, got {}",
                item.item_id(),
                current.revision(),
                item.revision()
            )));
        }
        self.parts.insert(item.item_id().to_string(), item.clone());
        Ok(())
    }
}

fn validate_identity(
    current: &TracePart,
    turn_id: &str,
    item_id: &str,
    started_sequence: u64,
) -> Result<(), TraceEventSinkError> {
    if current.turn_id() != turn_id
        || current.item_id() != item_id
        || current.started_sequence() != started_sequence
    {
        return Err(TraceEventSinkError::new(format!(
            "trace item {item_id} immutable identity changed"
        )));
    }
    Ok(())
}

impl InMemoryTraceEventSink {
    pub fn new(session_id: impl Into<String>, starting_sequence: u64) -> Self {
        Self {
            session_id: session_id.into(),
            state: Mutex::new(InMemoryTraceState {
                events: Vec::new(),
                next_sequence: starting_sequence,
                ledger: TraceEventLedger::default(),
            }),
        }
    }

    pub fn events(&self) -> Vec<TraceEvent> {
        self.state
            .lock()
            .map(|state| state.events.clone())
            .unwrap_or_default()
    }

    pub fn drain(&self) -> Vec<TraceEvent> {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }
}

impl TraceEventSink for InMemoryTraceEventSink {
    fn emit(&self, draft: TraceEventDraft) -> Result<TraceEvent, TraceEventSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| TraceEventSinkError::new("trace sink state is poisoned"))?;
        let sequence = state.next_sequence;
        state.ledger.validate(sequence, &draft.kind)?;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp: draft.timestamp,
            kind: draft.kind,
        };
        state.next_sequence = state.next_sequence.saturating_add(1);
        state.events.push(event.clone());
        Ok(event)
    }

    fn next_sequence(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.next_sequence)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TraceDelta, TracePartDeltaEvent, TraceTextChannel};

    #[test]
    fn sink_rejects_revision_gaps_and_late_deltas() {
        let sink = InMemoryTraceEventSink::new("session", 4);
        let item = TracePart::streaming_text("turn", "text", 4, TraceTextChannel::Final, 1);
        sink.emit(TraceEventDraft::new(
            1,
            TraceEventKind::TracePartStarted { item: item.clone() },
        ))
        .expect("start item");
        let gap = TracePartDeltaEvent {
            turn_id: "turn".to_string(),
            item_id: "text".to_string(),
            started_sequence: 4,
            revision: 2,
            created_at: 1,
            updated_at: 2,
            delta: TraceDelta::Text {
                channel: TraceTextChannel::Final,
                delta: "gap".to_string(),
            },
        };
        assert!(
            sink.emit(TraceEventDraft::new(
                2,
                TraceEventKind::TracePartDelta { event: gap },
            ))
            .is_err()
        );
        let delta = TracePartDeltaEvent {
            turn_id: "turn".to_string(),
            item_id: "text".to_string(),
            started_sequence: 4,
            revision: 1,
            created_at: 1,
            updated_at: 2,
            delta: TraceDelta::Text {
                channel: TraceTextChannel::Final,
                delta: "ok".to_string(),
            },
        };
        sink.emit(TraceEventDraft::new(
            2,
            TraceEventKind::TracePartDelta {
                event: delta.clone(),
            },
        ))
        .expect("continuous delta");
        let mut terminal = item;
        terminal
            .apply(terminal.command(2, TracePartAction::Append(delta.delta.clone())))
            .expect("fold delta");
        terminal
            .apply(terminal.command(
                3,
                TracePartAction::Complete(crate::TracePartCompletion::Text {
                    authoritative_content: Some("ok".to_string()),
                }),
            ))
            .expect("terminal item");
        sink.emit(TraceEventDraft::new(
            3,
            TraceEventKind::TracePartCompleted { item: terminal },
        ))
        .expect("terminal event");
        assert!(
            sink.emit(TraceEventDraft::new(
                4,
                TraceEventKind::TracePartDelta { event: delta },
            ))
            .is_err()
        );
    }
}
