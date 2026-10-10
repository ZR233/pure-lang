//! Owns startup-once and coalesced per-instance discovery tasks through shutdown.
use super::StudioRuntime;
use crate::config::ProviderId;
use anyhow::Result;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, watch};

#[derive(Clone, Default)]
pub(super) struct ModelCatalogTasks(Arc<Mutex<Tasks>>);
#[derive(Default)]
struct Tasks {
    started: bool,
    closing: bool,
    active: BTreeMap<ProviderId, ProbeTask>,
}
struct ProbeTask {
    generation: u64,
    done: watch::Receiver<Option<std::result::Result<(), String>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl StudioRuntime {
    pub(in crate::studio::runtime) async fn start_model_catalog_probes(&self) -> Result<()> {
        let mut tasks = self.model_catalog_tasks.0.lock().await;
        if tasks.started || tasks.closing {
            return Ok(());
        }
        tasks.started = true;
        let snapshot = self.config_runtime.read()?;
        for (id, provider) in snapshot.config.models.providers {
            if provider.supports_model_discovery() {
                self.spawn_catalog_probe(&mut tasks, id).await?;
            }
        }
        Ok(())
    }

    async fn spawn_catalog_probe(
        &self,
        tasks: &mut Tasks,
        id: ProviderId,
    ) -> Result<(
        u64,
        watch::Receiver<Option<std::result::Result<(), String>>>,
    )> {
        if let Some(task) = tasks.active.get(&id)
            && task.done.borrow().is_none()
        {
            return Ok((task.generation, task.done.clone()));
        }
        // Completion is published only after all network and blocking commits have ended.
        // A task never takes this registry lock, so joining its completed tail cannot deadlock.
        if let Some(task) = tasks.active.remove(&id) {
            task.handle.await?;
        }
        let generation = self.config_runtime.model_catalog_generation(&id)?;
        let (done, receiver) = watch::channel(None);
        let runtime = self.clone();
        let provider = id.clone();
        let handle = tokio::spawn(async move {
            let result = runtime.run_catalog_probe(provider).await;
            if let Err(error) = &result {
                tracing::error!(error = %error, "model catalog owner failed");
            }
            done.send_replace(Some(result.map_err(|error| error.to_string())));
        });
        tasks.active.insert(
            id,
            ProbeTask {
                generation,
                done: receiver.clone(),
                handle,
            },
        );
        Ok((generation, receiver))
    }

    async fn run_catalog_probe(&self, provider: ProviderId) -> Result<()> {
        let owner = self.config_runtime.clone();
        let probe = tokio::task::spawn_blocking(move || owner.begin_model_catalog_probe(provider))
            .await??;
        let Some(probe) = probe else {
            return Ok(());
        };
        let result = tokio::select! {
            biased;
            _ = probe.cancellation.cancelled() => return Ok(()),
            result = probe.query.execute(probe.previous.as_ref()) => result,
        };
        let owner = self.config_runtime.clone();
        // Do not abort this task: blocking commit owns atomic replacement and must be joined.
        tokio::task::spawn_blocking(move || owner.finish_model_catalog_probe(probe, result))
            .await??;
        Ok(())
    }

    /// Coalesces with an active startup/manual query for this instance. Failures are typed in the
    /// returned canonical provider status; desired config/revision is never modified.
    pub async fn refresh_model_catalog(
        &self,
        request: pl_protocol::studio::RefreshModelCatalogRequest,
    ) -> Result<pl_protocol::studio::ModelCatalogSnapshot> {
        let id = ProviderId::new(request.provider_id)?;
        loop {
            let (generation, mut done) = {
                let mut tasks = self.model_catalog_tasks.0.lock().await;
                anyhow::ensure!(!tasks.closing, "model catalog owner is closing");
                self.spawn_catalog_probe(&mut tasks, id.clone()).await?
            };
            loop {
                if let Some(result) = done.borrow_and_update().clone() {
                    result.map_err(anyhow::Error::msg)?;
                    break;
                }
                done.changed()
                    .await
                    .map_err(|_| anyhow::anyhow!("model catalog task ended without a result"))?;
            }
            // Address/credential changes cancel the old generation. A manual refresh must
            // observe the current instance rather than return that cancelled query as success.
            if self.config_runtime.model_catalog_generation(&id)? == generation {
                let settings = self.config_runtime.read()?;
                let catalog = self.config_runtime.read_catalog()?;
                return super::settings_api::model_catalog_snapshot(&settings, &catalog);
            }
        }
    }

    pub(in crate::studio::runtime) async fn stop_model_catalog_probes(&self) -> Result<()> {
        let handles = {
            let mut tasks = self.model_catalog_tasks.0.lock().await;
            tasks.closing = true;
            std::mem::take(&mut tasks.active)
        };
        let owner = self.config_runtime.clone();
        let close = tokio::task::spawn_blocking(move || owner.close_model_catalogs()).await;
        let mut failure = None;
        for task in handles.into_values() {
            if let Err(error) = task.handle.await {
                failure.get_or_insert_with(|| anyhow::Error::new(error));
            } else if let Some(Err(error)) = task.done.borrow().as_ref() {
                failure.get_or_insert_with(|| anyhow::anyhow!(error.clone()));
            }
        }
        close??;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
}
