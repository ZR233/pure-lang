//! Agent 可观察生命周期的唯一协议 union。

mod cancelling;
mod closed;
mod closing;
mod faulted;
mod idle;
mod queued;
mod running;
mod waiting_interaction;
mod waiting_tool;

pub use cancelling::CancellingAgentState;
pub use closed::ClosedAgentState;
pub use closing::ClosingAgentState;
pub use faulted::{AgentFaultClassification, FaultedAgentState};
pub use idle::{AgentBudgetPause, IdleAgentState};
pub use queued::QueuedAgentState;
pub use running::RunningAgentState;
pub use waiting_interaction::WaitingInteractionAgentState;
pub use waiting_tool::WaitingToolAgentState;

use serde::{Deserialize, Serialize};

use crate::TurnId;

/// Agent 的唯一 canonical 生命周期状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum AgentState {
    Idle(IdleAgentState),
    Queued(QueuedAgentState),
    Running(RunningAgentState),
    WaitingTool(WaitingToolAgentState),
    WaitingInteraction(WaitingInteractionAgentState),
    Cancelling(CancellingAgentState),
    Closing(ClosingAgentState),
    Closed(ClosedAgentState),
    Faulted(FaultedAgentState),
}

impl AgentState {
    /// 创建 idle Agent 状态。
    pub fn idle() -> Self {
        Self::Idle(IdleAgentState::new())
    }

    /// 创建因 child Turn 预算耗尽而暂停的 idle 状态。
    pub fn budget_paused(pause: AgentBudgetPause) -> Self {
        Self::Idle(IdleAgentState::budget_paused(pause))
    }

    /// 返回当前 active、queued 或诊断 Turn。
    pub fn turn_id(&self) -> Option<&TurnId> {
        match self {
            Self::Queued(state) => Some(state.turn_id()),
            Self::Running(state) => Some(state.turn_id()),
            Self::WaitingTool(state) => Some(state.turn_id()),
            Self::WaitingInteraction(state) => Some(state.turn_id()),
            Self::Cancelling(state) => Some(state.turn_id()),
            Self::Faulted(state) => state.turn_id(),
            Self::Closing(state) => state.turn_id(),
            Self::Idle(_) | Self::Closed(_) => None,
        }
    }

    /// 返回状态是否仍接受执行命令。
    pub fn is_operational(&self) -> bool {
        matches!(
            self,
            Self::Idle(_)
                | Self::Queued(_)
                | Self::Running(_)
                | Self::WaitingTool(_)
                | Self::WaitingInteraction(_)
                | Self::Cancelling(_)
        )
    }

    /// 返回状态是否仍接受新工作。
    pub fn is_accepting_work(&self) -> bool {
        matches!(
            self,
            Self::Idle(_)
                | Self::Queued(_)
                | Self::Running(_)
                | Self::WaitingTool(_)
                | Self::WaitingInteraction(_)
        )
    }

    /// 返回 Agent 是否 idle。
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle(_))
    }

    /// 返回 Agent 是否正等待父 Agent 检查预算终态并显式续跑。
    pub fn is_budget_paused(&self) -> bool {
        matches!(self, Self::Idle(state) if state.budget_pause().is_some())
    }

    /// 返回 child 预算暂停快照。
    pub fn budget_pause(&self) -> Option<&AgentBudgetPause> {
        match self {
            Self::Idle(state) => state.budget_pause(),
            Self::Queued(_)
            | Self::Running(_)
            | Self::WaitingTool(_)
            | Self::WaitingInteraction(_)
            | Self::Cancelling(_)
            | Self::Closing(_)
            | Self::Closed(_)
            | Self::Faulted(_) => None,
        }
    }

    /// 返回 Agent 是否已排队。
    pub fn is_queued(&self) -> bool {
        matches!(self, Self::Queued(_))
    }

    /// 返回 Agent 是否等待用户 Interaction。
    pub fn is_waiting_interaction(&self) -> bool {
        matches!(self, Self::WaitingInteraction(_))
    }

    /// 返回 Agent 是否已停止执行并可供等待方收束。
    pub fn is_settled(&self) -> bool {
        matches!(self, Self::Idle(_) | Self::Closed(_) | Self::Faulted(_))
    }
}
