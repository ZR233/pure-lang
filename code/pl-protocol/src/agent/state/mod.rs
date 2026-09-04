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
            Self::Idle(_) | Self::Closing(_) | Self::Closed(_) => None,
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
        matches!(
            self,
            Self::Idle(_) | Self::Closing(_) | Self::Closed(_) | Self::Faulted(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;
    use crate::StateError;

    fn turn(value: &str) -> TurnId {
        TurnId::new(value).expect("valid Turn id")
    }

    #[test]
    fn agent_states_round_trip_with_exact_payloads() {
        let states = [
            AgentState::idle(),
            AgentState::Queued(QueuedAgentState::new(turn("turn-1"))),
            AgentState::Running(RunningAgentState::new(turn("turn-1"))),
            AgentState::WaitingTool(WaitingToolAgentState::new(turn("turn-1"))),
            AgentState::WaitingInteraction(WaitingInteractionAgentState::new(
                turn("turn-1"),
                "interaction-1".to_string(),
            )),
            AgentState::Cancelling(CancellingAgentState::new(turn("turn-1"))),
            AgentState::Closing(ClosingAgentState::new()),
            AgentState::Closed(ClosedAgentState::new()),
            AgentState::Faulted(FaultedAgentState::classified(
                StateError {
                    code: "agentRuntimeFault".to_string(),
                    message: "runtime failure".to_string(),
                    retryable: false,
                },
                Some(turn("turn-1")),
                AgentFaultClassification::RecoverableRuntime,
            )),
        ];

        for state in states {
            let json = serde_json::to_value(&state).expect("serialize Agent state");
            let restored = serde_json::from_value(json.clone()).expect("deserialize Agent state");
            assert_eq!(state, restored, "state JSON: {json}");
        }
    }

    #[test]
    fn agent_state_rejects_flattened_legacy_axes() {
        let legacy = json!({
            "status": "running",
            "lifecycle": "active",
            "activity": "activeRunning",
            "activeTurnId": "turn-1"
        });

        assert!(serde_json::from_value::<AgentState>(legacy).is_err());
    }

    #[test]
    fn legacy_idle_and_budget_pause_have_distinct_compatible_payloads() {
        let legacy: AgentState = serde_json::from_value(json!({
            "kind": "idle",
            "data": {}
        }))
        .expect("legacy idle");
        assert_eq!(legacy, AgentState::idle());

        let pause = AgentBudgetPause::new(
            turn("turn-budget"),
            crate::BudgetLimitSnapshot {
                kind: crate::BudgetLimitKind::WallClock,
                usage: crate::BudgetUsage {
                    elapsed_ms: 30_000,
                    ..crate::BudgetUsage::default()
                },
            },
            42,
        );
        let paused = AgentState::budget_paused(pause.clone());
        assert_eq!(paused.budget_pause(), Some(&pause));
        assert_eq!(
            serde_json::to_value(paused).expect("serialize budget pause"),
            json!({
                "kind": "idle",
                "data": {
                    "budgetPause": {
                        "turnId": "turn-budget",
                        "limit": {
                            "kind": "wallClock",
                            "usage": {
                                "modelSteps": 0,
                                "toolCalls": 0,
                                "waitCalls": 0,
                                "elapsedMs": 30_000
                            }
                        },
                        "pausedAt": 42
                    }
                }
            })
        );
    }
}
