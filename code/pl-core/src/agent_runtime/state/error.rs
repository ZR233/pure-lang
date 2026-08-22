use crate::agent_runtime::{ThreadId, TurnId};

use super::AgentState;

/// 非泛型 handle 使用的 runtime 错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentRuntimeError {
    #[error("agent not found: {0}")]
    NotFound(ThreadId),
    #[error("agent already exists: {0}")]
    AlreadyExists(ThreadId),
    #[error("agent {0} is not active: {1:?}")]
    NotActive(ThreadId, AgentState),
    #[error("agent has no active turn: {0}")]
    NoActiveTurn(ThreadId),
    #[error("active turn mismatch: expected {expected}, got {actual}")]
    TurnMismatch { expected: TurnId, actual: TurnId },
    #[error("agent {agent_id} canonical thread mismatch: expected {expected}, got {actual}")]
    ThreadMismatch {
        agent_id: ThreadId,
        expected: ThreadId,
        actual: ThreadId,
    },
    #[error("invalid agent input: {0}")]
    InvalidInput(String),
    #[error("agent repository failed: {0}")]
    Repository(String),
    #[error("agent revision conflict: expected {expected:?}, actual {actual:?}")]
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("agent lifecycle failed: {0}")]
    Lifecycle(String),
    #[error("thread events failed: {0}")]
    ThreadEvents(String),
    #[error("agent runtime channel closed")]
    ChannelClosed,
    #[error("agent wait timed out")]
    TimedOut,
}

pub type AgentRuntimeResult<T> = std::result::Result<T, AgentRuntimeError>;
