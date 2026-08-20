use pl_protocol::{ObservedStateMeta, Thread};
use serde::{Deserialize, Serialize};

use crate::{
    ProjectRecord, ProviderUsageStateSnapshot, SkillsStateSnapshot, StudioAgentDirectoryEntry,
    StudioLspHealth, StudioMcpHealth, StudioRecoveryIssue, StudioTaskRuntime,
    StudioUpdateStateSnapshot,
};

/// Studio 产品级事件信封。
///
/// `sequence` 只检测 transport lag；消费者判断新旧必须使用 payload 自带的领域 revision。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProductEventEnvelope {
    pub event_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioProductEventKind,
}

/// Studio 全局产品事件。除 `ThreadDirectoryChanged` 携带增量 payload 外，
/// 每个变体都携带可直接替换的完整领域 snapshot。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StudioProductEventKind {
    ProjectDirectoryChanged(StudioProjectDirectoryState),
    ThreadDirectoryChanged(StudioThreadDirectoryDelta),
    TaskDirectoryChanged(StudioTaskDirectoryState),
    AgentDirectoryChanged(StudioAgentDirectoryState),
    SettingsStateChanged(Box<StudioSettingsStateSnapshot>),
    RecoveryStateChanged(StudioRecoveryStateSnapshot),
    McpStateChanged(StudioMcpStateSnapshot),
    LspStateChanged(StudioLspStateSnapshot),
    SkillsStateChanged(StudioSkillsStateSnapshot),
    ProviderUsageStateChanged(ProviderUsageStateSnapshot),
    UpdaterStateChanged(StudioUpdateStateSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProjectDirectoryState {
    pub meta: ObservedStateMeta,
    pub projects: Vec<ProjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryState {
    pub meta: ObservedStateMeta,
    pub threads: Vec<Thread>,
}

/// Thread directory 增量事件 payload：由常驻内存目录索引派生，不再携带全量列表。
///
/// `upserted` 按线程身份原位替换，`removed` 携带已归档/删除的 Thread id；
/// 未加载进分页窗口的增量由消费端忽略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryDelta {
    pub meta: ObservedStateMeta,
    pub upserted: Vec<Thread>,
    pub removed: Vec<String>,
}

/// Thread directory 的 keyset 分页页（按 `updatedAt` 倒序、id 倒序）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryPage {
    pub meta: ObservedStateMeta,
    pub threads: Vec<Thread>,
    /// `None` 表示没有更旧的页。
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDirectoryState {
    pub meta: ObservedStateMeta,
    pub tasks: Vec<StudioTaskDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDirectoryEntry {
    pub root_thread_id: String,
    pub task: StudioTaskRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryState {
    pub meta: ObservedStateMeta,
    pub agents: Vec<StudioAgentDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSettingsStateSnapshot {
    pub meta: ObservedStateMeta,
    pub settings: pl_protocol::studio::StudioSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryStateSnapshot {
    pub meta: ObservedStateMeta,
    pub issues: Vec<StudioRecoveryIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpStateSnapshot {
    pub meta: ObservedStateMeta,
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: StudioMcpHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspStateSnapshot {
    pub meta: ObservedStateMeta,
    pub health: StudioLspHealth,
}

/// Studio 关机的固定阶段序列。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioShutdownPhase {
    StoppingSubscriptions,
    CancellingTurns,
    FlushingPersistence,
    SuspendingTasks,
    StoppingMcp,
    StoppingLsp,
    Stopped,
}

impl StudioShutdownPhase {
    /// 1-based 阶段序号；驱动验收按它断言顺序与完备性。
    pub fn index(self) -> u8 {
        match self {
            Self::StoppingSubscriptions => 1,
            Self::CancellingTurns => 2,
            Self::FlushingPersistence => 3,
            Self::SuspendingTasks => 4,
            Self::StoppingMcp => 5,
            Self::StoppingLsp => 6,
            Self::Stopped => 7,
        }
    }
}

/// 一次关机进度的进度事件。
///
/// `FlushingPersistence` 除进入事件外还发布 pending=0 的完成事件；
/// 并发 shutdown 共享同一次阶段序列。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioShutdownProgress {
    pub phase: StudioShutdownPhase,
    pub pending_commits: u64,
}

/// Transport-neutral published Skills state with an owned catalog payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSkillsStateSnapshot {
    pub meta: ObservedStateMeta,
    pub project_id: String,
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: pl_core::skill::SkillCatalog,
}

impl From<SkillsStateSnapshot> for StudioSkillsStateSnapshot {
    fn from(value: SkillsStateSnapshot) -> Self {
        Self {
            meta: value.meta,
            project_id: value.project_id,
            config_fingerprint: value.config_fingerprint,
            catalog_revision: value.catalog_revision,
            catalog: value.catalog.as_ref().clone(),
        }
    }
}

/// Complete Studio query snapshot shared by FRB and HTTP adapters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioStateSnapshot {
    pub runtime: crate::StudioRuntimeSnapshot,
    pub project_directory: StudioProjectDirectoryState,
    pub thread_directory: StudioThreadDirectoryPage,
    pub task_directory: StudioTaskDirectoryState,
    pub agent_directory: StudioAgentDirectoryState,
    pub settings: StudioSettingsStateSnapshot,
    pub recovery: StudioRecoveryStateSnapshot,
    pub mcp: StudioMcpStateSnapshot,
    pub lsp: StudioLspStateSnapshot,
    pub skills_by_project: Vec<StudioSkillsStateSnapshot>,
    pub provider_usage: ProviderUsageStateSnapshot,
    pub updater: StudioUpdateStateSnapshot,
}
