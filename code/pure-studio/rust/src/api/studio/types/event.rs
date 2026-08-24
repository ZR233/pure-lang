use serde::{Deserialize, Serialize};

use super::{
    BridgeAgentDirectoryState, BridgeLspStateSnapshot, BridgeMcpStateSnapshot,
    BridgePersistenceStateSnapshot, BridgeProjectDirectoryState, BridgeProviderUsageStateSnapshot,
    BridgeRecoveryStateSnapshot, BridgeSettingsStateSnapshot, BridgeSkillsStateSnapshot,
    BridgeTaskDirectoryState, BridgeThread, BridgeUpdaterStateSnapshot,
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
    /// Thread directory 增量：GUI 按身份合并进分页窗口，未加载条目的增量忽略。
    ThreadDirectoryChanged(BridgeThreadDirectoryDelta),
    TaskDirectoryChanged(BridgeTaskDirectoryState),
    AgentDirectoryChanged(BridgeAgentDirectoryState),
    SettingsStateChanged(Box<BridgeSettingsStateSnapshot>),
    RecoveryStateChanged(BridgeRecoveryStateSnapshot),
    McpStateChanged(BridgeMcpStateSnapshot),
    LspStateChanged(BridgeLspStateSnapshot),
    SkillsStateChanged(BridgeSkillsStateSnapshot),
    ProviderUsageStateChanged(BridgeProviderUsageStateSnapshot),
    UpdaterStateChanged(BridgeUpdaterStateSnapshot),
    PersistenceStateChanged(BridgePersistenceStateSnapshot),
    Stale {
        lagged_events: u64,
    },
}

/// Thread directory 增量事件 payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThreadDirectoryDelta {
    pub revision: u64,
    pub updated_at: i64,
    pub upserted: Vec<BridgeThread>,
    pub removed: Vec<String>,
}

/// 一次关机进度的精确阶段状态；只有持久化刷新阶段携带 pending commit 数。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeShutdownProgress {
    StoppingSubscriptions,
    CancellingTurns,
    FlushingPersistence { pending_commits: u64 },
    SuspendingTasks,
    StoppingMcp,
    StoppingLsp,
    Stopped,
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
