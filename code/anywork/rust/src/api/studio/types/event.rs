use serde::{Deserialize, Serialize};

use super::{
    BridgeAgentDirectoryState, BridgeLspStateSnapshot, BridgeMcpStateSnapshot,
    BridgeModelCatalogStateSnapshot, BridgeModelPerformanceSnapshot,
    BridgePersistenceStateSnapshot, BridgeProjectDirectoryState, BridgeProviderUsageStateSnapshot,
    BridgeRecoveryStateSnapshot, BridgeSettingsConfigStateSnapshot, BridgeSkillsStateSnapshot,
    BridgeThread, BridgeThreadModeCatalogSnapshot, BridgeUpdaterStateSnapshot,
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
    AgentDirectoryChanged(BridgeAgentDirectoryState),
    SettingsConfigStateChanged(Box<BridgeSettingsConfigStateSnapshot>),
    ModelCatalogStateChanged(Box<BridgeModelCatalogStateSnapshot>),
    RecoveryStateChanged(BridgeRecoveryStateSnapshot),
    McpStateChanged(BridgeMcpStateSnapshot),
    LspStateChanged(BridgeLspStateSnapshot),
    SkillsStateChanged(BridgeSkillsStateSnapshot),
    ThreadModeCatalogChanged(BridgeThreadModeCatalogSnapshot),
    ProviderUsageStateChanged(BridgeProviderUsageStateSnapshot),
    ModelPerformanceStateChanged(BridgeModelPerformanceSnapshot),
    SessionCostsChanged(Box<super::response::BridgeSessionCostsState>),
    UpdaterStateChanged(BridgeUpdaterStateSnapshot),
    PersistenceStateChanged(BridgePersistenceStateSnapshot),
    PersistenceQueueStateChanged(Box<super::response::BridgePersistenceQueueStateSnapshot>),
    AgentProfilesStateChanged(Box<super::response::BridgeAgentProfilesStateSnapshot>),
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
    StoppingAgents,
    StoppingMcp,
    StoppingLsp,
    Stopped,
}
