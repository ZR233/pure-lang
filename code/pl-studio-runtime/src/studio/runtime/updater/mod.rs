//! Canonical Studio updater lifecycle and its durable runtime owner.

mod state;

pub use state::*;

use std::sync::Arc;

use anyhow::{Context, Result};
use pl_protocol::StateError;
use tokio::sync::{Mutex, MutexGuard, RwLock};

use crate::studio::ids::unix_seconds;
use crate::updater::STUDIO_VERSION;
use crate::{
    StudioStore, StudioUpdate, StudioUpdateCheck, StudioUpdateErrorCode, StudioUpdateEvent,
    StudioUpdater,
};

const CACHE_KEY: &str = "studioUpdateState:v2";

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
            state: Arc::new(RwLock::new(StudioUpdateStateSnapshot::idle(unix_seconds()))),
            events,
        })
    }

    pub(crate) async fn load_cache(&self) -> Result<()> {
        let Some(value) = self.store.load_setting(CACHE_KEY).await? else {
            return Ok(());
        };
        *self.state.write().await = serde_json::from_str(&value)?;
        self.apply(StudioUpdateCommand::RecoverAfterRestart {
            expected_revision: self.read().await.revision(),
            updated_at: unix_seconds(),
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn read(&self) -> StudioUpdateStateSnapshot {
        self.state.read().await.clone()
    }

    pub(crate) async fn check(&self) -> Result<StudioUpdateStateSnapshot> {
        let _command = self.command_lock.try_lock().map_err(|_| {
            crate::StudioUpdateError::new(
                StudioUpdateErrorCode::InstallInProgress,
                "another update command is already running",
            )
        })?;
        let previous = self.read().await;
        let running = self
            .apply(StudioUpdateCommand::BeginCheck {
                expected_revision: previous.revision(),
                operation_id: format!("studio-update-check-{}", previous.revision() + 1),
                started_at: unix_seconds(),
            })
            .await?;
        let result = self.updater.check(STUDIO_VERSION.trim()).await;
        let checked_at = unix_seconds();
        let command = match result {
            Ok(StudioUpdateCheck::UpToDate) => StudioUpdateCommand::FinishUpToDate {
                expected_revision: running.revision(),
                checked_at,
            },
            Ok(StudioUpdateCheck::Available(update)) => StudioUpdateCommand::FinishAvailable {
                expected_revision: running.revision(),
                checked_at,
                update,
            },
            Err(error) => {
                tracing::warn!(code = error.code().as_str(), error = %error, "Studio update check failed");
                let command = StudioUpdateCommand::FailCheck {
                    expected_revision: running.revision(),
                    failed_at: checked_at,
                    error: state_error(&error),
                };
                return self.apply(command).await;
            }
        };
        self.apply(command).await
    }

    pub(crate) async fn verified_update(
        &self,
        expected_revision: u64,
        version: &str,
    ) -> Result<StudioUpdate> {
        let state = self.read().await;
        anyhow::ensure!(
            state.revision() == expected_revision,
            "update revision conflict: expected {expected_revision}, actual {}",
            state.revision()
        );
        let update = match &state {
            StudioUpdateStateSnapshot::Available(value) => value.update(),
            StudioUpdateStateSnapshot::InstallFailed(value) => value.update(),
            _ => anyhow::bail!("cached update is not available for installation"),
        };
        (update.version == version)
            .then(|| update.clone())
            .context("requested update is not the cached verified update")
    }

    pub(crate) async fn apply_install_event(
        &self,
        update: &StudioUpdate,
        event: &StudioUpdateEvent,
    ) -> Result<StudioUpdateStateSnapshot> {
        let current = self.read().await;
        let now = unix_seconds();
        let command = match event {
            StudioUpdateEvent::Started { total } => StudioUpdateCommand::BeginDownload {
                expected_revision: current.revision(),
                updated_at: now,
                update: update.clone(),
                total: *total,
            },
            StudioUpdateEvent::Progress { downloaded, total } => {
                StudioUpdateCommand::ReportDownload {
                    expected_revision: current.revision(),
                    updated_at: now,
                    downloaded: *downloaded,
                    total: *total,
                }
            }
            StudioUpdateEvent::Verifying => StudioUpdateCommand::BeginVerify {
                expected_revision: current.revision(),
                updated_at: now,
            },
            StudioUpdateEvent::InstallerLaunched => StudioUpdateCommand::MarkInstallerLaunched {
                expected_revision: current.revision(),
                launched_at: now,
            },
            StudioUpdateEvent::Failed { code, message } => StudioUpdateCommand::FailInstall {
                expected_revision: current.revision(),
                failed_at: now,
                error: StateError {
                    code: code.clone(),
                    message: message.clone(),
                    retryable: code == StudioUpdateErrorCode::Network.as_str()
                        || code == StudioUpdateErrorCode::Io.as_str()
                        || code == StudioUpdateErrorCode::RuntimeBusy.as_str()
                        || code == StudioUpdateErrorCode::Cancelled.as_str(),
                },
            },
        };
        self.apply(command).await
    }

    pub(crate) fn updater(&self) -> StudioUpdater {
        self.updater.clone()
    }

    pub(crate) async fn lock_install(
        &self,
        update: &StudioUpdate,
    ) -> Result<MutexGuard<'_, ()>, crate::StudioUpdateError> {
        let command = self.command_lock.try_lock().map_err(|_| {
            crate::StudioUpdateError::new(
                StudioUpdateErrorCode::InstallInProgress,
                "another update command is already running",
            )
        })?;
        let state = self.read().await;
        if !matches!(
            state,
            StudioUpdateStateSnapshot::Available(_) | StudioUpdateStateSnapshot::InstallFailed(_)
        ) || state.update() != Some(update)
        {
            return Err(crate::StudioUpdateError::new(
                StudioUpdateErrorCode::InvalidManifest,
                "selected update is no longer available for installation",
            ));
        }
        Ok(command)
    }

    async fn apply(&self, command: StudioUpdateCommand) -> Result<StudioUpdateStateSnapshot> {
        let current = self.read().await;
        let decision = current.decide(command)?;
        let next = decision.next_state;
        if !decision.changed {
            return Ok(next);
        }
        self.store
            .save_setting(CACHE_KEY, &serde_json::to_string(&next)?)
            .await?;
        *self.state.write().await = next.clone();
        self.events.emit_updater_state(next.clone());
        Ok(next)
    }
}

fn state_error(error: &crate::StudioUpdateError) -> StateError {
    StateError {
        code: error.code().as_str().to_string(),
        message: error.to_string(),
        retryable: matches!(
            error.code(),
            StudioUpdateErrorCode::Network | StudioUpdateErrorCode::Io
        ),
    }
}
