use super::*;
use crate::config::model_catalog::{CatalogProbe, SuccessCache};
use pl_protocol::studio::{StudioModelCatalogError, StudioModelCatalogSource};

impl ConfigRuntime {
    pub(crate) fn model_catalog_generation(
        &self,
        provider: &ProviderId,
    ) -> ConfigRuntimeResult<u64> {
        let state = self.state.read().map_err(|_| config_runtime_poisoned())?;
        state
            .observations
            .get(provider)
            .map(|observation| observation.generation)
            .ok_or_else(|| {
                PureError::ConfigError("model catalog provider no longer exists".into()).into()
            })
    }
    pub(crate) fn subscribe_model_catalogs(
        &self,
    ) -> tokio::sync::broadcast::Receiver<CatalogChange> {
        self.catalog_updates.subscribe()
    }

    /// Called on a blocking worker. No network or asynchronous lock is held here.
    pub(crate) fn begin_model_catalog_probe(
        &self,
        provider_id: ProviderId,
    ) -> ConfigRuntimeResult<Option<CatalogProbe>> {
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        if state.closing {
            return Ok(None);
        }
        let provider = state
            .desired
            .models
            .providers
            .get(&provider_id)
            .ok_or_else(|| {
                PureError::ConfigError("model catalog provider no longer exists".into())
            })?;
        if !provider.supports_model_discovery() {
            return Ok(None);
        }
        let query = match pl_model::provider::discovery::ModelCatalogQuery::for_provider(provider) {
            Ok(query) => query,
            Err(error) => {
                let error = model_catalog::query_error(error)?;
                if let Some(observation) = state.observations.get_mut(&provider_id) {
                    observation.status.error = Some(error);
                    observation.status.checked_at = Some(unix_seconds());
                }
                self.publish_catalog_locked(&mut state, Vec::new());
                return Ok(None);
            }
        };
        let observation = state
            .observations
            .get_mut(&provider_id)
            .ok_or_else(|| PureError::ConfigError("model observation is missing".into()))?;
        if observation.status.probing {
            return Ok(None);
        }
        observation.status.probing = true;
        observation.status.error = None;
        let probe = CatalogProbe {
            provider: provider_id,
            generation: observation.generation,
            previous: observation.cache.as_ref().map(|cache| {
                pl_model::provider::discovery::ModelCatalogQueryCache {
                    identity: query.identity().to_owned(),
                    etag: cache.query_etag().map(str::to_owned),
                    models: cache.models.clone(),
                }
            }),
            query,
            cancellation: observation.cancellation.child_token(),
        };
        self.publish_catalog_locked(&mut state, Vec::new());
        Ok(Some(probe))
    }

    /// Atomic file replacement precedes publication. The command lock is also the closing gate.
    /// A spawned blocking commit must always be joined, never aborted/detached.
    pub(crate) fn finish_model_catalog_probe(
        &self,
        probe: CatalogProbe,
        result: std::result::Result<
            pl_model::provider::discovery::ModelCatalogQueryResult,
            pl_model::provider::discovery::ModelCatalogQueryError,
        >,
    ) -> ConfigRuntimeResult<()> {
        use pl_model::provider::discovery::ModelCatalogQueryResult;
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let state = self.state.read().map_err(|_| config_runtime_poisoned())?;
        if state.closing || probe.cancellation.is_cancelled() {
            return Ok(());
        }
        let Some(old) = state.observations.get(&probe.provider).cloned() else {
            return Ok(());
        };
        if old.generation != probe.generation
            || old.identity.as_deref() != Some(probe.query.identity())
        {
            return Ok(());
        }
        let cache = match result {
            Ok(ModelCatalogQueryResult::Updated {
                identity,
                etag,
                models,
            }) => {
                if identity != probe.query.identity() {
                    return Ok(());
                }
                Ok(SuccessCache::new(
                    &probe.provider,
                    &identity,
                    etag,
                    models,
                    unix_seconds(),
                ))
            }
            Ok(ModelCatalogQueryResult::NotModified { identity, etag }) => {
                if identity != probe.query.identity() {
                    return Ok(());
                }
                match &old.cache {
                    Some(cache) => Ok(cache.not_modified(etag)),
                    None => Err(StudioModelCatalogError::UnexpectedNotModified),
                }
            }
            Err(error) => Err(model_catalog::query_error(error)?),
        };
        let mut next_observations = state.observations.clone();
        let desired = state.desired.clone();
        let current_providers = state.snapshot.config.models.providers.clone();
        drop(state);
        let next = next_observations
            .get_mut(&probe.provider)
            .ok_or_else(|| PureError::ConfigError("model observation is missing".into()))?;
        next.status.probing = false;
        next.status.checked_at = Some(unix_seconds());
        let mut affected = Vec::new();
        let mut effective_config = None;
        match cache {
            Ok(cache) => {
                // Validate the complete proposed observation before touching the previous durable file.
                next.cache = Some(cache.clone());
                let effective = model_catalog::effective(&desired, &next_observations)?;
                effective.validate_declarations()?;
                match cache.persist(self.store.paths(), &probe.provider, probe.query.identity()) {
                    Ok(()) => {
                        if effective.models.providers.get(&probe.provider)
                            != current_providers.get(&probe.provider)
                        {
                            affected.push(probe.provider.clone());
                        }
                        effective_config = Some(effective);
                        let next = next_observations.get_mut(&probe.provider).ok_or_else(|| {
                            PureError::ConfigError("model observation is missing".into())
                        })?;
                        next.status.source = StudioModelCatalogSource::Online;
                        next.status.last_success_at = Some(cache.success_at);
                        next.status.checked_at = Some(cache.checked_at);
                        next.status.error = None;
                        next.status.cache_warning = None;
                    }
                    Err(_) => {
                        let next = next_observations.get_mut(&probe.provider).ok_or_else(|| {
                            PureError::ConfigError("model observation is missing".into())
                        })?;
                        next.cache = old.cache.clone();
                        next.status.error = Some(StudioModelCatalogError::CacheWrite);
                    }
                }
            }
            Err(error) => next.status.error = Some(error),
        }
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        if let Some(config) = effective_config {
            state.snapshot.config = config;
        }
        state.observations = next_observations;
        self.publish_catalog_locked(&mut state, affected);
        Ok(())
    }

    fn publish_catalog_locked(&self, state: &mut RuntimeState, affected: Vec<ProviderId>) {
        state.snapshot.model_catalog_revision =
            state.snapshot.model_catalog_revision.saturating_add(1);
        state.snapshot.updated_at = unix_seconds();
        state.snapshot.model_catalogs = statuses(&state.observations);
        let _ = self.catalog_updates.send(CatalogChange {
            snapshot: state.snapshot.clone(),
            affected,
        });
    }

    /// Serializes with commit, prevents new publication, and cancels outstanding network futures.
    pub(crate) fn close_model_catalogs(&self) -> ConfigRuntimeResult<()> {
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        state.closing = true;
        for observation in state.observations.values_mut() {
            observation.cancellation.cancel();
            if observation.status.probing {
                observation.status.probing = false;
                observation.status.error = Some(StudioModelCatalogError::Closing);
            }
        }
        state.snapshot.model_catalogs = statuses(&state.observations);
        Ok(())
    }
}
