use pl_protocol::{ObservedResource, Thread};
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
    pub state: ObservedResource<StudioProjectDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProjectDirectoryData {
    pub projects: Vec<ProjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryState {
    pub state: ObservedResource<StudioThreadDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryData {
    pub threads: Vec<Thread>,
}

/// Thread directory 增量事件 payload：由常驻内存目录索引派生，不再携带全量列表。
///
/// `upserted` 按线程身份原位替换，`removed` 携带已归档/删除的 Thread id；
/// 未加载进分页窗口的增量由消费端忽略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryDelta {
    pub revision: u64,
    pub updated_at: i64,
    pub upserted: Vec<Thread>,
    pub removed: Vec<String>,
}

/// Thread directory 的 keyset 分页页（按 `updatedAt` 倒序、id 倒序）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryPage {
    pub state: ObservedResource<StudioThreadDirectoryPageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryPageData {
    pub threads: Vec<Thread>,
    /// `None` 表示没有更旧的页。
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDirectoryState {
    pub state: ObservedResource<StudioTaskDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDirectoryData {
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
    pub state: ObservedResource<StudioAgentDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryData {
    pub agents: Vec<StudioAgentDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSettingsStateSnapshot {
    pub state: ObservedResource<pl_protocol::studio::StudioSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryStateSnapshot {
    pub state: ObservedResource<Vec<StudioRecoveryIssue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpStateSnapshot {
    pub state: ObservedResource<StudioMcpStateData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpStateData {
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: StudioMcpHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspStateSnapshot {
    pub state: ObservedResource<StudioLspHealth>,
}

/// Transport-neutral published Skills state with an owned catalog payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSkillsStateSnapshot {
    pub project_id: String,
    pub state: ObservedResource<StudioSkillsStateData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSkillsStateData {
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: pl_core::skill::SkillCatalog,
}

impl From<SkillsStateSnapshot> for StudioSkillsStateSnapshot {
    fn from(value: SkillsStateSnapshot) -> Self {
        Self {
            project_id: value.project_id,
            state: value.state.map(|data| StudioSkillsStateData {
                config_fingerprint: data.config_fingerprint,
                catalog_revision: data.catalog_revision,
                catalog: data.catalog.as_ref().clone(),
            }),
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
