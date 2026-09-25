//! Product observation registration; callbacks execute outside the registry lock.
use super::*;

type Observe = dyn Fn(String, ThreadHandle) + Send + Sync;
type Drain = dyn Fn(String) -> futures::future::BoxFuture<'static, Result<(), ThreadAssemblyError>>
    + Send
    + Sync;
/// Durable record of one accepted message identity inside a Thread's history.
///
/// The index is the authority a delivery uses once it left core's bounded resident window. A
/// migrated row may prove that the identity was accepted without proving the body, which must never
/// be re-delivered as new work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DurableMessageIdentity {
    /// The accepted body digest and its original admission sequence.
    Proven { sequence: u64, digest: String },
    /// The identity is recorded but its accepted body cannot be verified.
    Unverifiable,
}

/// Durable lookup of one `(target thread, message id)` identity.
///
/// A named alias keeps the install signature readable; the callback runs outside the registry lock
/// and its future must stay `Send` because the observation worker is a spawned task. `None` answers
/// "this Thread never accepted that identity"; a failure must be propagated so the caller fails
/// closed instead of admitting a possible duplicate.
pub(crate) type MessageQuery = futures::future::BoxFuture<
    'static,
    Result<Option<DurableMessageIdentity>, ThreadAssemblyError>,
>;
pub(crate) type MessageIdentityLookup = dyn Fn(String, String) -> MessageQuery + Send + Sync;

/// Outcome of adjudicating one delivery against the durable message identity index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeliveryDisposition {
    /// The identity is already accepted with an identical body: answer with this original receipt.
    Receipt(u64),
    /// The same identity is accepted with a provably different body.
    Conflict,
    /// The identity is recorded but its accepted body cannot be verified (migrated row).
    Unverifiable,
    /// The Thread never accepted this identity: a normal admission may proceed.
    New,
}
#[derive(Clone)]
pub(super) struct AssemblyObservation(Arc<Observe>, Arc<Drain>, Option<Arc<MessageIdentityLookup>>);
impl std::fmt::Debug for AssemblyObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssemblyObservation")
    }
}
impl StudioThreadAssembler {
    pub(crate) fn observe_assembly(
        &self,
        observe: impl Fn(String, ThreadHandle) + Send + Sync + 'static,
        drain: impl Fn(String) -> futures::future::BoxFuture<'static, Result<(), ThreadAssemblyError>>
        + Send
        + Sync
        + 'static,
        messages: Option<Arc<MessageIdentityLookup>>,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing
            || !state.entries.is_empty()
            || !state.creating.is_empty()
            || state.observation.is_some()
        {
            return Err(ThreadAssemblyError::Closed);
        }
        state.observation = Some(AssemblyObservation(
            Arc::new(observe),
            Arc::new(drain),
            messages,
        ));
        Ok(())
    }
    pub(super) fn publish_observation(&self, id: &str, thread: &ThreadHandle) {
        let observer = self.0.state().observation.clone();
        if let Some(observer) = observer {
            (observer.0)(id.to_owned(), thread.clone());
        }
    }
    pub(super) async fn drain_observation(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        let observer = self.0.state().observation.clone();
        if let Some(observer) = observer {
            let result = (observer.1)(id.to_owned()).await;
            result?;
        }
        Ok(())
    }

    /// Adjudicates one message delivery against the target Thread's durable identity index.
    ///
    /// Every admission site must run this before it accepts a message as new work, because a delivery
    /// whose resident copy is gone would otherwise be admitted a second time. The result is the
    /// original admission receipt for an identical repeat, `New` for an identity this Thread never
    /// accepted, and an explicit conflict/unverifiable disposition for the same identity carrying a
    /// body the durable index cannot match. A storage failure is propagated so the caller fails
    /// closed instead of admitting a possible duplicate.
    ///
    /// An assembler without an installed Studio host has no durable index; that host always installs
    /// one, and the bounded resident dedup in core still covers the live window in the meantime.
    pub(super) async fn adjudicate_message(
        &self,
        thread_id: &str,
        message: &pl_core::thread::inbox::ThreadMessage,
    ) -> Result<DeliveryDisposition, ThreadAssemblyError> {
        match self
            .durable_message_identity(thread_id, &message.id)
            .await?
        {
            None => Ok(DeliveryDisposition::New),
            Some(DurableMessageIdentity::Proven { sequence, digest })
                if digest == message.digest() =>
            {
                Ok(DeliveryDisposition::Receipt(sequence))
            }
            Some(DurableMessageIdentity::Proven { .. }) => Ok(DeliveryDisposition::Conflict),
            Some(DurableMessageIdentity::Unverifiable) => Ok(DeliveryDisposition::Unverifiable),
        }
    }

    async fn durable_message_identity(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> Result<Option<DurableMessageIdentity>, ThreadAssemblyError> {
        // Resolve the callback before awaiting: the registry lock only guards the synchronous state
        // read, so no `MutexGuard` is ever held across the durable query (this may run on a spawned
        // `Send` future).
        let lookup = self
            .0
            .state()
            .observation
            .as_ref()
            .and_then(|observation| observation.2.clone());
        let Some(lookup) = lookup else {
            return Ok(None);
        };
        lookup(thread_id.to_owned(), message_id.to_owned()).await
    }
    pub(crate) async fn notify_parent(
        &self,
        id: &str,
        message: pl_core::thread::inbox::ThreadMessage,
        wake: bool,
    ) -> Result<(), ThreadAssemblyError> {
        let target = {
            let state = self.0.state();
            if state.closing {
                return Ok(());
            }
            Some(
                state
                    .entries
                    .get(id)
                    .ok_or_else(|| ThreadAssemblyError::Identity(id.into()))?,
            )
            .filter(|entry| {
                entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
            })
            .map(|entry| (entry.thread.clone(), entry.execution))
        };
        let Some((thread, execution)) = target else {
            return Ok(());
        };
        // A terminal event's child write sequence is its identity. Its report includes observation-
        // time fields, so regenerating it after a restart need not reproduce the first accepted
        // body. The parent's durable receipt owns that body; a repeated event only wakes its still
        // pending message. Ordinary authored messages retain strict body-digest adjudication.
        match self.durable_message_identity(id, &message.id).await? {
            Some(DurableMessageIdentity::Proven { sequence, .. }) => {
                if wake {
                    thread
                        .wake_accepted_message(&message.id, sequence, execution)
                        .await?;
                }
                return Ok(());
            }
            // Older history proves acceptance without a receipt. Never admit a second copy.
            Some(DurableMessageIdentity::Unverifiable) => return Ok(()),
            None => {}
        }
        // The committed inbox already owns this immutable terminal watermark. In particular,
        // replay must not resend a migrated, previously consumed notification with a new body.
        let snapshot = thread.snapshot();
        let message = match snapshot
            .inbox
            .iter()
            .find(|record| record.message.id == message.id)
        {
            Some(record) if record.sequence <= snapshot.consumed_messages => return Ok(()),
            Some(record) => record.message.clone(),
            None => message,
        };
        let result = if wake {
            thread.send_message_and_resume(message, execution).await
        } else {
            // History projection can repair an inbox but cannot start a model call.
            thread.send_message(message).await
        };
        match result {
            Ok(_) => Ok(()),
            Err(ThreadError::Closed) if thread.snapshot().lifecycle != ThreadLifecycle::Open => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}
