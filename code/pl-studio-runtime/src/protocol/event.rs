use pl_protocol::{ObservedStateMeta, Thread};

use crate::config::ConfigRuntimeSnapshot;
use crate::{
    ProjectRecord, ProviderUsageStateSnapshot, SkillsStateSnapshot, StudioAgentDirectoryEntry,
    StudioLspHealth, StudioMcpHealth, StudioRecoveryIssue, StudioTaskRuntime,
    StudioUpdateStateSnapshot,
};

/// Studio 产品级事件信封。
///
/// `sequence` 只检测 transport lag；消费者判断新旧必须使用 payload 自带的领域 revision。
#[derive(Debug, Clone, PartialEq)]
pub struct StudioProductEventEnvelope {
    pub event_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioProductEventKind,
}

/// Studio 全局产品事件。每个变体都携带可直接替换的完整领域 snapshot。
#[derive(Debug, Clone, PartialEq)]
pub enum StudioProductEventKind {
    ProjectDirectoryChanged(StudioProjectDirectoryState),
    ThreadDirectoryChanged(StudioThreadDirectoryState),
    TaskDirectoryChanged(StudioTaskDirectoryState),
    AgentDirectoryChanged(StudioAgentDirectoryState),
    SettingsStateChanged(Box<StudioSettingsStateSnapshot>),
    RecoveryStateChanged(StudioRecoveryStateSnapshot),
    McpStateChanged(StudioMcpStateSnapshot),
    LspStateChanged(StudioLspStateSnapshot),
    SkillsStateChanged(SkillsStateSnapshot),
    ProviderUsageStateChanged(ProviderUsageStateSnapshot),
    UpdaterStateChanged(StudioUpdateStateSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioProjectDirectoryState {
    pub meta: ObservedStateMeta,
    pub projects: Vec<ProjectRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioThreadDirectoryState {
    pub meta: ObservedStateMeta,
    pub threads: Vec<Thread>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioTaskDirectoryState {
    pub meta: ObservedStateMeta,
    pub tasks: Vec<StudioTaskDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioTaskDirectoryEntry {
    pub root_thread_id: String,
    pub task: StudioTaskRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioAgentDirectoryState {
    pub meta: ObservedStateMeta,
    pub agents: Vec<StudioAgentDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioSettingsStateSnapshot {
    pub meta: ObservedStateMeta,
    pub settings: ConfigRuntimeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioRecoveryStateSnapshot {
    pub meta: ObservedStateMeta,
    pub issues: Vec<StudioRecoveryIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioMcpStateSnapshot {
    pub meta: ObservedStateMeta,
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: StudioMcpHealth,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioLspStateSnapshot {
    pub meta: ObservedStateMeta,
    pub health: StudioLspHealth,
}
