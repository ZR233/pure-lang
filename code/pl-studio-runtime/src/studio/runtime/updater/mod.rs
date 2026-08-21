use std::sync::Arc;

use anyhow::{Context, Result};
use pl_protocol::{ObservedStateMeta, ObservedStatePhase, StateOperation};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::studio::ids::unix_seconds;
use crate::{StudioStore, StudioUpdate, StudioUpdateCheck, StudioUpdater};

const CACHE_KEY: &str = "observed:studioUpdate:v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioUpdateStateSnapshot {
    pub meta: ObservedStateMeta,
    pub update: Option<StudioUpdate>,
}

#[derive(Clone)]
pub(crate) struct StudioUpdateRuntime {
    store: StudioStore,
    updater: StudioUpdater,
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<StudioUpdateStateSnapshot>>,
    events: crate::ProductEventBus,
}

impl StudioUpdateRuntime {
    pub(crate) fn new(store: StudioStore, events: crate::ProductEventBus) -> Result<Self> {
        Ok(Self {
            store,
            updater: StudioUpdater::new_default()?,
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(StudioUpdateStateSnapshot {
                meta: ObservedStateMeta::uninitialized(unix_seconds()),
                update: None,
            })),
            events,
        })
    }

    pub(crate) async fn load_cache(&self) -> Result<()> {
        let Some(value) = self.store.load_setting(CACHE_KEY).await? else {
            return Ok(());
        };
        *self.state.write().await = serde_json::from_str(&value)?;
        Ok(())
    }

    /// 只读已验证 last-known update，不访问网络。
    pub(crate) async fn read(&self) -> StudioUpdateStateSnapshot {
        self.state.read().await.clone()
    }

    /// 使用编译时应用版本检查更新并持久化已验证结果。
    pub(crate) async fn check(&self) -> Result<StudioUpdateStateSnapshot> {
        let _command = self.command_lock.lock().await;
        let previous = self.read().await;
        let running = StudioUpdateStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Running {
                    operation: StateOperation::Check,
                    operation_id: format!("studio-update-check-{}", previous.meta.revision + 1),
                },
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            update: previous.update,
        };
        self.publish(running.clone()).await;
        let result = self.updater.check(env!("CARGO_PKG_VERSION")).await;
        let checked_at = unix_seconds();
        let next = match result {
            Ok(StudioUpdateCheck::UpToDate) => StudioUpdateStateSnapshot {
                meta: ObservedStateMeta {
                    revision: running.meta.revision.saturating_add(1),
                    phase: ObservedStatePhase::Ready,
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    stale: false,
                },
                update: None,
            },
            Ok(StudioUpdateCheck::Available(update)) => StudioUpdateStateSnapshot {
                meta: ObservedStateMeta {
                    revision: running.meta.revision.saturating_add(1),
                    phase: ObservedStatePhase::Ready,
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    stale: false,
                },
                update: Some(update),
            },
            Err(error) => {
                let failed = StudioUpdateStateSnapshot {
                    meta: ObservedStateMeta {
                        revision: running.meta.revision.saturating_add(1),
                        phase: ObservedStatePhase::Failed {
                            operation: StateOperation::Check,
                            error: pl_protocol::StateError {
                                code: error.code().as_str().to_string(),
                                message: error.to_string(),
                                retryable: matches!(
                                    error.code(),
                                    crate::StudioUpdateErrorCode::Network
                                        | crate::StudioUpdateErrorCode::Io
                                ),
                            },
                        },
                        updated_at: checked_at,
                        last_checked_at: Some(checked_at),
                        stale: true,
                    },
                    update: running.update,
                };
                self.store
                    .save_setting(CACHE_KEY, &serde_json::to_string(&failed)?)
                    .await?;
                self.publish(failed).await;
                return Err(error.into());
            }
        };
        self.store
            .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
            .await?;
        self.publish(next.clone()).await;
        Ok(next)
    }

    pub(crate) async fn verified_update(
        &self,
        expected_revision: u64,
        version: &str,
    ) -> Result<StudioUpdate> {
        let state = self.read().await;
        anyhow::ensure!(
            state.meta.revision == expected_revision,
            "update revision conflict: expected {expected_revision}, actual {}",
            state.meta.revision
        );
        anyhow::ensure!(
            matches!(state.meta.phase, ObservedStatePhase::Ready) && !state.meta.stale,
            "cached update is not in a verified ready state"
        );
        state
            .update
            .filter(|update| update.version == version)
            .context("requested update is not the cached verified update")
    }

    pub(crate) fn updater(&self) -> StudioUpdater {
        self.updater.clone()
    }

    async fn publish(&self, snapshot: StudioUpdateStateSnapshot) {
        *self.state.write().await = snapshot.clone();
        self.events.emit_updater_state(snapshot);
    }
}
