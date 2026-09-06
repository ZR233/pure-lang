//! Studio 异步持久化健康状态。

use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

/// 没有待落库事实，writer 可接受新工作。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyPersistence {
    pub pending_commits: u64,
}

/// writer 正常排空待落库事实。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlushingPersistence {
    pub pending_commits: u64,
    pub oldest_pending_revision: Option<u64>,
}

/// 连续快速重试失败，后台仍会自动重试。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DegradedPersistence {
    pub pending_commits: u64,
    pub oldest_pending_revision: Option<u64>,
    pub first_failed_at: i64,
    pub error: StateError,
}

/// SQLite 已恢复，writer 正在排空故障期间积压。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveringPersistence {
    pub pending_commits: u64,
    pub oldest_pending_revision: Option<u64>,
    pub first_failed_at: i64,
}

/// 修订冲突、数据库损坏或其他不能自动安全重试的错误。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockedPersistence {
    pub pending_commits: u64,
    pub oldest_pending_revision: Option<u64>,
    pub first_failed_at: i64,
    pub error: StateError,
}

/// 持久化 owner 的强类型状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum PersistenceState {
    Ready(ReadyPersistence),
    Flushing(FlushingPersistence),
    Degraded(DegradedPersistence),
    Recovering(RecoveringPersistence),
    Blocked(BlockedPersistence),
}

impl PersistenceState {
    pub fn pending_commits(&self) -> u64 {
        match self {
            Self::Ready(state) => state.pending_commits,
            Self::Flushing(state) => state.pending_commits,
            Self::Degraded(state) => state.pending_commits,
            Self::Recovering(state) => state.pending_commits,
            Self::Blocked(state) => state.pending_commits,
        }
    }
}

/// 对产品事件和 Bridge 发布的持久化快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistenceStateSnapshot {
    pub revision: u64,
    pub state: PersistenceState,
}

impl Default for PersistenceStateSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            state: PersistenceState::Ready(ReadyPersistence { pending_commits: 0 }),
        }
    }
}
