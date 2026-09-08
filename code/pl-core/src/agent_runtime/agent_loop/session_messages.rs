use crate::session_runtime::{
    SessionInboxError, SessionMessageError, SessionMessageReceipt, SessionWakeEvent,
};

use super::super::{AgentRuntimeHost, DurableCommitFacts, ThreadMutation};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};

pub(super) type SessionWaitReply = tokio::sync::oneshot::Sender<
    Result<crate::session_runtime::SessionEventBatch, SessionMessageError>,
>;

impl<H: AgentRuntimeHost> AgentLoop<H> {
    pub(super) fn wait_session_events(&mut self, reply: SessionWaitReply) {
        if self
            .session_waiter
            .as_ref()
            .is_some_and(|waiter| !waiter.is_closed())
        {
            let _ = reply.send(Err(SessionInboxError::AlreadyWaiting.into()));
            return;
        }
        self.session_waiter = Some(reply);
        self.wake_session_waiter();
    }

    pub(super) fn wake_session_waiter(&mut self) {
        if !self.state.snapshot.state.is_operational() || self.state.session.inbox.is_closed() {
            if let Some(reply) = self.session_waiter.take() {
                let _ = reply.send(Err(SessionInboxError::Closed.into()));
            }
        } else if self.state.session.inbox.has_pending()
            && let Some(reply) = self.session_waiter.take()
        {
            let batch = self
                .state
                .session
                .inbox
                .offer(std::num::NonZeroUsize::new(64).expect("nonzero batch limit"));
            let _ = reply.send(batch.map_err(Into::into));
        }
    }

    pub(super) async fn publish_session_event(
        &mut self,
        source: &str,
        id: &str,
        event: SessionWakeEvent,
    ) -> Result<SessionMessageReceipt, SessionMessageError> {
        if !self.state.snapshot.state.is_operational() {
            return Err(SessionInboxError::Closed.into());
        }
        let mut next = self.state.clone();
        let now = super::super::state::unix_timestamp();
        let receipt = next.session.inbox.publish_event(source, id, event, now)?;
        if matches!(receipt, SessionMessageReceipt::Duplicate { .. }) {
            return Ok(receipt);
        }
        next.snapshot.revision = next.snapshot.revision.saturating_add(1);
        next.snapshot.updated_at = now;
        let facts = DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), None, None);
        self.commit_and_publish(
            PendingCommit::new(next, facts, ThreadMutation::SnapshotAndQueue)
                .persistence(super::super::PersistenceClass::Standard)
                .publish(
                    CommitPublication::new(Some(self.state.snapshot.identity.id.clone()), None)
                        .store_directory_snapshot(),
                ),
        )
        .await
        .map_err(SessionMessageError::Commit)?;
        self.wake_session_waiter();
        Ok(receipt)
    }
}
