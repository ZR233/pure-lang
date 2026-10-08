//! typed 产品 topic 的 FRB 镜像：订阅请求、首帧基线与流帧。

use serde::{Deserialize, Serialize};

use super::error::BridgeError;
use super::event::BridgeProductEventEnvelope;
use super::{
    BridgeAgentDirectoryState, BridgeAgentProfilesStateSnapshot, BridgeLspStateSnapshot,
    BridgeMcpStateSnapshot, BridgeModelPerformanceSnapshot, BridgePersistenceQueueStateSnapshot,
    BridgePersistenceStateSnapshot, BridgeProjectDirectoryState, BridgeProviderUsageStateSnapshot,
    BridgeRecoveryStateSnapshot, BridgeSessionCostsState, BridgeSettingsStateSnapshot,
    BridgeSkillsStateSnapshot, BridgeThreadDirectoryPage, BridgeThreadModeCatalogSnapshot,
    BridgeUpdaterStateSnapshot,
};

/// 产品订阅 topic；带 payload 的变体表达作用域，不存在无作用域组合形式。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BridgeProductTopic {
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

impl BridgeProductTopic {
    /// 校验作用域身份；空/空白 scope 直接拒绝，不落到全局。
    pub fn validate_scope(&self) -> Result<(), BridgeError> {
        let invalid = |message: &str| BridgeError::invalid_argument(message);
        match self {
            Self::Skills { project_id } if project_id.trim().is_empty() => {
                Err(invalid("Skills topic requires a non-empty projectId"))
            }
            Self::SessionCosts { root_thread_id } if root_thread_id.trim().is_empty() => Err(
                invalid("SessionCosts topic requires a non-empty rootThreadId"),
            ),
            _ => Ok(()),
        }
    }

    pub(crate) fn to_runtime(&self) -> pl_studio_runtime::StudioProductTopic {
        use pl_studio_runtime::StudioProductTopic;
        match self.clone() {
            Self::ProjectDirectory => StudioProductTopic::ProjectDirectory,
            Self::ThreadDirectory => StudioProductTopic::ThreadDirectory,
            Self::AgentDirectory => StudioProductTopic::AgentDirectory,
            Self::Settings => StudioProductTopic::Settings,
            Self::Recovery => StudioProductTopic::Recovery,
            Self::Mcp => StudioProductTopic::Mcp,
            Self::Lsp => StudioProductTopic::Lsp,
            Self::Skills { project_id } => StudioProductTopic::Skills { project_id },
            Self::ThreadModeCatalog => StudioProductTopic::ThreadModeCatalog,
            Self::ProviderUsage => StudioProductTopic::ProviderUsage,
            Self::ModelPerformance => StudioProductTopic::ModelPerformance,
            Self::SessionCosts { root_thread_id } => {
                StudioProductTopic::SessionCosts { root_thread_id }
            }
            Self::Updater => StudioProductTopic::Updater,
            Self::Persistence => StudioProductTopic::Persistence,
            Self::PersistenceQueue => StudioProductTopic::PersistenceQueue,
            Self::AgentProfiles => StudioProductTopic::AgentProfiles,
        }
    }
}

/// 单个 topic 的 canonical 基线 payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BridgeProductBaseline {
    ProjectDirectory(BridgeProjectDirectoryState),
    ThreadDirectory(BridgeThreadDirectoryPage),
    AgentDirectory(BridgeAgentDirectoryState),
    Settings(Box<BridgeSettingsStateSnapshot>),
    Recovery(BridgeRecoveryStateSnapshot),
    Mcp(BridgeMcpStateSnapshot),
    Lsp(BridgeLspStateSnapshot),
    Skills(BridgeSkillsStateSnapshot),
    ThreadModeCatalog(BridgeThreadModeCatalogSnapshot),
    ProviderUsage(BridgeProviderUsageStateSnapshot),
    ModelPerformance(BridgeModelPerformanceSnapshot),
    SessionCosts(Box<BridgeSessionCostsState>),
    Updater(BridgeUpdaterStateSnapshot),
    Persistence(BridgePersistenceStateSnapshot),
    PersistenceQueue(Box<BridgePersistenceQueueStateSnapshot>),
    AgentProfiles(Box<BridgeAgentProfilesStateSnapshot>),
}

/// typed topic 订阅交付的帧。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BridgeProductTopicStreamEnvelope {
    /// 首帧：当前领域基线，明确 topic 与领域 revision。
    Baseline {
        topic: BridgeProductTopic,
        revision: u64,
        state: Box<BridgeProductBaseline>,
    },
    /// 后续 owner 事实；payload 携带领域 revision，旧事实由消费者拒绝。
    Data {
        event: Box<BridgeProductEventEnvelope>,
    },
    /// 无法证明该 topic 增量连续：只重读该 topic 基线，无 durable replay。
    Lagged {
        topic: BridgeProductTopic,
        dropped: u64,
    },
    Failure {
        error: BridgeError,
    },
    Closed,
}
