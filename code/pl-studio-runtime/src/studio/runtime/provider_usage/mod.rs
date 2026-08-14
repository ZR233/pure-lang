use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{ObservedStateMeta, ObservedStatePhase, StateOperation};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::config::StudioConfig;
use crate::{ProviderUsageRecord, ProviderUsageState, StudioStore};

const CACHE_KEY: &str = "observed:providerUsage:v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageStateSnapshot {
    pub meta: ObservedStateMeta,
    pub config_fingerprint: String,
    pub usages: Vec<ProviderUsageRecord>,
}

#[derive(Clone)]
pub struct ProviderUsageRuntime {
    store: StudioStore,
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<ProviderUsageStateSnapshot>>,
    events: crate::StudioProductEventRuntime,
}

impl ProviderUsageRuntime {
    pub fn new(store: StudioStore, events: crate::StudioProductEventRuntime) -> Self {
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
        let running = ProviderUsageStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Running {
                    operation: StateOperation::Check,
                    operation_id: format!("provider-usage-check-{}", previous.meta.revision + 1),
                },
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            ..previous
        };
        self.publish(running.clone()).await;
        let usages = crate::provider_usage_records(config).await;
        let checked_at = unix_seconds();
        let failures = usages
            .iter()
            .filter_map(|usage| match &usage.state {
                ProviderUsageState::Failed(message) => {
                    Some(format!("{}: {message}", usage.provider_id))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            let failed = ProviderUsageStateSnapshot {
                meta: ObservedStateMeta {
                    revision: running.meta.revision.saturating_add(1),
                    phase: ObservedStatePhase::Failed {
                        operation: StateOperation::Check,
                        error: pl_protocol::StateError {
                            code: "providerUsageCheckFailed".to_string(),
                            message: failures.join("; "),
                            retryable: true,
                        },
                    },
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    stale: true,
                },
                config_fingerprint: running.config_fingerprint,
                usages: running.usages,
            };
            self.store
                .save_setting(CACHE_KEY, &serde_json::to_string(&failed)?)
                .await?;
            self.publish(failed).await;
            anyhow::bail!("provider usage check failed: {}", failures.join("; "));
        }
        let next = ProviderUsageStateSnapshot {
            meta: ObservedStateMeta {
                revision: running.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: checked_at,
                last_checked_at: Some(checked_at),
                stale: false,
            },
            config_fingerprint: config_fingerprint(config)?,
            usages,
        };
        if let Err(error) = self
            .store
            .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
            .await
        {
            let failed = ProviderUsageStateSnapshot {
                meta: ObservedStateMeta {
                    revision: running.meta.revision.saturating_add(1),
                    phase: ObservedStatePhase::Failed {
                        operation: StateOperation::Check,
                        error: pl_protocol::StateError {
                            code: "providerUsageCacheFailed".to_string(),
                            message: error.to_string(),
                            retryable: true,
                        },
                    },
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    stale: true,
                },
                config_fingerprint: running.config_fingerprint,
                usages: running.usages,
            };
            self.publish(failed).await;
            return Err(error);
        }
        self.publish(next.clone()).await;
        Ok(next)
    }

    /// Provider desired config 改变时保留 payload，并 authoritative 删除已移除 provider。
    pub async fn apply_config(&self, config: &StudioConfig) -> Result<ProviderUsageStateSnapshot> {
        let fingerprint = config_fingerprint(config)?;
        let provider_ids = config
            .models
            .providers
            .keys()
            .map(ToString::to_string)
            .collect::<std::collections::BTreeSet<_>>();
        let previous = self.read().await;
        let mut next = previous.clone();
        next.usages
            .retain(|usage| provider_ids.contains(&usage.provider_id));
        if next.config_fingerprint != fingerprint || next.usages != previous.usages {
            next.meta.revision = next.meta.revision.saturating_add(1);
            next.meta.updated_at = unix_seconds();
            next.meta.stale = next.config_fingerprint != fingerprint;
            self.store
                .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
                .await?;
            self.publish(next.clone()).await;
        }
        Ok(next)
    }

    async fn publish(&self, snapshot: ProviderUsageStateSnapshot) {
        *self.state.write().await = snapshot.clone();
        self.events.emit_provider_usage_state(snapshot);
    }
}

fn empty_state() -> ProviderUsageStateSnapshot {
    ProviderUsageStateSnapshot {
        meta: ObservedStateMeta::uninitialized(unix_seconds()),
        config_fingerprint: String::new(),
        usages: Vec::new(),
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

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
