//! Product observations own no execution tasks and project a single core commit watermark.
use super::StudioRuntime;
use anyhow::Result;
use pl_core::thread::{ThreadHandle, ThreadSnapshot, ThreadSubscription, journal::ThreadCommit};
use std::{num::NonZeroUsize, sync::Arc};

/// Snapshot stream backed by the canonical owner or immutable retired-child history.
/// Dropping it stops only observation.
/// Intermediate watch updates may coalesce; every emitted snapshot is authoritative.
pub struct StudioThreadSubscription {
    runtime: StudioRuntime,
    thread_id: String,
    source: SubscriptionSource,
    _residency_pin: super::residency::ThreadResidencyPins,
}

enum SubscriptionSource {
    Live(Box<LiveSubscription>),
    Retired(Option<Box<pl_protocol::ThreadSnapshot>>),
}

struct LiveSubscription {
    handle: ThreadHandle,
    observations: ThreadSubscription,
    journal: Vec<Arc<ThreadCommit>>,
}

impl StudioThreadSubscription {
    /// Receives a product snapshot, propagating projection or storage errors.
    ///
    /// # Errors
    /// Returns malformed historical content or missing committed facts; no partial snapshot is emitted.
    pub async fn recv(&mut self) -> Result<Option<pl_protocol::ThreadSubscriptionUpdate>> {
        let live = match &mut self.source {
            SubscriptionSource::Live(live) => live,
            SubscriptionSource::Retired(snapshot) => {
                if let Some(snapshot) = snapshot.take() {
                    return Ok(Some(pl_protocol::ThreadSubscriptionUpdate::Snapshot {
                        snapshot,
                    }));
                }
                // Immutable history has no producer. The transport cancels/drops this wait
                // when the client leaves; ending it would trigger GUI reconnect loops.
                return std::future::pending().await;
            }
        };
        let Some(state) = live.observations.next().await else {
            return Ok(None);
        };
        while live.journal.last().map_or(0, |commit| commit.sequence) < state.commit_sequence {
            let after = live.journal.last().map_or(0, |commit| commit.sequence);
            let page = live
                .handle
                .journal_page(after, NonZeroUsize::new(256).expect("positive page limit"))
                .await?;
            anyhow::ensure!(
                !page.is_empty(),
                "Thread snapshot references missing commits"
            );
            live.journal.extend(page);
        }
        let thread = self.runtime.read_protocol_thread(&self.thread_id).await?;
        let mut snapshot =
            crate::studio::thread_projection::project_snapshot(thread, &state, &live.journal)?;
        self.runtime.annotate_model_route(&mut snapshot)?;
        self.runtime
            .index_timeline(&self.thread_id, &state, &snapshot.items)
            .await;
        Ok(Some(pl_protocol::ThreadSubscriptionUpdate::Snapshot {
            snapshot: Box::new(snapshot),
        }))
    }
}

impl StudioRuntime {
    /// Observes canonical facts; retired child history never activates an execution owner.
    pub async fn subscribe_thread(
        &self,
        request: pl_protocol::ThreadSubscriptionRequest,
    ) -> Result<StudioThreadSubscription> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let residency_pin = self.residency.pin_many([request.thread_id.clone()]);
        let thread = self.read_protocol_thread(&request.thread_id).await?;
        let source = if thread.parent_thread_id.is_some() && thread.role == "planner" {
            SubscriptionSource::Retired(Some(Box::new(
                self.thread_snapshot(&request.thread_id).await?,
            )))
        } else {
            let handle = self.ensure_thread_owner(&request.thread_id).await?;
            SubscriptionSource::Live(Box::new(LiveSubscription {
                observations: handle.subscribe(),
                handle,
                journal: Vec::new(),
            }))
        };
        Ok(StudioThreadSubscription {
            source,
            runtime: self.clone(),
            thread_id: request.thread_id,
            _residency_pin: residency_pin,
        })
    }

    /// Reads live or cold facts without executing a model, tool or recovery continuation.
    pub async fn thread_snapshot(&self, thread_id: &str) -> Result<pl_protocol::ThreadSnapshot> {
        let thread = self.read_protocol_thread(thread_id).await?;
        let (state, journal) = self.read_thread_facts(thread_id).await?;
        let mut snapshot =
            crate::studio::thread_projection::project_snapshot(thread, &state, &journal)?;
        self.annotate_model_route(&mut snapshot)?;
        Ok(snapshot)
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

    fn annotate_model_route(&self, snapshot: &mut pl_protocol::ThreadSnapshot) -> Result<()> {
        let Some(route) = snapshot
            .runtime
            .as_mut()
            .and_then(|runtime| runtime.model_route.as_mut())
        else {
            return Ok(());
        };
        let selector = pl_model::config::ModelRouteConfig {
            provider: pl_model::config::ProviderId::new(route.provider_id.clone())?,
            model: route.model.clone(),
            effort: route
                .effort
                .clone()
                .map(pl_model::config::ReasoningEffort::new),
        };
        let config = self.config_runtime.read()?.config;
        let role = if snapshot.thread.parent_thread_id.is_none() {
            crate::config::StudioRole::Planner.id()
        } else {
            pl_protocol::AgentRoleId::new(snapshot.thread.role.clone())?
        };
        let resolved = config
            .models
            .resolve_route(role, &selector)
            .and_then(|resolved| {
                if snapshot.thread.parent_thread_id.is_none() {
                    let mode = self.thread_modes.snapshot().mode(&snapshot.thread.mode);
                    crate::mode::validate_thread_mode_model(
                        mode.as_ref().map(|mode| mode.as_ref()),
                        &resolved.model,
                    )?;
                }
                Ok(resolved)
            });
        match resolved {
            Ok(_) => {
                route.available = true;
                route.unavailable_reason = None;
            }
            Err(error) => {
                route.available = false;
                route.unavailable_reason = Some(error.to_string());
            }
        }
        Ok(())
    }
}
