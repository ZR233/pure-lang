//! 数据库↔枚举标签映射。
//!
//! 这里的函数是 [`pl_protocol::LabeledEnum`] 在 store 层的薄封装：标签字符串已经
//! 在协议枚举上定义一次（`impl LabeledEnum`），本模块只处理那些无法直接走 trait 的
//! 聚合类型（如 `&ThreadActorState → 状态字符串`、`&ThreadItemContent → 类型字符串`、
//! `MailboxPresentation` 等非协议枚举）。
//!
//! 标签字符串是数据库列值的稳定标识，新增枚举变体时必须同步更新对应映射，
//! 否则 `cargo check` 会在穷尽 match 处报错。

use pl_core::{
    AgentActivityState, AgentLifecycleState, AgentTurnOutcome, MailboxPresentation,
    ThreadActorState, TurnOutcomeKind,
};
use pl_protocol::{
    InteractionKind, InteractionStatus, LabeledEnum, ThreadItemContent, ThreadItemStatus,
    ThreadMode, ThreadStatus, TurnPhase, TurnState,
};

use crate::PureError;

use super::store_error;

/// 把 agent 生命周期与活动状态投影成 Thread 表的 `status` 列值。
pub(super) fn thread_status_label(state: &ThreadActorState) -> &'static str {
    match state.snapshot.lifecycle {
        AgentLifecycleState::Closing | AgentLifecycleState::Closed => "closed",
        AgentLifecycleState::Faulted => "failed",
        AgentLifecycleState::Active => match state.snapshot.activity {
            AgentActivityState::Idle => "idle",
            AgentActivityState::Queued
            | AgentActivityState::Running
            | AgentActivityState::Cancelling => "running",
            AgentActivityState::WaitingTool | AgentActivityState::WaitingInteraction => "waiting",
        },
    }
}

/// 从 Thread 表 `status` 列恢复 [`ThreadStatus`]。
pub(super) fn thread_status_from_label(label: &str) -> Result<ThreadStatus, PureError> {
    ThreadStatus::from_label(label).map_err(map_label_error)
}

/// 把 agent 当前活动状态映射成活动 turn 的 `phase` 列值。
pub(super) fn activity_phase(activity: AgentActivityState) -> &'static str {
    match activity {
        AgentActivityState::WaitingTool => "runningTool",
        AgentActivityState::WaitingInteraction => "waitingInteraction",
        AgentActivityState::Queued => "preparing",
        AgentActivityState::Running | AgentActivityState::Cancelling | AgentActivityState::Idle => {
            "responding"
        }
    }
}

/// 从活动 turn 的 `phase` 列值恢复 [`TurnPhase`]。
pub(super) fn turn_phase_from_label(label: &str) -> Result<TurnPhase, PureError> {
    TurnPhase::from_label(label).map_err(map_label_error)
}

/// 把 [`TurnState`] 拆成 turn 表的 `status`、`phase`、`reason` 三列。
pub(super) fn turn_state_columns(
    state: &TurnState,
) -> (&'static str, Option<&'static str>, Option<&str>) {
    match state {
        TurnState::Queued => ("queued", None, None),
        TurnState::InProgress { phase } => ("inProgress", Some(phase.label()), None),
        TurnState::Completed => ("completed", None, None),
        TurnState::Failed { reason } => ("failed", None, Some(reason.as_str())),
        TurnState::Interrupted { reason } => ("interrupted", None, Some(reason.as_str())),
    }
}

/// 把 [`AgentTurnOutcome`] 拆成 turn 表的 `status`、`reason` 两列。
pub(super) fn outcome_columns(outcome: &AgentTurnOutcome) -> (&'static str, Option<&str>) {
    match outcome.kind {
        TurnOutcomeKind::Completed => ("completed", outcome.reason.as_deref()),
        TurnOutcomeKind::Failed => ("failed", outcome.reason.as_deref()),
        TurnOutcomeKind::Cancelled => ("interrupted", outcome.reason.as_deref()),
        TurnOutcomeKind::BudgetLimited => (
            "interrupted",
            outcome.reason.as_deref().or(Some("budgetLimited")),
        ),
    }
}

/// 把 [`ThreadItemContent`] 映射成 item 表的 `item_kind` 列值。
pub(super) fn item_kind_label(content: &ThreadItemContent) -> &'static str {
    match content {
        ThreadItemContent::UserMessage { .. } => "userMessage",
        ThreadItemContent::AgentMessage { .. } => "agentMessage",
        ThreadItemContent::Reasoning { .. } => "reasoning",
        ThreadItemContent::Plan { .. } => "plan",
        ThreadItemContent::ToolCall { .. } => "toolCall",
        ThreadItemContent::File { .. } => "file",
        ThreadItemContent::ContextCompaction { .. } => "contextCompaction",
    }
}

/// 把 [`ThreadItemStatus`] 映射成 item 表的 `status` 列值。
pub(super) fn item_status_label(status: ThreadItemStatus) -> &'static str {
    status.label()
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

/// 把 [`InteractionKind`] 映射成 interaction 表的 `kind` 列值。
pub(super) fn interaction_kind_label(value: InteractionKind) -> &'static str {
    value.label()
}

/// 把 [`InteractionStatus`] 映射成 interaction 表的 `status` 列值。
pub(super) fn interaction_status_label(value: InteractionStatus) -> &'static str {
    value.label()
}

/// 从 thread 表的 `status` 列值恢复 [`AgentLifecycleState`]。
pub(super) fn lifecycle_from_status(status: &str) -> Result<AgentLifecycleState, PureError> {
    match status {
        "closed" => Ok(AgentLifecycleState::Closed),
        "failed" => Ok(AgentLifecycleState::Faulted),
        "idle" | "running" | "waiting" | "completed" => Ok(AgentLifecycleState::Active),
        other => Err(store_error(format!("unknown Thread status {other}"))),
    }
}

/// 从 thread 表的 `mode` 列值恢复 [`ThreadMode`]。
pub(super) fn thread_mode_from_label(label: &str) -> Result<ThreadMode, PureError> {
    ThreadMode::from_label(label).map_err(map_label_error)
}

fn map_label_error(error: pl_protocol::UnknownLabelError) -> PureError {
    store_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_state_columns_covers_every_variant() {
        for state in [
            TurnState::Queued,
            TurnState::InProgress {
                phase: TurnPhase::Thinking,
            },
            TurnState::Completed,
            TurnState::Failed {
                reason: String::from("x"),
            },
            TurnState::Interrupted {
                reason: String::from("y"),
            },
        ] {
            let (status, phase, reason) = turn_state_columns(&state);
            assert!(!status.is_empty());
            if matches!(state, TurnState::InProgress { .. }) {
                assert!(phase.is_some());
            }
            if matches!(
                state,
                TurnState::Failed { .. } | TurnState::Interrupted { .. }
            ) {
                assert!(reason.is_some());
            }
        }
    }
}
