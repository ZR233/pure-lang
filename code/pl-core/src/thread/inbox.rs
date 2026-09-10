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

impl Owner {
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
        if let Some(existing) = self
            .state
            .inbox
            .iter()
            .find(|record| record.message.id == message.id)
        {
            if existing.message != message {
                return Err(ThreadError::InvalidIdentity);
            }
            let sequence = existing.sequence;
            self.request_message_wakeup(sequence, drive);
            self.publish();
            self.publish_snapshot();
            return Ok(sequence);
        }
        let sequence = self
            .state
            .inbox
            .last()
            .map_or(0, |record| record.sequence)
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        let mut inbox = self.state.inbox.to_vec();
        inbox.push(InboxRecord { sequence, message });
        self.state.inbox = inbox.into();
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
            self.input_driver = Some(options);
            self.input_driver_error = None;
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
