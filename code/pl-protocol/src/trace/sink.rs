//! Canonical trace event publication boundary.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use crate::trace::{TraceEvent, TraceEventKind, TracePart, TracePartAction};

/// A producer operation without a preallocated sequence or revision.
#[derive(Debug, Clone)]
pub enum TraceOperation {
    Start {
        turn_id: String,
        item_id: String,
        source: crate::trace::TracePartSource,
        state: crate::trace::TracePartState,
    },
    Apply {
        turn_id: String,
        item_id: String,
        action: TracePartAction,
    },
    InteractionChanged {
        event: crate::InteractionChangedEvent,
    },
    SkillActivated {
        activation: crate::SkillActivation,
    },
    EnabledToolsRecorded {
        event: crate::trace::EnabledToolsEvent,
    },
}

/// One operation submitted to the canonical in-memory publication owner.
#[derive(Debug, Clone)]
pub struct TraceEventDraft {
    pub timestamp: i64,
    pub operation: TraceOperation,
}

impl TraceEventDraft {
    pub fn new(timestamp: i64, operation: TraceOperation) -> Self {
        Self {
            timestamp,
            operation,
        }
    }

    /// Starts a new item; its sequence and initial revision belong to the sink.
    pub fn start(
        timestamp: i64,
        turn_id: String,
        item_id: String,
        source: crate::trace::TracePartSource,
        state: crate::trace::TracePartState,
    ) -> Self {
        Self::new(
            timestamp,
            TraceOperation::Start {
                turn_id,
                item_id,
                source,
                state,
            },
        )
    }

    /// Applies a typed action against the latest owner state.
    pub fn apply(
        timestamp: i64,
        turn_id: String,
        item_id: String,
        action: TracePartAction,
    ) -> Self {
        Self::new(
            timestamp,
            TraceOperation::Apply {
                turn_id,
                item_id,
                action,
            },
        )
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

/// Validated next state. Created and accepted while holding one sink's publication lock.
#[derive(Debug)]
pub struct PreparedTraceEvent {
    kind: TraceEventKind,
    item: Option<TracePart>,
}

impl PreparedTraceEvent {
    /// Canonical event to enqueue before accepting the prepared state.
    pub fn kind(&self) -> &TraceEventKind {
        &self.kind
    }
}

/// Synchronous lifecycle ledger shared by canonical sink implementations.
#[derive(Debug, Default)]
pub struct TraceEventLedger {
    parts: BTreeMap<String, TracePart>,
}

impl TraceEventLedger {
    /// Prepares an operation without changing the ledger. Call `accept` only after enqueue succeeds.
    pub fn prepare(
        &self,
        sequence: u64,
        draft: TraceEventDraft,
    ) -> Result<PreparedTraceEvent, TraceEventSinkError> {
        let timestamp = draft.timestamp;
        match draft.operation {
            TraceOperation::Start {
                turn_id,
                item_id,
                source,
                state,
            } => {
                if self.parts.contains_key(&item_id) {
                    return Err(TraceEventSinkError::new(format!(
                        "trace item {item_id} already started"
                    )));
                }
                let item = TracePart::new(turn_id, item_id, sequence, timestamp, source, state);
                Ok(PreparedTraceEvent {
                    kind: TraceEventKind::TracePartStarted { item: item.clone() },
                    item: Some(item),
                })
            }
            TraceOperation::Apply {
                turn_id,
                item_id,
                action,
            } => {
                let mut item = self.parts.get(&item_id).cloned().ok_or_else(|| {
                    TraceEventSinkError::new(format!(
                        "trace operation targets missing item {item_id}"
                    ))
                })?;
                if item.turn_id() != turn_id {
                    return Err(TraceEventSinkError::new(format!(
                        "trace item {item_id} belongs to another turn"
                    )));
                }
                let failed = matches!(
                    &action,
                    TracePartAction::Fail { .. }
                        | TracePartAction::FailTool { .. }
                        | TracePartAction::Cancel { .. }
                        | TracePartAction::DenyTool { .. }
                );
                let delta = if let TracePartAction::Append(delta) = &action {
                    Some(delta.clone())
                } else {
                    None
                };
                let decision = item
                    .apply(item.command(timestamp, action))
                    .map_err(|error| {
                        TraceEventSinkError::new(format!(
                            "trace operation rejected for item {item_id}: {error}"
                        ))
                    })?;
                if let Some(delta) = delta
                    && decision.changed
                {
                    return Ok(PreparedTraceEvent {
                        kind: TraceEventKind::TracePartDelta {
                            event: crate::trace::TracePartDeltaEvent {
                                turn_id,
                                item_id,
                                started_sequence: item.started_sequence(),
                                revision: item.revision(),
                                created_at: item.created_at(),
                                updated_at: item.updated_at(),
                                delta,
                            },
                        },
                        item: Some(item),
                    });
                }
                if item.is_terminal() {
                    if failed || item.failure().is_some() {
                        Ok(PreparedTraceEvent {
                            kind: TraceEventKind::TracePartFailed { item: item.clone() },
                            item: Some(item),
                        })
                    } else {
                        Ok(PreparedTraceEvent {
                            kind: TraceEventKind::TracePartCompleted { item: item.clone() },
                            item: Some(item),
                        })
                    }
                } else {
                    Ok(PreparedTraceEvent {
                        kind: TraceEventKind::TracePartStarted { item: item.clone() },
                        item: Some(item),
                    })
                }
            }
            TraceOperation::InteractionChanged { event } => Ok(PreparedTraceEvent {
                kind: TraceEventKind::InteractionChanged { event },
                item: None,
            }),
            TraceOperation::SkillActivated { activation } => Ok(PreparedTraceEvent {
                kind: TraceEventKind::SkillActivated { activation },
                item: None,
            }),
            TraceOperation::EnabledToolsRecorded { event } => Ok(PreparedTraceEvent {
                kind: TraceEventKind::EnabledToolsRecorded { event },
                item: None,
            }),
        }
    }

    /// Installs the state prepared under the same publication lock, after enqueue succeeds.
    pub fn accept(&mut self, prepared: PreparedTraceEvent) {
        if let Some(item) = prepared.item {
            self.parts.insert(item.item_id().to_owned(), item);
        }
    }

    /// Current item states, independent of event retention.
    pub fn items(&self) -> Vec<TracePart> {
        self.parts.values().cloned().collect()
    }

    /// Latest canonical item snapshot, including output from other producers.
    pub fn item(&self, item_id: &str) -> Option<TracePart> {
        self.parts.get(item_id).cloned()
    }

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
        let timestamp = draft.timestamp;
        let prepared = state.ledger.prepare(sequence, draft)?;
        let event = TraceEvent {
            session_id: self.session_id.clone(),
            sequence,
            timestamp,
            kind: prepared.kind().clone(),
        };
        state.ledger.accept(prepared);
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
