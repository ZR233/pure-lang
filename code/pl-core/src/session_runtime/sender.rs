use std::fmt;

use tokio::sync::oneshot;

use crate::agent_runtime::agent_loop::{AgentLoopCommand, AgentLoopHandle};

use super::{SessionInboxError, SessionMessage, SessionMessageReceipt, SessionWakeEvent};

/// Source-scoped capability to publish messages to one existing Thread owner.
///
/// Clones address the same actor incarnation. They never activate a cold Thread,
/// start a Turn, or rebind to a replacement actor with the same Thread ID.
#[derive(Clone)]
pub struct SessionMessageSender {
    actor: AgentLoopHandle,
    source: String,
}

/// Publication failed before acknowledgement. A cancelled caller may retry the same ID.
#[derive(Debug, thiserror::Error)]
pub enum SessionMessageError {
    #[error("session owner is not ready during construction")]
    NotReady,
    #[error(transparent)]
    Inbox(#[from] SessionInboxError),
    #[error("session message owner is unavailable")]
    OwnerUnavailable,
    #[error("session message commit failed")]
    Commit(#[source] crate::AgentRuntimeError),
}

impl SessionMessageSender {
    pub(crate) fn new(actor: AgentLoopHandle, source: String) -> Result<Self, SessionMessageError> {
        if source.trim().is_empty() || source.len() > 128 {
            return Err(SessionInboxError::InvalidField {
                field: "source",
                limit: 128,
            }
            .into());
        }
        Ok(Self { actor, source })
    }

    /// Publishes extension data without impersonating a framework event.
    ///
    /// Acceptance means the owner committed hot state and registered write-behind
    /// persistence. It does not mean the message has reached the model or disk.
    ///
    /// # Errors
    /// Returns explicit closure, capacity, identity, size, or commit failures.
    /// Cancellation after sending has an unknown receipt; retry with the same ID.
    pub async fn publish(
        &self,
        message: SessionMessage,
    ) -> Result<SessionMessageReceipt, SessionMessageError> {
        let cancellation = self.actor.cancellation();
        tokio::select! {
            result = self.publish_to_owner(message.id.clone(), SessionWakeEvent::Message(message)) => result,
            _ = cancellation.cancelled() => Err(SessionInboxError::Closed.into()),
        }
    }

    /// Publishes with cancellation-safe backpressure, retaining the same message identity.
    ///
    /// # Errors
    /// Returns closure, invalid identity, size, or commit errors. A full inbox suspends
    /// until an owner mutation makes capacity available; no model polling is involved.
    pub async fn publish_wait(
        &self,
        message: SessionMessage,
    ) -> Result<SessionMessageReceipt, SessionMessageError> {
        self.publish_event_wait(message.id.clone(), SessionWakeEvent::Message(message))
            .await
    }

    pub(crate) async fn publish_event_wait(
        &self,
        id: String,
        event: SessionWakeEvent,
    ) -> Result<SessionMessageReceipt, SessionMessageError> {
        let cancellation = self.actor.cancellation();
        let changed = self.actor.inbox_changed();
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let result = tokio::select! {
                result = self.publish_to_owner(id.clone(), event.clone()) => result,
                _ = cancellation.cancelled() => return Err(SessionInboxError::Closed.into()),
            };
            match result {
                Err(SessionMessageError::Inbox(SessionInboxError::Full)) => tokio::select! {
                    _ = notified => {},
                    _ = cancellation.cancelled() => return Err(SessionInboxError::Closed.into()),
                },
                result => return result,
            }
        }
    }

    async fn publish_to_owner(
        &self,
        id: String,
        event: SessionWakeEvent,
    ) -> Result<SessionMessageReceipt, SessionMessageError> {
        self.actor
            .ensure_ready()
            .map_err(|_| SessionMessageError::NotReady)?;
        let (reply, receiver) = oneshot::channel();
        self.actor
            .send(AgentLoopCommand::PublishSessionEvent {
                source: self.source.clone(),
                id,
                event: Box::new(event),
                reply,
            })
            .await
            .map_err(|_| SessionMessageError::OwnerUnavailable)?;
        receiver
            .await
            .map_err(|_| SessionMessageError::OwnerUnavailable)?
    }
}

impl fmt::Debug for SessionMessageSender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionMessageSender")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
