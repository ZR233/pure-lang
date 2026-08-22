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
        progress: tokio::sync::mpsc::UnboundedSender<crate::StudioUpdateEvent>,
        cancellation: crate::StudioUpdateCancellation,
        before_launch: F,
    ) -> Result<(), crate::StudioUpdateError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), crate::StudioUpdateError>>,
    {
        self.updater
            .updater()
            .install_after(update, progress, cancellation, before_launch)
            .await
    }

    pub async fn read_lsp_state(&self) -> crate::StudioLspStateSnapshot {
        self.external_runtimes.lsp_state.read().await
    }
}
