use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{ObservedResource, ObservedResourceCommand, StateError, StateOperation};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::config::StudioConfig;
use crate::studio::ids::unix_seconds;
use crate::{ProviderUsageRecord, ProviderUsageState, StudioStore};

const CACHE_KEY: &str = "observed:providerUsage:v2";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageStateSnapshot {
    pub state: ObservedResource<ProviderUsageStateData>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageStateData {
    pub config_fingerprint: String,
    pub usages: Vec<ProviderUsageRecord>,
}

#[derive(Clone)]
pub struct ProviderUsageRuntime {
    store: StudioStore,
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<ProviderUsageStateSnapshot>>,
    events: crate::ProductEventBus,
}

impl ProviderUsageRuntime {
    pub fn new(store: StudioStore, events: crate::ProductEventBus) -> Self {
        Self {
            store,
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(empty_state())),
            events,
        }
    }

    pub async fn load_cache(&self) -> Result<()> {
        let Some(value) = self.store.load_setting(CACHE_KEY).await? else {
            return Ok(());
        };
        *self.state.write().await = serde_json::from_str(&value)?;
        Ok(())
    }

    /// 只读 last-known payload，不访问 provider 网络。
    pub async fn read(&self) -> ProviderUsageStateSnapshot {
        self.state.read().await.clone()
    }

    pub async fn check(&self, config: &StudioConfig) -> Result<ProviderUsageStateSnapshot> {
        let _command = self.command_lock.lock().await;
        let previous = self.read().await;
        let revision = previous.state.revision();
        let operation_id = format!("provider-usage-check-{}", revision.saturating_add(1));
        let running = ProviderUsageStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Begin {
                    expected_revision: revision,
                    operation: StateOperation::Check,
                    operation_id: operation_id.clone(),
                    started_at: unix_seconds(),
                })
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .next_state,
        };
        self.publish(running.clone()).await;
        let prior_usages = previous
            .state
            .value()
            .map(|data| data.usages.as_slice())
            .unwrap_or_default();
        let usages = crate::provider_usage_records(config, prior_usages, &operation_id).await?;
        let checked_at = unix_seconds();
        let failures = usages
            .iter()
            .filter_map(|usage| match usage.state() {
                ProviderUsageState::Failed(state) => Some(format!(
                    "{}: {}",
                    usage.provider_id(),
                    state.error().message
                )),
                ProviderUsageState::Unsupported(_)
                | ProviderUsageState::MissingCredential(_)
                | ProviderUsageState::Ready(_) => None,
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            let failed = ProviderUsageStateSnapshot {
                state: running
                    .state
                    .decide(ObservedResourceCommand::Fail {
                        expected_revision: running.state.revision(),
                        failed_at: checked_at,
                        error: StateError {
                            code: "providerUsageCheckFailed".to_string(),
                            message: failures.join("; "),
                            retryable: true,
                        },
                    })
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?
                    .next_state,
            };
            self.store
                .save_setting(CACHE_KEY, &serde_json::to_string(&failed)?)
                .await?;
            self.publish(failed).await;
            anyhow::bail!("provider usage check failed: {}", failures.join("; "));
        }
        let next = ProviderUsageStateSnapshot {
            state: running
                .state
                .decide(ObservedResourceCommand::Succeed {
                    expected_revision: running.state.revision(),
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    value: ProviderUsageStateData {
                        config_fingerprint: config_fingerprint(config)?,
                        usages,
                    },
                })
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .next_state,
        };
        if let Err(error) = self
            .store
            .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
            .await
        {
            let failed = ProviderUsageStateSnapshot {
                state: running
                    .state
                    .decide(ObservedResourceCommand::Fail {
                        expected_revision: running.state.revision(),
                        failed_at: checked_at,
                        error: StateError {
                            code: "providerUsageCacheFailed".to_string(),
                            message: error.to_string(),
                            retryable: true,
                        },
                    })
                    .map_err(|transition| anyhow::anyhow!(transition.to_string()))?
                    .next_state,
            };
            self.publish(failed).await;
            return Err(error);
        }
        self.publish(next.clone()).await;
        Ok(next)
    }

    /// Provider desired config 改变时保留 payload，并 authoritative 删除已移除 provider。
    pub async fn apply_config(&self, config: &StudioConfig) -> Result<ProviderUsageStateSnapshot> {
        let _command = self.command_lock.lock().await;
        let fingerprint = config_fingerprint(config)?;
        let provider_ids = config
            .models
            .providers
            .keys()
            .map(ToString::to_string)
            .collect::<std::collections::BTreeSet<_>>();
        let previous = self.read().await;
        let Some(mut data) = previous.state.value().cloned() else {
            return Ok(previous);
        };
        let previous_data = data.clone();
        data.usages
            .retain(|usage| provider_ids.contains(usage.provider_id()));
        if data.config_fingerprint != fingerprint || data != previous_data {
            let next = ProviderUsageStateSnapshot {
                state: previous
                    .state
                    .decide(ObservedResourceCommand::MarkStale {
                        expected_revision: previous.state.revision(),
                        stale_at: unix_seconds(),
                        value: data,
                    })
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?
                    .next_state,
            };
            self.store
                .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
                .await?;
            self.publish(next.clone()).await;
            return Ok(next);
        }
        Ok(previous)
    }

    async fn publish(&self, snapshot: ProviderUsageStateSnapshot) {
        *self.state.write().await = snapshot.clone();
        self.events.emit_provider_usage_state(snapshot);
    }
}

fn empty_state() -> ProviderUsageStateSnapshot {
    ProviderUsageStateSnapshot {
        state: ObservedResource::uninitialized(unix_seconds()),
    }
}

fn config_fingerprint(config: &StudioConfig) -> Result<String> {
    let mut redacted = config.clone();
    for provider in redacted.models.providers.values_mut() {
        provider.bearer_token = None;
    }
    let serialized = toml::to_string(&redacted)?;
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}
