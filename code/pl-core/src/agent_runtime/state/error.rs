use std::fmt;

use crate::agent_runtime::{AgentId, ThreadId, TurnId};

use super::AgentLifecycleState;

/// 非泛型 handle 使用的 runtime 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeError {
    NotFound(AgentId),
    AlreadyExists(AgentId),
    NotActive(AgentId, AgentLifecycleState),
    NoActiveTurn(AgentId),
    TurnMismatch {
        expected: TurnId,
        actual: TurnId,
    },
    ThreadMismatch {
        agent_id: AgentId,
        expected: ThreadId,
        actual: ThreadId,
    },
    InvalidInput(String),
    Repository(String),
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    Lifecycle(String),
    ThreadEvents(String),
    ChannelClosed,
    TimedOut,
}

pub type AgentRuntimeResult<T> = std::result::Result<T, AgentRuntimeError>;

impl fmt::Display for AgentRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(formatter, "agent not found: {id}"),
            Self::AlreadyExists(id) => write!(formatter, "agent already exists: {id}"),
            Self::NotActive(id, state) => write!(formatter, "agent {id} is not active: {state:?}"),
            Self::NoActiveTurn(id) => write!(formatter, "agent has no active turn: {id}"),
            Self::TurnMismatch { expected, actual } => {
                write!(
                    formatter,
                    "active turn mismatch: expected {expected}, got {actual}"
                )
            }
            Self::ThreadMismatch {
                agent_id,
                expected,
                actual,
            } => write!(
                formatter,
                "agent {agent_id} canonical thread mismatch: expected {expected}, got {actual}"
            ),
            Self::InvalidInput(reason) => write!(formatter, "invalid agent input: {reason}"),
            Self::Repository(error) => write!(formatter, "agent repository failed: {error}"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "agent revision conflict: expected {expected:?}, actual {actual:?}"
            ),
            Self::Lifecycle(error) => write!(formatter, "agent lifecycle failed: {error}"),
            Self::ThreadEvents(error) => write!(formatter, "thread events failed: {error}"),
            Self::ChannelClosed => formatter.write_str("agent runtime channel closed"),
            Self::TimedOut => formatter.write_str("agent wait timed out"),
        }
    }
}

impl std::error::Error for AgentRuntimeError {}
