use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    SessionEventBatch, SessionEventEnvelope, SessionMessage, SessionMessageReceipt,
    SessionWakeEvent,
};

/// Hard memory bounds, independent of whether a model is currently waiting.
#[derive(Debug, Clone, Copy)]
pub struct SessionInboxLimits {
    pending_events: NonZeroUsize,
    event_bytes: NonZeroUsize,
    retained_identities: NonZeroUsize,
}

impl SessionInboxLimits {
    /// Sets event count, encoded event size, and lifetime deduplication bounds.
    pub fn new(
        pending_events: NonZeroUsize,
        event_bytes: NonZeroUsize,
        retained_identities: NonZeroUsize,
    ) -> Self {
        Self {
            pending_events,
            event_bytes,
            retained_identities,
        }
    }
}

impl Default for SessionInboxLimits {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(256).expect("nonzero constant"),
            NonZeroUsize::new(64 * 1024).expect("nonzero constant"),
            NonZeroUsize::new(65_536).expect("nonzero constant"),
        )
    }
}

/// Typed publication and checkpoint rejection. Rejection never consumes an event.
#[derive(Debug, thiserror::Error)]
pub enum SessionInboxError {
    #[error("session inbox is closed")]
    Closed,
    #[error("session already has a pending event waiter")]
    AlreadyWaiting,
    #[error("session inbox pending event capacity reached")]
    Full,
    #[error("session inbox deduplication capacity reached")]
    IdentityCapacity,
    #[error("session event reservation {id} does not exist")]
    MissingReservation { id: String },
    #[error("session event exceeds {limit} encoded bytes")]
    TooLarge { limit: usize },
    #[error("{field} must be nonempty and at most {limit} bytes")]
    InvalidField { field: &'static str, limit: usize },
    #[error("message identity {source_id}/{message_id} already has different content")]
    IdentityConflict {
        source_id: String,
        message_id: String,
    },
    #[error("session event sequence exhausted")]
    SequenceExhausted,
    #[error("wait acknowledgement does not match the offered event prefix")]
    InvalidAcknowledgement,
    #[error("invalid inbox snapshot: {reason}")]
    InvalidSnapshot { reason: &'static str },
    #[error("cannot encode session event")]
    Encoding(#[from] serde_json::Error),
}

/// Durable owner state. Deserialization alone does not validate its invariants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionInboxSnapshot {
    published_sequence: u64,
    consumed_sequence: u64,
    pending: VecDeque<SessionEventEnvelope>,
    identities: BTreeMap<String, BTreeMap<String, MessageIdentity>>,
    #[serde(default)]
    reservations: BTreeSet<String>,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageIdentity {
    sequence: u64,
    digest: String,
}

/// Single-owner inbox. There is no internal lock, task, or global event registry.
#[derive(Debug, Clone)]
pub struct SessionInbox {
    snapshot: Arc<SessionInboxSnapshot>,
    limits: SessionInboxLimits,
    identity_count: usize,
}

impl SessionInbox {
    /// Creates an empty inbox for one session owner.
    pub fn new(limits: SessionInboxLimits) -> Self {
        Self {
            snapshot: Arc::new(SessionInboxSnapshot::default()),
            limits,
            identity_count: 0,
        }
    }

    /// Restores persisted state, retaining consumption and deduplication identities.
    ///
    /// # Errors
    /// Rejects inconsistent sequences, invalid identities, and exceeded limits.
    pub fn restore(
        snapshot: SessionInboxSnapshot,
        limits: SessionInboxLimits,
    ) -> Result<Self, SessionInboxError> {
        let identity_count = snapshot.identities.values().map(BTreeMap::len).sum();
        let inbox = Self {
            snapshot: Arc::new(snapshot),
            limits,
            identity_count,
        };
        inbox.validate_snapshot()?;
        Ok(inbox)
    }

    pub(crate) fn validate_task_references(
        &self,
        tasks: &super::SessionTasks,
    ) -> Result<(), SessionInboxError> {
        for envelope in &self.snapshot.pending {
            if let SessionWakeEvent::ToolFinished(task) = &envelope.event
                && tasks.get(&task.receipt.task_id).ok() != Some(task)
            {
                return Err(SessionInboxError::InvalidSnapshot {
                    reason: "terminal event differs from its canonical task result",
                });
            }
        }
        Ok(())
    }

    pub(crate) fn migrate_task_previews(&mut self) -> Result<(), SessionInboxError> {
        for envelope in &mut Arc::make_mut(&mut self.snapshot).pending {
            if let SessionWakeEvent::ToolFinished(task) = &mut envelope.event {
                *task = super::model_view::task(task).map_err(|_| {
                    SessionInboxError::InvalidSnapshot {
                        reason: "cannot migrate task result preview",
                    }
                })?;
            }
        }
        self.validate_snapshot()
    }

    /// Returns immutable state to include in the owning Thread checkpoint.
    pub fn snapshot(&self) -> &SessionInboxSnapshot {
        &self.snapshot
    }

    /// Prevents all subsequent publications, including duplicate retries.
    pub fn close(&mut self) {
        Arc::make_mut(&mut self.snapshot).closed = true;
    }

    /// Whether this owner has stopped accepting messages.
    pub fn is_closed(&self) -> bool {
        self.snapshot.closed
    }

    /// Whether there are unacknowledged events, including events offered by wait.
    pub fn has_pending(&self) -> bool {
        !self.snapshot.pending.is_empty()
    }

    /// Reserves capacity for a framework task's eventual terminal event.
    ///
    /// # Errors
    /// Rejects closed or full inboxes and invalid reservation identities.
    pub fn reserve(&mut self, id: &str) -> Result<(), SessionInboxError> {
        if self.snapshot.closed {
            return Err(SessionInboxError::Closed);
        }
        validate_field("reservation id", id, 256)?;
        if self.snapshot.reservations.contains(id) {
            return Ok(());
        }
        if self.snapshot.pending.len() + self.snapshot.reservations.len()
            >= self.limits.pending_events.get()
        {
            return Err(SessionInboxError::Full);
        }
        Arc::make_mut(&mut self.snapshot)
            .reservations
            .insert(id.to_owned());
        Ok(())
    }

    /// Publishes a framework event using previously reserved capacity.
    ///
    /// May settle already accepted work after closure. Extension publishers cannot
    /// access this method through their source-scoped sender.
    ///
    /// # Errors
    /// Rejects absent reservations, invalid sources, oversized events, or sequence exhaustion.
    pub fn publish_reserved(
        &mut self,
        id: &str,
        source: &str,
        event: SessionWakeEvent,
        created_at: i64,
    ) -> Result<u64, SessionInboxError> {
        validate_field("source", source, 128)?;
        if !self.snapshot.reservations.contains(id) {
            return Err(SessionInboxError::MissingReservation { id: id.to_owned() });
        }
        let mut next = self.clone();
        Arc::make_mut(&mut next.snapshot).reservations.remove(id);
        let sequence = next.append(source, event, created_at)?;
        *self = next;
        Ok(sequence)
    }

    /// Returns the reservations used to audit task/inbox recovery consistency.
    pub fn reservations(&self) -> impl Iterator<Item = &str> {
        self.snapshot.reservations.iter().map(String::as_str)
    }

    /// Releases an unused framework reservation when a source is disabled or closes.
    pub(crate) fn release_reservation(&mut self, id: &str) {
        Arc::make_mut(&mut self.snapshot).reservations.remove(id);
    }

    /// Stages one extension message from a runtime-authenticated source.
    ///
    /// The owner must commit this mutation before returning the receipt or waking
    /// subscribers. This method does not perform I/O or start a model Turn.
    ///
    /// # Errors
    /// Rejects closed/full inboxes, invalid or conflicting identities, and oversized data.
    pub fn publish_message(
        &mut self,
        source: &str,
        message: SessionMessage,
        created_at: i64,
    ) -> Result<SessionMessageReceipt, SessionInboxError> {
        let id = message.id.clone();
        self.publish_event(source, &id, SessionWakeEvent::Message(message), created_at)
    }

    pub(crate) fn publish_event(
        &mut self,
        source: &str,
        id: &str,
        event: SessionWakeEvent,
        created_at: i64,
    ) -> Result<SessionMessageReceipt, SessionInboxError> {
        if self.snapshot.closed {
            return Err(SessionInboxError::Closed);
        }
        validate_field("source", source, 128)?;
        validate_field("message id", id, 256)?;
        if let SessionWakeEvent::Message(message) = &event {
            validate_field("message kind", &message.kind, 128)?;
            validate_field("message text", &message.text, self.limits.event_bytes.get())?;
        }
        let value = serde_json::to_value(&event)?;
        let digest = crate::canonical_json_hash(&value);
        if let Some(existing) = self
            .snapshot
            .identities
            .get(source)
            .and_then(|entries| entries.get(id))
        {
            return if existing.digest == digest {
                Ok(SessionMessageReceipt::Duplicate {
                    sequence: existing.sequence,
                })
            } else {
                Err(SessionInboxError::IdentityConflict {
                    source_id: source.to_owned(),
                    message_id: id.to_owned(),
                })
            };
        }
        if self.identity_count >= self.limits.retained_identities.get() {
            return Err(SessionInboxError::IdentityCapacity);
        }
        let sequence = self.append(source, event, created_at)?;
        Arc::make_mut(&mut self.snapshot)
            .identities
            .entry(source.to_owned())
            .or_default()
            .insert(id.to_owned(), MessageIdentity { sequence, digest });
        self.identity_count += 1;
        Ok(SessionMessageReceipt::Accepted { sequence })
    }

    /// Offers a count- and byte-bounded prefix without consuming it.
    ///
    /// # Errors
    /// Returns an encoding error without changing the consumption watermark.
    pub fn offer(&self, limit: NonZeroUsize) -> Result<SessionEventBatch, SessionInboxError> {
        let mut events = Vec::new();
        let mut bytes = 0usize;
        for event in self.snapshot.pending.iter().take(limit.get()) {
            let event_bytes = serde_json::to_vec(event)?.len();
            if !events.is_empty()
                && bytes.saturating_add(event_bytes) > self.limits.event_bytes.get()
            {
                break;
            }
            bytes = bytes.saturating_add(event_bytes);
            events.push(event.clone());
        }
        Ok(SessionEventBatch {
            after_sequence: self.snapshot.consumed_sequence,
            through_sequence: events
                .last()
                .map_or(self.snapshot.consumed_sequence, |event| event.sequence),
            has_more: self.snapshot.pending.len() > events.len(),
            events,
        })
    }

    /// Stages consumption of exactly the prefix included in a wait response.
    ///
    /// Call on the same owner-state clone as the transcript mutation. Install that
    /// clone only when the whole checkpoint commits; dropping it rolls both back.
    ///
    /// # Errors
    /// Rejects stale watermarks, skipped events, or altered event content.
    pub fn acknowledge(&mut self, offered: &SessionEventBatch) -> Result<(), SessionInboxError> {
        let count = offered.events.len();
        if offered.after_sequence != self.snapshot.consumed_sequence
            || count > self.snapshot.pending.len()
            || !self.snapshot.pending.iter().take(count).eq(&offered.events)
            || offered.through_sequence
                != offered
                    .events
                    .last()
                    .map_or(offered.after_sequence, |event| event.sequence)
        {
            return Err(SessionInboxError::InvalidAcknowledgement);
        }
        let snapshot = Arc::make_mut(&mut self.snapshot);
        snapshot.pending.drain(..count);
        snapshot.consumed_sequence = offered.through_sequence;
        Ok(())
    }

    fn append(
        &mut self,
        source: &str,
        event: SessionWakeEvent,
        created_at: i64,
    ) -> Result<u64, SessionInboxError> {
        if self.snapshot.pending.len() + self.snapshot.reservations.len()
            >= self.limits.pending_events.get()
        {
            return Err(SessionInboxError::Full);
        }
        let sequence = self
            .snapshot
            .published_sequence
            .checked_add(1)
            .ok_or(SessionInboxError::SequenceExhausted)?;
        let envelope = SessionEventEnvelope {
            sequence,
            source: source.to_owned(),
            created_at,
            event,
        };
        self.validate_event_size(&envelope)?;
        let snapshot = Arc::make_mut(&mut self.snapshot);
        snapshot.pending.push_back(envelope);
        snapshot.published_sequence = sequence;
        Ok(sequence)
    }

    fn validate_event_size(&self, event: &SessionEventEnvelope) -> Result<(), SessionInboxError> {
        let limit = self.limits.event_bytes.get();
        if serde_json::to_vec(event)?.len() > limit {
            return Err(SessionInboxError::TooLarge { limit });
        }
        Ok(())
    }

    fn validate_snapshot(&self) -> Result<(), SessionInboxError> {
        let invalid = |reason| SessionInboxError::InvalidSnapshot { reason };
        let state = &self.snapshot;
        if state
            .published_sequence
            .checked_sub(state.consumed_sequence)
            != Some(state.pending.len() as u64)
        {
            return Err(invalid("watermarks do not match pending events"));
        }
        if state.pending.len() + state.reservations.len() > self.limits.pending_events.get()
            || self.identity_count > self.limits.retained_identities.get()
        {
            return Err(invalid("restored state exceeds configured capacity"));
        }
        for id in &state.reservations {
            validate_field("reservation id", id, 256)?;
        }
        let mut expected = state.consumed_sequence;
        for event in &state.pending {
            expected = expected
                .checked_add(1)
                .ok_or_else(|| invalid("event sequence overflow"))?;
            if event.sequence != expected {
                return Err(invalid("pending events are not a contiguous prefix"));
            }
            validate_field("source", &event.source, 128)?;
            self.validate_event_size(event)?;
            if let SessionWakeEvent::Message(message) = &event.event {
                validate_field("message id", &message.id, 256)?;
                validate_field("message kind", &message.kind, 128)?;
                validate_field("message text", &message.text, self.limits.event_bytes.get())?;
                let identity = state
                    .identities
                    .get(&event.source)
                    .and_then(|entries| entries.get(&message.id))
                    .ok_or_else(|| invalid("pending message has no deduplication identity"))?;
                if identity.sequence != event.sequence
                    || identity.digest
                        != crate::canonical_json_hash(&serde_json::to_value(&event.event)?)
                {
                    return Err(invalid("pending message conflicts with its identity"));
                }
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (source, identities) in &state.identities {
            validate_field("source", source, 128)?;
            if identities.is_empty() {
                return Err(invalid("empty source identity group"));
            }
            for (id, identity) in identities {
                validate_field("message id", id, 256)?;
                if identity.sequence > state.consumed_sequence {
                    let pending = state
                        .pending
                        .iter()
                        .find(|event| event.sequence == identity.sequence);
                    if !pending.is_some_and(|event| {
                        event.source == *source
                            && match &event.event {
                                SessionWakeEvent::Message(message) => message.id == *id,
                                SessionWakeEvent::AgentChanged(_) => true,
                                SessionWakeEvent::ToolFinished(_)
                                | SessionWakeEvent::TimerElapsed(_)
                                | SessionWakeEvent::SourceFailed(_) => false,
                            }
                    }) {
                        return Err(invalid("retained identity has no matching pending message"));
                    }
                    if let Some(event) = pending
                        && identity.digest
                            != crate::canonical_json_hash(&serde_json::to_value(&event.event)?)
                    {
                        return Err(invalid(
                            "retained identity digest conflicts with pending event",
                        ));
                    }
                }
                if identity.sequence == 0
                    || identity.sequence > state.published_sequence
                    || !seen.insert(identity.sequence)
                    || !identity
                        .digest
                        .strip_prefix("sha256:")
                        .is_some_and(|digest| {
                            digest.len() == 64
                                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                {
                    return Err(invalid("invalid retained identity"));
                }
            }
        }
        Ok(())
    }
}

fn validate_field(field: &'static str, value: &str, limit: usize) -> Result<(), SessionInboxError> {
    if value.trim().is_empty() || value.len() > limit {
        return Err(SessionInboxError::InvalidField { field, limit });
    }
    Ok(())
}
