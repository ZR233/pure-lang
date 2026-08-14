use serde::{Deserialize, Serialize};

use super::{
    BridgeAgentDirectoryState, BridgeLspStateSnapshot, BridgeMcpStateSnapshot,
    BridgeProjectDirectoryState, BridgeProviderUsageStateSnapshot, BridgeRecoveryStateSnapshot,
    BridgeSettingsStateSnapshot, BridgeSkillsStateSnapshot, BridgeTaskDirectoryState,
    BridgeThreadDirectoryState, BridgeUpdaterStateSnapshot,
};

/// Flutter Bridge 的 Studio 产品事件信封。
///
/// `sequence` 只检测 transport lag；payload 中完整 snapshot 的领域 revision 决定替换顺序。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProductEventEnvelope {
    pub event_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeProductEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeProductEventPayload {
    ProjectDirectoryChanged(BridgeProjectDirectoryState),
    ThreadDirectoryChanged(BridgeThreadDirectoryState),
    TaskDirectoryChanged(BridgeTaskDirectoryState),
    AgentDirectoryChanged(BridgeAgentDirectoryState),
    SettingsStateChanged(Box<BridgeSettingsStateSnapshot>),
    RecoveryStateChanged(BridgeRecoveryStateSnapshot),
    McpStateChanged(BridgeMcpStateSnapshot),
    LspStateChanged(BridgeLspStateSnapshot),
    SkillsStateChanged(BridgeSkillsStateSnapshot),
    ProviderUsageStateChanged(BridgeProviderUsageStateSnapshot),
    UpdaterStateChanged(BridgeUpdaterStateSnapshot),
    Stale { lagged_events: u64 },
}

impl BridgeProductEventEnvelope {
    pub fn stale(lagged_events: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            event_id: format!("bridge-product-stale-{:x}", now.as_nanos()),
            sequence: 0,
            created_at: now.as_secs() as i64,
            payload: BridgeProductEventPayload::Stale { lagged_events },
        }
    }
}
