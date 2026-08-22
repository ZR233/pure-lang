use anyhow::Result;

use crate::config::ConfigRuntimeSnapshot;
use crate::studio::{StudioRecoveryIssue, StudioRuntimeSnapshot};

use super::super::StudioRuntime;

impl StudioRuntime {
    pub(crate) fn publish_settings_state(&self, settings: ConfigRuntimeSnapshot) -> Result<()> {
        let canonical = super::super::settings_api::settings_snapshot(settings)?;
        self.agent_facility.product_events.emit_settings_state(
            crate::StudioSettingsStateSnapshot {
                state: pl_protocol::ObservedResource::ready(
                    canonical.revision,
                    canonical.updated_at,
                    canonical.settings,
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
        snapshot.active_turns = self.derive_active_turns().await?;
        Ok(snapshot)
    }
}
