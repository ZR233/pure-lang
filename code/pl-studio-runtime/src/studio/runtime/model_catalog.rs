//! Owns startup-once per-instance discovery tasks through shutdown.
use super::StudioRuntime;
use crate::config::ProviderId;
use anyhow::Result;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub(super) struct ModelCatalogTasks(Arc<Mutex<Tasks>>);
#[derive(Default)]
struct Tasks {
    started: bool,
    closing: bool,
    active: BTreeMap<ProviderId, ProbeTask>,
}
struct ProbeTask {
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl StudioRuntime {
    pub(in crate::studio::runtime) async fn start_model_catalog_probes(&self) -> Result<()> {
        let mut tasks = self.model_catalog_tasks.0.lock().await;
        if tasks.started || tasks.closing {
            return Ok(());
        }
        tasks.started = true;
        let snapshot = self.config_runtime.read()?;
        for (id, provider) in &snapshot.canonical_config().models.providers {
            if provider.discovery_profile()?.is_some() {
                self.spawn_catalog_probe(&mut tasks, id.clone());
            }
        }
        Ok(())
    }

    fn spawn_catalog_probe(&self, tasks: &mut Tasks, id: ProviderId) {
        let runtime = self.clone();
        let provider = id.clone();
        let handle = tokio::spawn(async move { runtime.run_catalog_probe(provider).await });
        tasks.active.insert(id, ProbeTask { handle });
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
            match task.handle.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(error) => {
                    failure.get_or_insert_with(|| anyhow::Error::new(error));
                }
            }
        }
        close??;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
}
