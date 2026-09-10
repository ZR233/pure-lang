//! Product observations own no execution tasks and project a single core commit watermark.
use super::StudioRuntime;
use anyhow::Result;
use pl_core::thread::{ThreadHandle, ThreadSnapshot, ThreadSubscription, journal::ThreadCommit};
use std::{num::NonZeroUsize, sync::Arc};

/// Snapshot stream backed by the canonical Thread owner. Dropping it stops only observation.
/// Intermediate watch updates may coalesce; every emitted snapshot is authoritative.
pub struct StudioThreadSubscription {
    runtime: StudioRuntime,
    thread_id: String,
    handle: ThreadHandle,
    observations: ThreadSubscription,
    journal: Vec<Arc<ThreadCommit>>,
    _residency_pin: super::residency::ThreadResidencyPins,
}

impl StudioThreadSubscription {
    /// Receives a product snapshot, propagating projection or storage errors.
    ///
    /// # Errors
    /// Returns malformed historical content or missing committed facts; no partial snapshot is emitted.
    pub async fn recv(&mut self) -> Result<Option<pl_protocol::ThreadSubscriptionUpdate>> {
        let Some(state) = self.observations.next().await else {
            return Ok(None);
        };
        while self.journal.last().map_or(0, |commit| commit.sequence) < state.commit_sequence {
            let after = self.journal.last().map_or(0, |commit| commit.sequence);
            let page = self
                .handle
                .journal_page(after, NonZeroUsize::new(256).expect("positive page limit"))
                .await?;
            anyhow::ensure!(
                !page.is_empty(),
                "Thread snapshot references missing commits"
            );
            self.journal.extend(page);
        }
        let thread = self.runtime.read_protocol_thread(&self.thread_id).await?;
        let snapshot =
            crate::studio::thread_projection::project_snapshot(thread, &state, &self.journal)?;
        Ok(Some(pl_protocol::ThreadSubscriptionUpdate::Snapshot {
            snapshot: Box::new(snapshot),
        }))
    }
}

impl StudioRuntime {
    /// Activates a Thread and observes its canonical facts through Studio's typed projection.
    pub async fn subscribe_thread(
        &self,
        request: pl_protocol::ThreadSubscriptionRequest,
    ) -> Result<StudioThreadSubscription> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let residency_pin = self.residency.pin_many([request.thread_id.clone()]);
        let handle = self.ensure_thread_owner(&request.thread_id).await?;
        Ok(StudioThreadSubscription {
            observations: handle.subscribe(),
            handle,
            runtime: self.clone(),
            thread_id: request.thread_id,
            journal: Vec::new(),
            _residency_pin: residency_pin,
        })
    }

    /// Reads live or cold facts without executing a model, tool or recovery continuation.
    pub async fn thread_snapshot(&self, thread_id: &str) -> Result<pl_protocol::ThreadSnapshot> {
        let thread = self.read_protocol_thread(thread_id).await?;
        let (state, journal) = self.read_thread_facts(thread_id).await?;
        Ok(crate::studio::thread_projection::project_snapshot(
            thread, &state, &journal,
        )?)
    }

    pub(in crate::studio) async fn read_thread_facts(
        &self,
        thread_id: &str,
    ) -> Result<(ThreadSnapshot, Vec<Arc<ThreadCommit>>)> {
        if let Some((_, handle)) = self
            .threads
            .observed_threads()
            .into_iter()
            .find(|(id, _)| id == thread_id)
        {
            // The owner publishes history before advertising this snapshot's watermark.
            let state = handle.snapshot();
            let mut journal = handle.journal().await?;
            journal.retain(|commit| commit.sequence <= state.commit_sequence);
            return Ok((state, journal));
        }
        self.read_owned_thread(thread_id).await?;
        let journal = self.store.sessions().read_thread_journal(thread_id).await?;
        let state = pl_core::thread::journal::replay(&journal)?;
        Ok((state, journal))
    }
}
