//! 数据库↔枚举标签映射。
//!
//! 这里的函数是 [`pl_protocol::LabeledEnum`] 在 store 层的薄封装：标签字符串已经
//! 在协议枚举上定义一次（`impl LabeledEnum`），本模块只处理那些无法直接走 trait 的
//! 聚合类型（如 `&ThreadActorState → 状态字符串`、`&ThreadItemState → 类型字符串`、
//! `MailboxPresentation` 等非协议枚举）。
//!
//! 标签字符串是数据库列值的稳定标识，新增枚举变体时必须同步更新对应映射，
//! 否则 `cargo check` 会在穷尽 match 处报错。

use pl_core::{AgentState, MailboxPresentation};
use pl_protocol::{ThreadItemState, ThreadMode, ThreadStatus};

use crate::PureError;

use super::store_error;

/// 由 canonical Agent 状态穷尽投影 Thread 展示状态。
pub(crate) fn thread_status(state: &AgentState) -> ThreadStatus {
    match state {
        AgentState::Idle(_) => ThreadStatus::Idle,
        AgentState::Queued(_) => ThreadStatus::Queued,
        AgentState::Running(_) => ThreadStatus::Running,
        AgentState::WaitingTool(_) => ThreadStatus::WaitingTool,
        AgentState::WaitingInteraction(_) => ThreadStatus::WaitingInteraction,
        AgentState::Cancelling(_) => ThreadStatus::Cancelling,
        AgentState::Closing(_) => ThreadStatus::Closing,
        AgentState::Closed(_) => ThreadStatus::Closed,
        AgentState::Faulted(_) => ThreadStatus::Faulted,
    }
}

pub(super) fn agent_state_kind(state: &AgentState) -> &'static str {
    match state {
        AgentState::Idle(_) => "idle",
        AgentState::Queued(_) => "queued",
        AgentState::Running(_) => "running",
        AgentState::WaitingTool(_) => "waitingTool",
        AgentState::WaitingInteraction(_) => "waitingInteraction",
        AgentState::Cancelling(_) => "cancelling",
        AgentState::Closing(_) => "closing",
        AgentState::Closed(_) => "closed",
        AgentState::Faulted(_) => "faulted",
    }
}

/// 把 canonical [`ThreadItemState`] 映射成 item 表的类别索引值。
pub(super) fn item_kind_label(state: &ThreadItemState) -> &'static str {
    match state {
        ThreadItemState::Text(_) => "text",
        ThreadItemState::Thinking(_) => "thinking",
        ThreadItemState::Tool(_) => "tool",
        ThreadItemState::Agent(_) => "agent",
        ThreadItemState::Turn(_) => "turn",
        ThreadItemState::Inference(_) => "inference",
        ThreadItemState::Skill(_) => "skill",
        ThreadItemState::File(_) => "file",
        ThreadItemState::ContextCompaction(_) => "contextCompaction",
    }
}

/// 把 [`MailboxPresentation`] 映射成 thread_input 表的 `presentation` 列值。
pub(super) fn presentation_label(value: MailboxPresentation) -> &'static str {
    match value {
        MailboxPresentation::User => "user",
        MailboxPresentation::Hidden => "hidden",
    }
}

/// 从 thread_input 表的 `presentation` 列值恢复 [`MailboxPresentation`]。
pub(super) fn presentation_from_label(value: &str) -> Result<MailboxPresentation, PureError> {
    match value {
        "user" => Ok(MailboxPresentation::User),
        "hidden" => Ok(MailboxPresentation::Hidden),
        other => Err(store_error(format!("unknown input presentation {other}"))),
    }
}

/// 从 thread 表的 `mode` 列值恢复 [`ThreadMode`]。
pub(super) fn thread_mode_from_label(label: &str) -> Result<ThreadMode, PureError> {
    ThreadMode::from_label(label).map_err(map_label_error)
}

fn map_label_error(error: pl_protocol::UnknownLabelError) -> PureError {
    store_error(error.to_string())
}
