//! Durable opaque messages and owner-assigned consumption watermarks.
use super::*;

/// Producer-selected message content. Context attribution remains Runtime, never Instruction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessage {
    pub id: String,
    pub source_id: String,
    pub payload: OpaquePayload,
    pub context: Vec<ContextContent>,
}

/// Ordered accepted message retained independently of model delivery.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRecord {
    pub sequence: u64,
    pub message: ThreadMessage,
}

impl ThreadMessage {
    /// Content digest used by the bounded consumed-message ledger for duplicate delivery checks.
    ///
    /// The digest covers the immutable message body and its frozen attribution; it never covers the
    /// owner-assigned sequence, which is a framework receipt.
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update([0]);
        hasher.update(self.source_id.as_bytes());
        hasher.update([0]);
        hasher.update(self.payload.format().as_bytes());
        hasher.update([0]);
        hasher.update(self.payload.content().as_bytes());
        for content in &self.context {
            hasher.update([0]);
            hasher.update(serde_json::to_vec(content).unwrap_or_default());
        }
        format!("sha256:{:x}", hasher.finalize())
    }
}

/// Minimal resident identity of a message the owner already consumed.
///
/// The body and its context stay in the durable history; this record is the only thing the live
/// owner keeps so a repeated delivery of the same stable message identity can still be answered
/// idempotently after the inbox entry left the resident queue. A repeat is only a no-op when it
/// matches the stored content digest; the same identity carrying a different body is an identity
/// conflict instead of a second delivery.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageIdentity {
    /// Content digest of the originally accepted message body.
    pub digest: String,
    pub id: String,
    /// Original admission sequence, returned as the repeat receipt.
    pub sequence: u64,
}

impl MessageIdentity {
    /// Projects one consumed inbox record into its minimal durable identity.
    pub fn from_record(record: &InboxRecord) -> Self {
        Self {
            digest: record.message.digest(),
            id: record.message.id.clone(),
            sequence: record.sequence,
        }
    }
}

/// Allocates the next durable message sequence for one Thread.
///
/// The value is the highest of the resident pending tail, the consumption watermark, the bounded
/// consumed-identity ledger and the checkpointed [`ThreadSnapshot::inbox_sequence`]. Only the last
/// one survives consumption pruning, so a message admitted after every earlier message was consumed
/// continues the sequence instead of restarting it. A legacy checkpoint that predates the durable
/// watermark still gets a safe answer because the resident tail and the watermark are considered too.
fn next_sequence(state: &ThreadSnapshot) -> Result<u64, ThreadError> {
    state
        .inbox
        .last()
        .map_or(0, |record| record.sequence)
        .max(state.consumed_messages)
        .max(
            state
                .consumed_message_identities
                .last()
                .map_or(0, |identity| identity.sequence),
        )
        .max(state.inbox_sequence)
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)
}

/// Appends one accepted message under the next admission sequence and advances the Thread's
/// monotonic admission watermark to it.
pub(super) fn admit_message(
    state: &mut ThreadSnapshot,
    message: ThreadMessage,
) -> Result<u64, ThreadError> {
    let sequence = next_sequence(state)?;
    let mut inbox = state.inbox.to_vec();
    inbox.push(InboxRecord { sequence, message });
    state.inbox = inbox.into();
    state.inbox_sequence = sequence;
    Ok(sequence)
}

impl Owner {
    /// Resolves an already accepted message identity to its original admission receipt.
    ///
    /// A message's stable identity is its `id`; its frozen body is the stored content digest. The
    /// resident queue answers the identities still waiting for model context, and the bounded
    /// consumed-identity ledger answers the ones whose body already left the queue. A repeat of an
    /// accepted identity with the same body is therefore a no-op receipt, while the same identity
    /// carrying a different body is an identity conflict and never a second message. `None` means
    /// this owner never accepted the identity, so the caller admits it normally.
    ///
    /// Callers that must not treat a repeat as new work resolve this before any admission check,
    /// so a repeat is never refused because the owner is closing, is waiting for an interaction or
    /// is under storage pressure, and never disturbs the work that is currently running.
    pub(super) fn accepted_message_receipt(
        &self,
        message: &ThreadMessage,
    ) -> Result<Option<u64>, ThreadError> {
        if let Some(record) = self
            .state
            .inbox
            .iter()
            .find(|record| record.message.id == message.id)
        {
            return if record.message == *message {
                Ok(Some(record.sequence))
            } else {
                Err(ThreadError::InvalidIdentity)
            };
        }
        // A consumed message left the resident queue, so its identity ledger answers a repeated
        // delivery: the same digest is an idempotent replay of an already-consumed message, while
        // the same identity carrying a different body stays an identity conflict and is never
        // accepted (or delivered) twice.
        if let Some(identity) = self
            .state
            .consumed_message_identities
            .iter()
            .find(|identity| identity.id == message.id)
        {
            return if identity.digest == message.digest() {
                Ok(Some(identity.sequence))
            } else {
                Err(ThreadError::InvalidIdentity)
            };
        }
        Ok(None)
    }

    pub(super) fn receive_message(
        &mut self,
        message: ThreadMessage,
        drive: Option<input::InputDriverOptions>,
    ) -> Result<u64, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if message.id.is_empty() || message.source_id.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if let Some(sequence) = self.accepted_message_receipt(&message)? {
            self.request_message_wakeup(sequence, drive);
            self.publish();
            self.publish_snapshot();
            return Ok(sequence);
        }
        let sequence = admit_message(&mut self.state, message)?;
        self.request_message_wakeup(sequence, drive);
        self.publish();
        self.publish_snapshot();
        Ok(sequence)
    }

    fn request_message_wakeup(&mut self, sequence: u64, drive: Option<input::InputDriverOptions>) {
        if sequence > self.state.consumed_messages
            && let Some(options) = drive
        {
            self.state.wake_messages_through = self.state.wake_messages_through.max(sequence);
            self.input_driver.wake(options);
        }
    }

    pub(super) fn message_context(&self, turn_id: &str) -> (Vec<ContextRecord>, u64) {
        let mut watermark = self.state.consumed_messages;
        let records = self
            .state
            .inbox
            .iter()
            .filter(|record| record.sequence > self.state.consumed_messages)
            .map(|record| {
                watermark = record.sequence;
                ContextRecord {
                    id: format!("inbox:{}", record.sequence),
                    turn_id: Some(turn_id.to_owned()),
                    source: ContextSource::Runtime {
                        source_id: record.message.source_id.clone(),
                    },
                    content: record.message.context.clone(),
                    tool_calls: Vec::new(),
                }
            })
            .collect();
        (records, watermark)
    }
}
