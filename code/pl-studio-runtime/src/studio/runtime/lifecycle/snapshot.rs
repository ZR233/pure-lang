use anyhow::Result;

use crate::config::{ConfigRuntimeSnapshot, ModelCatalogRuntimeSnapshot};
use crate::studio::{StudioRecoveryIssue, StudioRuntimeSnapshot};

use super::super::StudioRuntime;

impl StudioRuntime {
    pub(crate) fn publish_settings_state(&self, settings: ConfigRuntimeSnapshot) -> Result<()> {
        let config = super::super::settings_api::settings_config_snapshot(&settings)?;
        let catalog_state = self.config_runtime.read_catalog()?;
        let catalog =
            super::super::settings_api::model_catalog_snapshot(&settings, &catalog_state)?;
        self.settings_updates.send_replace(settings);
        self.agent_facility
            .product_events
            .emit_settings_config_state(crate::StudioSettingsConfigStateSnapshot {
                state: pl_protocol::ObservedResource::ready(
                    config.revision,
                    config.updated_at,
                    config,
                ),
            });
        self.agent_facility.product_events.emit_model_catalog_state(
            crate::StudioModelCatalogStateSnapshot {
                state: pl_protocol::ObservedResource::ready(
                    catalog.revision,
                    catalog.updated_at,
                    catalog,
                ),
            },
        );
        Ok(())
    }

    pub(in crate::studio::runtime) fn publish_model_catalog_state(
        &self,
        catalog_state: ModelCatalogRuntimeSnapshot,
    ) -> Result<()> {
        let settings = self.config_runtime.read()?;
        let catalog =
            super::super::settings_api::model_catalog_snapshot(&settings, &catalog_state)?;
        self.agent_facility.product_events.emit_model_catalog_state(
            crate::StudioModelCatalogStateSnapshot {
                state: pl_protocol::ObservedResource::ready(
                    catalog.revision,
                    catalog.updated_at,
                    catalog,
                ),
            },
        );
        Ok(())
    }

    /// 返回当前所有恢复问题的快照。
    ///
    /// 恢复问题由独立的 [`StudioRecoveryRegistry`] 持有，不混入 runtime 快照，
    /// 避免与生命周期转换竞争同一把锁。
    pub fn recovery_issues(&self) -> Vec<StudioRecoveryIssue> {
        self.recovery.snapshot()
    }

    pub async fn runtime_snapshot(&self) -> Result<StudioRuntimeSnapshot> {
        let mut snapshot = self.runtime_state.snapshot();
        snapshot.startup_recovery = self.startup_recovery.clone();
        snapshot.active_turns = self.derive_active_turns().await?;
        Ok(snapshot)
    }
}
