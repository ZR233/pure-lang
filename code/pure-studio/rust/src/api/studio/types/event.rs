use serde::{Deserialize, Serialize};

use super::runtime::BridgeObservedStateMeta;
use super::{
    BridgeAgentDirectoryState, BridgeLspStateSnapshot, BridgeMcpStateSnapshot,
    BridgeProjectDirectoryState, BridgeProviderUsageStateSnapshot, BridgeRecoveryStateSnapshot,
    BridgeSettingsStateSnapshot, BridgeSkillsStateSnapshot, BridgeTaskDirectoryState, BridgeThread,
    BridgeUpdaterStateSnapshot,
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
    Stale {
        lagged_events: u64,
    },
}

/// Thread directory 增量事件 payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThreadDirectoryDelta {
    pub meta: BridgeObservedStateMeta,
    pub upserted: Vec<BridgeThread>,
    pub removed: Vec<String>,
}

/// 关机阶段枚举（与 Rust `StudioShutdownPhase` 一一对应）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeShutdownPhase {
    StoppingSubscriptions,
    CancellingTurns,
    FlushingPersistence,
    SuspendingTasks,
    StoppingMcp,
    StoppingLsp,
    Stopped,
}

/// 一次关机进度的进度事件；`flushingPersistence` 完成事件携带 `pendingCommits: 0`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeShutdownProgress {
    pub phase: BridgeShutdownPhase,
    pub pending_commits: u64,
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
