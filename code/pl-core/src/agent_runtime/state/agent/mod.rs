//! Canonical Agent lifecycle state machine.

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
pub use idle::IdleAgentState;
pub use queued::QueuedAgentState;
pub use running::RunningAgentState;
pub use waiting_interaction::WaitingInteractionAgentState;
pub use waiting_tool::WaitingToolAgentState;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;

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

    /// 返回当前 active 或 queued Turn。
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

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle(_))
    }

    pub fn is_queued(&self) -> bool {
        matches!(self, Self::Queued(_))
    }

    pub fn is_waiting_interaction(&self) -> bool {
        matches!(self, Self::WaitingInteraction(_))
    }

    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            Self::Idle(_) | Self::Closing(_) | Self::Closed(_) | Self::Faulted(_)
        )
    }

    /// 计算 command 对应的纯状态转换。
    ///
    /// # Errors
    ///
    /// 当前状态不接受命令或 Turn identity 不匹配时返回 [`AgentTransitionError`]。
    pub fn decide(
        self,
        command: AgentCommand,
    ) -> Result<AgentTransitionDecision, AgentTransitionError> {
        let current = self.kind();
        match (self, command) {
            (Self::Idle(_), AgentCommand::Queue { turn_id }) => Ok(
                AgentTransitionDecision::changed(Self::Queued(QueuedAgentState::new(turn_id))),
            ),
            (
                Self::Idle(_),
                AgentCommand::RecoverWaitingInteraction {
                    turn_id,
                    interaction_id,
                },
            ) => Ok(AgentTransitionDecision::changed(Self::WaitingInteraction(
                WaitingInteractionAgentState::new(turn_id, interaction_id),
            ))),
            (Self::Queued(state), AgentCommand::Queue { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::unchanged(Self::Queued(state)))
            }
            (Self::Queued(state), AgentCommand::Start { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::changed(Self::Running(
                    RunningAgentState::new(turn_id),
                )))
            }
            (Self::Running(state), AgentCommand::Resume { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::unchanged(Self::Running(state)))
            }
            (Self::WaitingTool(state), AgentCommand::Resume { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::changed(Self::Running(
                    RunningAgentState::new(turn_id),
                )))
            }
            (Self::WaitingInteraction(state), AgentCommand::Resume { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::changed(Self::Running(
                    RunningAgentState::new(turn_id),
                )))
            }
            (Self::Running(_), AgentCommand::WaitForTool { turn_id }) => {
                Ok(AgentTransitionDecision::changed(Self::WaitingTool(
                    WaitingToolAgentState::new(turn_id),
                )))
            }
            (Self::WaitingTool(state), AgentCommand::WaitForTool { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::unchanged(Self::WaitingTool(state)))
            }
            (
                Self::Running(_) | Self::WaitingTool(_),
                AgentCommand::WaitForInteraction {
                    turn_id,
                    interaction_id,
                },
            ) => Ok(AgentTransitionDecision::changed(Self::WaitingInteraction(
                WaitingInteractionAgentState::new(turn_id, interaction_id),
            ))),
            (
                Self::WaitingInteraction(state),
                AgentCommand::WaitForInteraction {
                    turn_id,
                    interaction_id,
                },
            ) if state.turn_id() == &turn_id && state.interaction_id() == interaction_id => Ok(
                AgentTransitionDecision::unchanged(Self::WaitingInteraction(state)),
            ),
            (
                Self::Running(_) | Self::WaitingTool(_) | Self::WaitingInteraction(_),
                AgentCommand::Cancel { turn_id },
            ) => Ok(AgentTransitionDecision::changed(Self::Cancelling(
                CancellingAgentState::new(turn_id),
            ))),
            (Self::Cancelling(state), AgentCommand::Cancel { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(AgentTransitionDecision::unchanged(Self::Cancelling(state)))
            }
            (
                Self::Running(_)
                | Self::WaitingTool(_)
                | Self::WaitingInteraction(_)
                | Self::Cancelling(_),
                AgentCommand::Settle { next_turn_id },
            ) => Ok(AgentTransitionDecision::changed(match next_turn_id {
                Some(turn_id) => Self::Queued(QueuedAgentState::new(turn_id)),
                None => Self::idle(),
            })),
            (
                Self::WaitingInteraction(state),
                AgentCommand::ContinueInteraction {
                    interaction_id,
                    turn_id,
                },
            ) if state.interaction_id() == interaction_id => Ok(AgentTransitionDecision::changed(
                Self::Queued(QueuedAgentState::new(turn_id)),
            )),
            (
                Self::Idle(_) | Self::Queued(_) | Self::WaitingInteraction(_),
                AgentCommand::BeginClose,
            ) => Ok(AgentTransitionDecision::changed(Self::Closing(
                ClosingAgentState::new(),
            ))),
            (Self::Closing(_), AgentCommand::Close) => Ok(AgentTransitionDecision::changed(
                Self::Closed(ClosedAgentState::new()),
            )),
            (Self::Closing(_), AgentCommand::Restore { next_turn_id }) => {
                Ok(AgentTransitionDecision::changed(match next_turn_id {
                    Some(turn_id) => Self::Queued(QueuedAgentState::new(turn_id)),
                    None => Self::idle(),
                }))
            }
            (Self::Faulted(state), AgentCommand::RecoverFaulted { target })
                if state.classification().is_recoverable() =>
            {
                let next_state = match target {
                    AgentRecoveryTarget::Idle => Self::idle(),
                    AgentRecoveryTarget::Closed => Self::Closed(ClosedAgentState::new()),
                };
                Ok(AgentTransitionDecision::changed(next_state))
            }
            (state, AgentCommand::Recover { target }) => {
                let next_state = match target {
                    AgentRecoveryTarget::Idle => Self::idle(),
                    AgentRecoveryTarget::Closed => Self::Closed(ClosedAgentState::new()),
                };
                Ok(if state == next_state {
                    AgentTransitionDecision::unchanged(state)
                } else {
                    AgentTransitionDecision::changed(next_state)
                })
            }
            (
                state,
                AgentCommand::Fault {
                    error,
                    turn_id,
                    classification,
                },
            ) if !matches!(state, Self::Closed(_)) => {
                Ok(AgentTransitionDecision::changed(Self::Faulted(
                    FaultedAgentState::classified(error, turn_id, classification),
                )))
            }
            (_, command) => Err(AgentTransitionError::new(current, &command)),
        }
    }

    fn kind(&self) -> AgentStateKind {
        match self {
            Self::Idle(_) => AgentStateKind::Idle,
            Self::Queued(_) => AgentStateKind::Queued,
            Self::Running(_) => AgentStateKind::Running,
            Self::WaitingTool(_) => AgentStateKind::WaitingTool,
            Self::WaitingInteraction(_) => AgentStateKind::WaitingInteraction,
            Self::Cancelling(_) => AgentStateKind::Cancelling,
            Self::Closing(_) => AgentStateKind::Closing,
            Self::Closed(_) => AgentStateKind::Closed,
            Self::Faulted(_) => AgentStateKind::Faulted,
        }
    }
}

/// 可以改变 Agent 生命周期的领域命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCommand {
    Queue {
        turn_id: TurnId,
    },
    Start {
        turn_id: TurnId,
    },
    Resume {
        turn_id: TurnId,
    },
    WaitForTool {
        turn_id: TurnId,
    },
    WaitForInteraction {
        turn_id: TurnId,
        interaction_id: String,
    },
    RecoverWaitingInteraction {
        turn_id: TurnId,
        interaction_id: String,
    },
    ContinueInteraction {
        interaction_id: String,
        turn_id: TurnId,
    },
    Cancel {
        turn_id: TurnId,
    },
    Settle {
        next_turn_id: Option<TurnId>,
    },
    BeginClose,
    Close,
    Restore {
        next_turn_id: Option<TurnId>,
    },
    Recover {
        target: AgentRecoveryTarget,
    },
    RecoverFaulted {
        target: AgentRecoveryTarget,
    },
    Fault {
        error: pl_protocol::StateError,
        turn_id: Option<TurnId>,
        classification: AgentFaultClassification,
    },
}

impl AgentCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Queue { .. } => "queue",
            Self::Start { .. } => "start",
            Self::Resume { .. } => "resume",
            Self::WaitForTool { .. } => "waitForTool",
            Self::WaitForInteraction { .. } => "waitForInteraction",
            Self::RecoverWaitingInteraction { .. } => "recoverWaitingInteraction",
            Self::ContinueInteraction { .. } => "continueInteraction",
            Self::Cancel { .. } => "cancel",
            Self::Settle { .. } => "settle",
            Self::BeginClose => "beginClose",
            Self::Close => "close",
            Self::Restore { .. } => "restore",
            Self::Recover { .. } => "recover",
            Self::RecoverFaulted { .. } => "recoverFaulted",
            Self::Fault { .. } => "fault",
        }
    }
}

/// session recovery 重建 Agent 时的显式目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRecoveryTarget {
    Idle,
    Closed,
}

/// Agent 状态机的纯转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTransitionDecision {
    pub next_state: AgentState,
    pub changed: bool,
}

impl AgentTransitionDecision {
    fn changed(next_state: AgentState) -> Self {
        Self {
            next_state,
            changed: true,
        }
    }

    fn unchanged(next_state: AgentState) -> Self {
        Self {
            next_state,
            changed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentStateKind {
    Idle,
    Queued,
    Running,
    WaitingTool,
    WaitingInteraction,
    Cancelling,
    Closing,
    Closed,
    Faulted,
}

/// 非法 Agent 状态转换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTransitionError {
    current: AgentStateKind,
    command: &'static str,
}

impl AgentTransitionError {
    fn new(current: AgentStateKind, command: &AgentCommand) -> Self {
        Self {
            current,
            command: command.name(),
        }
    }
}

impl fmt::Display for AgentTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "agent state {:?} does not accept command {}",
            self.current, self.command
        )
    }
}

impl std::error::Error for AgentTransitionError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(id: &str) -> TurnId {
        TurnId::new(id).unwrap()
    }

    #[test]
    fn active_agent_follows_declared_transition_path() {
        let queued = AgentState::idle()
            .decide(AgentCommand::Queue { turn_id: turn("1") })
            .unwrap()
            .next_state;
        let running = queued
            .decide(AgentCommand::Start { turn_id: turn("1") })
            .unwrap()
            .next_state;
        let waiting = running
            .decide(AgentCommand::WaitForInteraction {
                turn_id: turn("1"),
                interaction_id: "ask-1".to_string(),
            })
            .unwrap()
            .next_state;
        let settled = waiting
            .decide(AgentCommand::Settle { next_turn_id: None })
            .unwrap()
            .next_state;

        assert!(settled.is_idle());
        assert!(
            settled
                .decide(AgentCommand::Close)
                .expect_err("idle must begin close before terminal close")
                .to_string()
                .contains("close")
        );
    }

    #[test]
    fn recover_faulted_accepts_only_typed_recoverable_faults() {
        let error = pl_protocol::StateError {
            code: "fault".to_string(),
            message: "failed".to_string(),
            retryable: false,
        };
        let recoverable = AgentState::Faulted(FaultedAgentState::classified(
            error.clone(),
            Some(turn("recoverable")),
            AgentFaultClassification::RecoverableRuntime,
        ));
        let recovered = recoverable
            .decide(AgentCommand::RecoverFaulted {
                target: AgentRecoveryTarget::Idle,
            })
            .expect("recoverable fault")
            .next_state;
        assert!(recovered.is_idle());

        let corrupt = AgentState::Faulted(FaultedAgentState::classified(
            error,
            None,
            AgentFaultClassification::AggregateCorruption,
        ));
        assert!(
            corrupt
                .decide(AgentCommand::RecoverFaulted {
                    target: AgentRecoveryTarget::Idle,
                })
                .is_err()
        );
    }
}
