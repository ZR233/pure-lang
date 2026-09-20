//! Product observations own no execution tasks and project a single core commit watermark.
use super::StudioRuntime;
use anyhow::Result;
use pl_core::thread::{ThreadHandle, ThreadSnapshot, ThreadSubscription, journal::ThreadCommit};
use std::{num::NonZeroUsize, sync::Arc};

/// Bounded item budget for a cold Thread's authoritative first frame.
const COLD_FIRST_FRAME_ITEMS: usize = 100;

/// Where a subscription's authoritative frames come from.
enum SubscriptionSource {
    /// A resident owner: live notifications from the canonical Thread actor.
    Live {
        handle: ThreadHandle,
        observations: ThreadSubscription,
    },
    /// A cold Thread: exactly one bounded window read from the durable timeline index.
    Cold {
        frame: Option<Box<pl_protocol::ThreadSnapshot>>,
    },
}

/// Snapshot stream for one Thread. Dropping it stops only observation and its residency pin.
///
/// A resident owner is observed live through the canonical Thread actor; a cold Thread yields one
/// authoritative bounded window derived from the durable timeline index and then completes. Neither
/// path activates an owner, replays the whole journal or creates a model or tool
/// (design/17 §17.5, design/18 §18.3).
pub struct StudioThreadSubscription {
    runtime: StudioRuntime,
    thread_id: String,
    source: SubscriptionSource,
    /// 订阅 pin：guard 存活期间该线程不参与 LRU 淘汰；这是轻量 pin，不是 owner 激活。
    _residency_pin: super::residency::ThreadResidencyPins,
}

impl StudioThreadSubscription {
    /// Receives the next authoritative product frame, propagating projection or storage errors.
    ///
    /// Intermediate watch updates may coalesce; every emitted snapshot is authoritative. A cold
    /// subscription yields exactly one bounded index window, then completes.
    ///
    /// # Errors
    /// Returns malformed historical content, missing committed facts or an unreadable durable index;
    /// no partial snapshot is emitted.
    pub async fn recv(&mut self) -> Result<Option<pl_protocol::ThreadSubscriptionUpdate>> {
        let runtime = self.runtime.clone();
        let thread_id = self.thread_id.clone();
        match &mut self.source {
            SubscriptionSource::Cold { frame } => Ok(frame
                .take()
                .map(|snapshot| pl_protocol::ThreadSubscriptionUpdate::Snapshot { snapshot })),
            SubscriptionSource::Live {
                handle,
                observations,
            } => {
                let Some(state) = observations.next().await else {
                    return Ok(None);
                };
                // Read the canonical journal fresh in bounded keyset pages for this frame only. The
                // subscription owns no accumulating journal copy; the durable index is the cold
                // source (design/17 §17.5).
                let journal = owner_journal_through(handle, state.commit_sequence).await?;
                let thread = runtime.read_protocol_thread(&thread_id).await?;
                let snapshot =
                    crate::studio::thread_projection::project_snapshot(thread, &state, &journal)?;
                runtime
                    .index_timeline(&thread_id, &state, &snapshot.items)
                    .await;
                Ok(Some(pl_protocol::ThreadSubscriptionUpdate::Snapshot {
                    snapshot: Box::new(snapshot),
                }))
            }
        }
    }
}

/// Bounded keyset read of one owner's committed journal through `through_sequence`.
async fn owner_journal_through(
    handle: &ThreadHandle,
    through_sequence: u64,
) -> Result<Vec<Arc<ThreadCommit>>> {
    let mut journal = Vec::<Arc<ThreadCommit>>::new();
    while journal
        .last()
        .map_or(0, |commit: &Arc<ThreadCommit>| commit.sequence)
        < through_sequence
    {
        let after = journal.last().map_or(0, |commit| commit.sequence);
        let page = handle
            .journal_page(after, NonZeroUsize::new(256).expect("positive page limit"))
            .await?;
        anyhow::ensure!(
            !page.is_empty(),
            "Thread snapshot references missing commits"
        );
        journal.extend(page);
    }
    journal.retain(|commit| commit.sequence <= through_sequence);
    Ok(journal)
}

impl StudioRuntime {
    /// Observes a Thread without executing it.
    ///
    /// A Thread that already has a resident owner is observed through live notifications; a cold
    /// Thread yields exactly one bounded window read from the durable timeline index. A cold
    /// subscribe never activates an owner, never creates a model or tool and never replays the whole
    /// journal; a missing index returns the typed preparing/failure state instead of an empty page
    /// (design/17 §17.5, design/18 §18.3).
    ///
    /// # Errors
    /// Propagates a missing Thread or a cold Thread's typed index preparing/failure state.
    pub async fn subscribe_thread(
        &self,
        request: pl_protocol::ThreadSubscriptionRequest,
    ) -> Result<StudioThreadSubscription> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        // 订阅 pin：轻量 residency pin，不激活也不保留完整 owner（design/17 §17.5）。
        let residency_pin = self.residency.pin_many([request.thread_id.clone()]);
        let thread_id = request.thread_id;
        let source = match self.threads.thread(&thread_id) {
            // Only an already resident owner subscribes to live notifications.
            Some(handle) => SubscriptionSource::Live {
                observations: handle.subscribe(),
                handle,
            },
            None => SubscriptionSource::Cold {
                frame: Some(self.cold_subscription_frame(&thread_id).await?),
            },
        };
        Ok(StudioThreadSubscription {
            runtime: self.clone(),
            thread_id,
            source,
            _residency_pin: residency_pin,
        })
    }

    /// Builds a cold Thread's authoritative first frame from the durable timeline index.
    ///
    /// The window is served by the Studio timeline reader: small items decode inline while oversized
    /// items resolve through their versioned content reference in bounded chunk reads. The read is
    /// bounded by item and byte budgets, activates no owner and replays no whole journal; a missing
    /// index is surfaced as the typed preparing/failure result, never as an empty page.
    ///
    /// # Errors
    /// Propagates the typed preparing/failure state, an unknown Thread or a corrupt index row.
    async fn cold_subscription_frame(
        &self,
        thread_id: &str,
    ) -> Result<Box<pl_protocol::ThreadSnapshot>> {
        let page = self
            .list_timeline_items(
                thread_id,
                pl_protocol::TimelineQuery::Latest,
                COLD_FIRST_FRAME_ITEMS,
            )
            .await?;
        let thread = self.read_protocol_thread(thread_id).await?;
        let active_turn = page
            .turns
            .iter()
            .rev()
            .find(|meta| matches!(meta.turn.state, pl_protocol::TurnState::Running(_)))
            .map(|meta| meta.turn.clone());
        Ok(Box::new(pl_protocol::ThreadSnapshot {
            schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
            revision: page.watermark,
            thread,
            active_turn,
            items: page.items,
            interactions: Vec::new(),
            runtime: None,
        }))
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
