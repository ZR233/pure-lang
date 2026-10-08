//! 产品订阅的 typed topic、作用域校验与首帧基线/订阅帧契约。
//!
//! 消费者按页面职责订阅一个或多个 topic；通道只分发 owner 已发布的同一 canonical
//! 事实，不持有业务投影。订阅先登记接收者，再读取该 topic 的当前基线，之后只交付
//! 该领域通知（见 design/18 §18.2.1）。

use serde::{Deserialize, Serialize};

use crate::{
    PersistenceStateSnapshot, ProviderUsageStateSnapshot, StudioAgentDirectoryState,
    StudioAgentProfilesStateSnapshot, StudioLspStateSnapshot, StudioMcpStateSnapshot,
    StudioModelPerformanceSnapshot, StudioPersistenceQueueStateSnapshot, StudioProductEventKind,
    StudioProjectDirectoryState, StudioRecoveryStateSnapshot, StudioSessionCostsState,
    StudioSettingsStateSnapshot, StudioSkillsStateSnapshot, StudioThreadDirectoryPage,
    StudioUpdateStateSnapshot,
};

/// 一个产品订阅 topic；带 payload 的变体表达作用域，不存在无作用域组合形式。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "scope",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StudioProductTopic {
    ProjectDirectory,
    ThreadDirectory,
    AgentDirectory,
    Settings,
    Recovery,
    Mcp,
    Lsp,
    Skills { project_id: String },
    ThreadModeCatalog,
    ProviderUsage,
    ModelPerformance,
    SessionCosts { root_thread_id: String },
    Updater,
    Persistence,
    PersistenceQueue,
    AgentProfiles,
}

/// typed topic 请求的唯一失败形态：带作用域 topic 的身份为空。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("topic `{topic}` requires a non-empty `{field}` scope")]
pub struct StudioProductTopicScopeError {
    pub topic: &'static str,
    pub field: &'static str,
}

impl StudioProductTopic {
    /// 校验作用域身份；空/空白 scope 是协议错误，不静默落到全局。
    pub fn validate(&self) -> Result<(), StudioProductTopicScopeError> {
        match self {
            Self::Skills { project_id } if project_id.trim().is_empty() => {
                Err(scope_error("skills", "projectId"))
            }
            Self::SessionCosts { root_thread_id } if root_thread_id.trim().is_empty() => {
                Err(scope_error("sessionCosts", "rootThreadId"))
            }
            _ => Ok(()),
        }
    }

    /// 该事件所属的 topic；事件只在对应 topic 通道内分发。
    pub fn of_kind(kind: &StudioProductEventKind) -> Self {
        match kind {
            StudioProductEventKind::ProjectDirectoryChanged(_) => Self::ProjectDirectory,
            StudioProductEventKind::ThreadDirectoryChanged(_) => Self::ThreadDirectory,
            StudioProductEventKind::AgentDirectoryChanged(_) => Self::AgentDirectory,
            StudioProductEventKind::SettingsStateChanged(_) => Self::Settings,
            StudioProductEventKind::RecoveryStateChanged(_) => Self::Recovery,
            StudioProductEventKind::McpStateChanged(_) => Self::Mcp,
            StudioProductEventKind::LspStateChanged(_) => Self::Lsp,
            StudioProductEventKind::SkillsStateChanged(state) => Self::Skills {
                project_id: state.project_id.clone(),
            },
            StudioProductEventKind::ThreadModeCatalogChanged(_) => Self::ThreadModeCatalog,
            StudioProductEventKind::ProviderUsageStateChanged(_) => Self::ProviderUsage,
            StudioProductEventKind::ModelPerformanceStateChanged(_) => Self::ModelPerformance,
            StudioProductEventKind::SessionCostsChanged(state) => Self::SessionCosts {
                root_thread_id: state.root_thread_id.clone(),
            },
            StudioProductEventKind::UpdaterStateChanged(_) => Self::Updater,
            StudioProductEventKind::PersistenceStateChanged(_) => Self::Persistence,
            StudioProductEventKind::PersistenceQueueStateChanged(_) => Self::PersistenceQueue,
            StudioProductEventKind::AgentProfilesStateChanged(_) => Self::AgentProfiles,
        }
    }
}

fn scope_error(topic: &'static str, field: &'static str) -> StudioProductTopicScopeError {
    StudioProductTopicScopeError { topic, field }
}

/// 订阅首帧：该 topic 当前的 canonical 基线。
///
/// `revision` 是从 payload 提取的领域 revision，消费者只按它合并基线与后续事件；
/// Thread directory 基线是分页页（含 cursor 与 revision），不能用 delta 冒充。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioProductBaseline {
    pub topic: StudioProductTopic,
    pub revision: u64,
    pub state: StudioProductBaselineState,
}

/// 单个 topic 的 canonical 基线 payload。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StudioProductBaselineState {
    ProjectDirectory(Box<StudioProjectDirectoryState>),
    ThreadDirectory(Box<StudioThreadDirectoryPage>),
    AgentDirectory(Box<StudioAgentDirectoryState>),
    Settings(Box<StudioSettingsStateSnapshot>),
    Recovery(Box<StudioRecoveryStateSnapshot>),
    Mcp(Box<StudioMcpStateSnapshot>),
    Lsp(Box<StudioLspStateSnapshot>),
    Skills(Box<StudioSkillsStateSnapshot>),
    ThreadModeCatalog(Box<pl_protocol::ThreadModeCatalogSnapshot>),
    ProviderUsage(Box<ProviderUsageStateSnapshot>),
    ModelPerformance(Box<StudioModelPerformanceSnapshot>),
    SessionCosts(Box<StudioSessionCostsState>),
    Updater(Box<StudioUpdateStateSnapshot>),
    Persistence(Box<PersistenceStateSnapshot>),
    PersistenceQueue(Box<StudioPersistenceQueueStateSnapshot>),
    AgentProfiles(Box<StudioAgentProfilesStateSnapshot>),
}

impl StudioProductBaselineState {
    /// 该领域当前水位；由各 payload 的 canonical revision 提取。
    pub fn revision(&self) -> u64 {
        match self {
            Self::ProjectDirectory(state) => revision_of(&state.state),
            Self::ThreadDirectory(state) => revision_of(&state.state),
            Self::AgentDirectory(state) => revision_of(&state.state),
            Self::Settings(state) => revision_of(&state.state),
            Self::Recovery(state) => revision_of(&state.state),
            Self::Mcp(state) => revision_of(&state.state),
            Self::Lsp(state) => revision_of(&state.state),
            Self::Skills(state) => revision_of(&state.state),
            Self::ThreadModeCatalog(state) => state.revision,
            Self::ProviderUsage(state) => revision_of(&state.state),
            Self::ModelPerformance(state) => state.revision,
            Self::SessionCosts(state) => state.revision,
            Self::Updater(state) => state.revision(),
            Self::Persistence(state) => state.revision,
            Self::PersistenceQueue(state) => state.revision,
            Self::AgentProfiles(state) => revision_of(&state.state),
        }
    }
}

/// typed topic 订阅交付的帧。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StudioProductFrame {
    /// 首帧：当前领域基线，明确 topic 与领域 revision。
    Baseline(Box<StudioProductBaseline>),
    /// 后续 owner 事实；payload 携带领域 revision，旧事实被消费者拒绝。
    Event(Box<crate::StudioProductEventEnvelope>),
    /// 无法证明该 topic 增量连续：只重读该 topic 基线，无 durable replay。
    Lagged {
        topic: StudioProductTopic,
        dropped: u64,
    },
}

fn revision_of<T>(resource: &pl_protocol::ObservedResource<T>) -> u64 {
    resource.revision()
}
