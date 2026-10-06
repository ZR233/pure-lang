use anyhow::Result;

use super::super::StudioRuntime;
use super::super::{ProviderUsageStateSnapshot, StudioUpdateStateSnapshot};

impl StudioRuntime {
    pub async fn read_provider_usage_state(&self) -> ProviderUsageStateSnapshot {
        self.provider_usage.read().await
    }

    pub async fn check_provider_usage(&self) -> Result<ProviderUsageStateSnapshot> {
        let config = self.config_runtime.read()?.config;
        self.provider_usage.check(&config).await
    }

    pub async fn apply_provider_config(
        &self,
        config: &crate::StudioConfig,
    ) -> Result<ProviderUsageStateSnapshot> {
        self.provider_usage.apply_config(config).await
    }

    pub async fn read_update_state(&self) -> StudioUpdateStateSnapshot {
        self.updater.read().await
    }

    pub async fn check_studio_update(&self) -> Result<StudioUpdateStateSnapshot> {
        self.updater.check().await
    }

    /// Resolves the exact verified update selected by the desktop host.
    pub async fn verified_studio_update(
        &self,
        expected_revision: u64,
        version: &str,
    ) -> Result<crate::StudioUpdate> {
        self.updater
            .verified_update(expected_revision, version)
            .await
    }

    /// Downloads and installs a verified desktop update after the host's final shutdown check.
    pub async fn install_studio_update_after<F, Fut>(
        &self,
        update: crate::StudioUpdate,
        progress: tokio::sync::mpsc::UnboundedSender<StudioUpdateStateSnapshot>,
        cancellation: crate::StudioUpdateCancellation,
        before_launch: F,
    ) -> Result<(), crate::StudioUpdateError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), crate::StudioUpdateError>>,
    {
        let _command = self.updater.lock_install(&update).await?;
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let state_runtime = self.updater.clone();
        let state_update = update.clone();
        let state_cancellation = cancellation.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let forward = tokio::spawn(async move {
            let mut ready_tx = Some(ready_tx);
            while let Some(event) = event_rx.recv().await {
                let snapshot = match state_runtime
                    .apply_install_event(&state_update, &event)
                    .await
                {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        let _ = state_cancellation.cancel();
                        if let Some(ready) = ready_tx.take() {
                            let _ = ready.send(Err(projection_error(&error)));
                        }
                        return Err(error);
                    }
                };
                if matches!(event, crate::StudioUpdateEvent::Verifying)
                    && let Some(ready) = ready_tx.take()
                {
                    let _ = ready.send(Ok(()));
                }
                let _ = progress.send(snapshot);
            }
            Ok::<_, anyhow::Error>(())
        });
        let result = self
            .updater
            .updater()
            .install_after(update, event_tx, cancellation, || async {
                // All preceding download events and Verifying must be durable
                // before the host is allowed to stop services or launch a binary.
                ready_rx.await.map_err(|error| {
                    crate::StudioUpdateError::new(
                        crate::StudioUpdateErrorCode::Io,
                        format!("updater state projection ended before handoff: {error}"),
                    )
                })??;
                before_launch().await
            })
            .await;
        let forward_result = forward.await.map_err(|error| {
            crate::StudioUpdateError::new(
                crate::StudioUpdateErrorCode::InstallerLaunchFailed,
                format!("updater state projection task failed: {error}"),
            )
        })?;
        forward_result.map_err(|error| projection_error(&error))?;
        result
    }

    /// Restarts the desktop process only after an update has safely stopped it.
    /// The caller must exit the old GUI after this handoff succeeds.
    pub async fn restart_after_failed_update(&self) -> Result<bool> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        if !self.runtime_snapshot().await?.state.is_stopped() {
            return Ok(false);
        }
        let state = self.updater.read().await;
        anyhow::ensure!(
            matches!(
                state,
                StudioUpdateStateSnapshot::InstallFailed(_)
                    | StudioUpdateStateSnapshot::Verifying(_)
            ),
            "no failed update handoff to recover"
        );
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| anyhow::anyhow!("application executable has no directory"))?;
        let mut command = std::process::Command::new(&executable);
        command.current_dir(directory);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        pl_remote_helper::process::configure_handoff_std_command(&mut command);
        command.spawn()?;
        Ok(true)
    }

    pub async fn read_lsp_state(&self) -> crate::StudioLspStateSnapshot {
        self.external_runtimes.lsp_state.read().await
    }
}

fn projection_error(error: &anyhow::Error) -> crate::StudioUpdateError {
    crate::StudioUpdateError::new(
        crate::StudioUpdateErrorCode::Io,
        format!("updater state transition failed: {error:#}"),
    )
}
