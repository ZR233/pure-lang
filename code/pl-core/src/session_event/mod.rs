use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};

use pl_protocol::{
    SessionEventEnvelope, SessionEventPosition, SessionResyncReason, SessionStreamFrame,
    SessionSubscriptionRequest, SessionViewSnapshot,
};
use tokio::sync::broadcast;

mod fact;
mod interaction;
mod observation;
mod projector;
mod reducer;
mod trace_part;
pub(crate) use fact::project_session_facts;
pub use fact::{SessionEventFact, SessionEventFactPosition};
pub(crate) use observation::{
    ObservedTurnEvent, TurnObservation, compaction_observation, observation_from_agent_event,
    project_observation,
};
pub(crate) use projector::{project_runtime_event, project_trace_events, runtime_event_session_id};
use reducer::apply_session_event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEventOptions {
    pub channel_capacity: usize,
    pub replay_limit: usize,
    pub retained_durable_events: usize,
}

impl Default for SessionEventOptions {
    fn default() -> Self {
        Self {
            channel_capacity: 1024,
            replay_limit: 1000,
            retained_durable_events: 4096,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEventError {
    SessionMismatch {
        expected: String,
        actual: String,
    },
    ExpectedDurable,
    ExpectedTransient,
    SequenceGap {
        expected: u64,
        actual: u64,
    },
    RevisionGap {
        part_id: String,
        expected: u64,
        actual: u64,
    },
    ProjectionInvariant(String),
    LockPoisoned,
}

impl fmt::Display for SessionEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionMismatch { expected, actual } => {
                write!(
                    formatter,
                    "session event targets {actual}, expected {expected}"
                )
            }
            Self::ExpectedDurable => formatter.write_str("expected a durable session event"),
            Self::ExpectedTransient => formatter.write_str("expected a transient session event"),
            Self::SequenceGap { expected, actual } => {
                write!(
                    formatter,
                    "durable sequence gap: expected {expected}, got {actual}"
                )
            }
            Self::RevisionGap {
                part_id,
                expected,
                actual,
            } => write!(
                formatter,
                "part {part_id} revision gap: expected {expected}, got {actual}"
            ),
            Self::ProjectionInvariant(message) => formatter.write_str(message),
            Self::LockPoisoned => formatter.write_str("session event state lock poisoned"),
        }
    }
}

impl std::error::Error for SessionEventError {}

#[derive(Clone)]
pub struct SessionEventHub {
    inner: Arc<HubInner>,
}

struct HubInner {
    options: SessionEventOptions,
    sessions: RwLock<BTreeMap<String, Arc<SessionChannel>>>,
}

struct SessionChannel {
    session_id: String,
    sender: broadcast::Sender<SessionStreamFrame>,
    state: Mutex<SessionChannelState>,
}

struct SessionChannelState {
    durable_snapshot: SessionViewSnapshot,
    live_snapshot: SessionViewSnapshot,
    durable_events: VecDeque<SessionEventEnvelope>,
}

impl SessionEventHub {
    pub fn new(options: SessionEventOptions) -> Self {
        Self {
            inner: Arc::new(HubInner {
                options,
                sessions: RwLock::new(BTreeMap::new()),
            }),
        }
    }

    pub fn handle(&self) -> SessionEventHubHandle {
        SessionEventHubHandle { hub: self.clone() }
    }

    pub fn replace_snapshot(
        &self,
        snapshot: SessionViewSnapshot,
        durable_events: Vec<SessionEventEnvelope>,
    ) -> Result<(), SessionEventError> {
        let channel = self.channel(&snapshot.session_id)?;
        let mut state = channel
            .state
            .lock()
            .map_err(|_| SessionEventError::LockPoisoned)?;
        state.durable_snapshot = snapshot.clone();
        state.live_snapshot = snapshot;
        state.durable_events = durable_events
            .into_iter()
            .filter(|event| event.position.durable_sequence().is_some())
            .collect();
        trim_journal(&mut state.durable_events, self.inner.options);
        Ok(())
    }

    pub fn publish_durable(&self, event: SessionEventEnvelope) -> Result<(), SessionEventError> {
        if event.position.durable_sequence().is_none() {
            return Err(SessionEventError::ExpectedDurable);
        }
        self.publish_batch(vec![event])
    }

    pub fn publish_transient(&self, event: SessionEventEnvelope) -> Result<(), SessionEventError> {
        if matches!(event.position, SessionEventPosition::Durable { .. }) {
            return Err(SessionEventError::ExpectedTransient);
        }
        self.publish_batch(vec![event])
    }

    fn publish_batch(&self, events: Vec<SessionEventEnvelope>) -> Result<(), SessionEventError> {
        let Some(first) = events.first() else {
            return Ok(());
        };
        let started_at = std::time::Instant::now();
        let session_id = first.session_id.clone();
        if let Some(other) = events.iter().find(|event| event.session_id != session_id) {
            return Err(SessionEventError::SessionMismatch {
                expected: session_id,
                actual: other.session_id.clone(),
            });
        }
        let channel = self.channel(&session_id)?;
        let mut state = channel
            .state
            .lock()
            .map_err(|_| SessionEventError::LockPoisoned)?;
        let mut durable_snapshot = state.durable_snapshot.clone();
        let mut live_snapshot = state.live_snapshot.clone();
        let mut appended_durable = Vec::new();
        for event in &events {
            match event.position {
                SessionEventPosition::Durable { sequence } => {
                    let expected = durable_snapshot.through_sequence.saturating_add(1);
                    if sequence != expected {
                        return Err(SessionEventError::SequenceGap {
                            expected,
                            actual: sequence,
                        });
                    }
                    apply_session_event(&mut durable_snapshot, event)?;
                    durable_snapshot.through_sequence = sequence;
                    apply_session_event(&mut live_snapshot, event)?;
                    live_snapshot.through_sequence = sequence;
                    appended_durable.push(event.clone());
                }
                SessionEventPosition::Transient { revision: _ } => {
                    apply_session_event(&mut live_snapshot, event)?;
                }
            }
        }
        state.durable_snapshot = durable_snapshot;
        state.live_snapshot = live_snapshot;
        state.durable_events.extend(appended_durable);
        trim_journal(&mut state.durable_events, self.inner.options);
        let part_count = state.live_snapshot.parts.len();
        let message_count = state.live_snapshot.messages.len();
        let journal_count = state.durable_events.len();
        drop(state);
        tracing::trace!(
            session_id,
            batch_events = events.len(),
            message_count,
            part_count,
            journal_count,
            elapsed_micros = started_at.elapsed().as_micros(),
            "published session event batch"
        );
        for event in events {
            let _ = channel.sender.send(SessionStreamFrame::Event {
                event: Box::new(event),
            });
        }
        Ok(())
    }

    pub fn snapshot(&self, session_id: &str) -> Result<SessionViewSnapshot, SessionEventError> {
        let channel = self.channel(session_id)?;
        let snapshot = channel
            .state
            .lock()
            .map_err(|_| SessionEventError::LockPoisoned)?
            .live_snapshot
            .clone();
        Ok(snapshot)
    }

    pub(crate) fn project_durable(
        &self,
        session_id: &str,
        events: &[SessionEventEnvelope],
    ) -> Result<SessionViewSnapshot, SessionEventError> {
        let channel = self.channel(session_id)?;
        let state = channel
            .state
            .lock()
            .map_err(|_| SessionEventError::LockPoisoned)?;
        let mut snapshot = state.durable_snapshot.clone();
        for event in events {
            let Some(sequence) = event.position.durable_sequence() else {
                return Err(SessionEventError::ExpectedDurable);
            };
            let expected = snapshot.through_sequence.saturating_add(1);
            if sequence != expected {
                return Err(SessionEventError::SequenceGap {
                    expected,
                    actual: sequence,
                });
            }
            apply_session_event(&mut snapshot, event)?;
            snapshot.through_sequence = sequence;
        }
        Ok(snapshot)
    }

    pub fn subscribe(
        &self,
        request: SessionSubscriptionRequest,
    ) -> Result<SessionEventSubscription, SessionEventError> {
        let channel = self.channel(&request.session_id)?;
        let state = channel
            .state
            .lock()
            .map_err(|_| SessionEventError::LockPoisoned)?;
        let receiver = channel.sender.subscribe();
        let bootstrap = bootstrap_frames(&state, request.after_sequence, self.inner.options);
        drop(state);
        Ok(SessionEventSubscription {
            session_id: channel.session_id.clone(),
            bootstrap,
            receiver,
            terminal: false,
        })
    }

    fn channel(&self, session_id: &str) -> Result<Arc<SessionChannel>, SessionEventError> {
        if let Some(channel) = self
            .inner
            .sessions
            .read()
            .map_err(|_| SessionEventError::LockPoisoned)?
            .get(session_id)
            .cloned()
        {
            return Ok(channel);
        }
        let mut sessions = self
            .inner
            .sessions
            .write()
            .map_err(|_| SessionEventError::LockPoisoned)?;
        Ok(sessions
            .entry(session_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(self.inner.options.channel_capacity.max(1));
                Arc::new(SessionChannel {
                    session_id: session_id.to_string(),
                    sender,
                    state: Mutex::new(SessionChannelState {
                        durable_snapshot: SessionViewSnapshot::empty(session_id),
                        live_snapshot: SessionViewSnapshot::empty(session_id),
                        durable_events: VecDeque::new(),
                    }),
                })
            })
            .clone())
    }
}

impl Default for SessionEventHub {
    fn default() -> Self {
        Self::new(SessionEventOptions::default())
    }
}

impl fmt::Debug for SessionEventHub {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionEventHub")
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct SessionEventHubHandle {
    hub: SessionEventHub,
}

impl SessionEventHubHandle {
    pub fn subscribe(
        &self,
        request: SessionSubscriptionRequest,
    ) -> Result<SessionEventSubscription, SessionEventError> {
        self.hub.subscribe(request)
    }

    pub fn snapshot(&self, session_id: &str) -> Result<SessionViewSnapshot, SessionEventError> {
        self.hub.snapshot(session_id)
    }

    pub(crate) fn replace_snapshot(
        &self,
        snapshot: SessionViewSnapshot,
        durable_events: Vec<SessionEventEnvelope>,
    ) -> Result<(), SessionEventError> {
        self.hub.replace_snapshot(snapshot, durable_events)
    }

    pub(crate) fn project_durable(
        &self,
        session_id: &str,
        events: &[SessionEventEnvelope],
    ) -> Result<SessionViewSnapshot, SessionEventError> {
        self.hub.project_durable(session_id, events)
    }

    pub(crate) fn publish_batch(
        &self,
        events: Vec<SessionEventEnvelope>,
    ) -> Result<(), SessionEventError> {
        self.hub.publish_batch(events)
    }
}

pub struct SessionEventSubscription {
    session_id: String,
    bootstrap: VecDeque<SessionStreamFrame>,
    receiver: broadcast::Receiver<SessionStreamFrame>,
    terminal: bool,
}

impl SessionEventSubscription {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub async fn recv(&mut self) -> Option<SessionStreamFrame> {
        if let Some(frame) = self.bootstrap.pop_front() {
            return Some(frame);
        }
        if self.terminal {
            return None;
        }
        match self.receiver.recv().await {
            Ok(frame) => Some(frame),
            Err(broadcast::error::RecvError::Lagged(events)) => {
                self.terminal = true;
                Some(SessionStreamFrame::ResyncRequired {
                    reason: SessionResyncReason::Lagged { events },
                })
            }
            Err(broadcast::error::RecvError::Closed) => {
                self.terminal = true;
                None
            }
        }
    }
}

impl fmt::Debug for SessionEventSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionEventSubscription")
            .field("session_id", &self.session_id)
            .field("bootstrap_frames", &self.bootstrap.len())
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

fn bootstrap_frames(
    state: &SessionChannelState,
    after_sequence: Option<u64>,
    options: SessionEventOptions,
) -> VecDeque<SessionStreamFrame> {
    let Some(after_sequence) = after_sequence else {
        return VecDeque::from([SessionStreamFrame::Snapshot {
            snapshot: Box::new(state.live_snapshot.clone()),
        }]);
    };
    if after_sequence >= state.durable_snapshot.through_sequence {
        return VecDeque::new();
    }
    let oldest = state
        .durable_events
        .front()
        .and_then(|event| event.position.durable_sequence())
        .unwrap_or_else(|| state.durable_snapshot.through_sequence.saturating_add(1));
    if after_sequence.saturating_add(1) < oldest {
        return VecDeque::from([SessionStreamFrame::Snapshot {
            snapshot: Box::new(state.live_snapshot.clone()),
        }]);
    }
    let replay = state
        .durable_events
        .iter()
        .filter(|event| {
            event
                .position
                .durable_sequence()
                .is_some_and(|sequence| sequence > after_sequence)
        })
        .cloned()
        .collect::<Vec<_>>();
    if replay.len() > options.replay_limit {
        return VecDeque::from([SessionStreamFrame::Snapshot {
            snapshot: Box::new(state.live_snapshot.clone()),
        }]);
    }
    replay
        .into_iter()
        .map(|event| SessionStreamFrame::Event {
            event: Box::new(event),
        })
        .collect()
}

fn trim_journal(events: &mut VecDeque<SessionEventEnvelope>, options: SessionEventOptions) {
    while events.len() > options.retained_durable_events {
        events.pop_front();
    }
}

#[cfg(test)]
mod tests;
