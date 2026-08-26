//! Agent 状态协议之上的纯转换规则。

use std::fmt;

use pl_protocol::{
    AgentFaultClassification, AgentSnapshot, AgentState, CancellingAgentState, ClosedAgentState,
    ClosingAgentState, FaultedAgentState, QueuedAgentState, RunningAgentState, StateError, TurnId,
    WaitingInteractionAgentState, WaitingToolAgentState,
};

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
        error: StateError,
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

impl AgentStateKind {
    fn from_state(state: &AgentState) -> Self {
        match state {
            AgentState::Idle(_) => Self::Idle,
            AgentState::Queued(_) => Self::Queued,
            AgentState::Running(_) => Self::Running,
            AgentState::WaitingTool(_) => Self::WaitingTool,
            AgentState::WaitingInteraction(_) => Self::WaitingInteraction,
            AgentState::Cancelling(_) => Self::Cancelling,
            AgentState::Closing(_) => Self::Closing,
            AgentState::Closed(_) => Self::Closed,
            AgentState::Faulted(_) => Self::Faulted,
        }
    }
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

/// 为协议层 [`AgentState`] 提供 pl-core 拥有的纯状态转换。
pub trait AgentStateTransition {
    /// 计算 command 对应的下一状态和 changed 事实。
    ///
    /// # Errors
    ///
    /// 当前状态不接受命令或 Turn identity 不匹配时返回转换错误。
    fn decide(self, command: AgentCommand)
    -> Result<AgentTransitionDecision, AgentTransitionError>;
}

impl AgentStateTransition for AgentState {
    fn decide(
        self,
        command: AgentCommand,
    ) -> Result<AgentTransitionDecision, AgentTransitionError> {
        let current = AgentStateKind::from_state(&self);
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
                let next_state = recovery_state(target);
                Ok(AgentTransitionDecision::changed(next_state))
            }
            (state, AgentCommand::Recover { target }) => {
                let next_state = recovery_state(target);
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
}

fn recovery_state(target: AgentRecoveryTarget) -> AgentState {
    match target {
        AgentRecoveryTarget::Idle => AgentState::idle(),
        AgentRecoveryTarget::Closed => AgentState::Closed(ClosedAgentState::new()),
    }
}

/// 为协议层 [`AgentSnapshot`] 提供 pl-core 拥有的原子状态转换。
pub trait AgentSnapshotTransition {
    /// 对 snapshot 应用 canonical Agent command。
    ///
    /// # Errors
    ///
    /// 当前状态不接受命令或 Turn identity 不匹配时返回转换错误。
    fn transition(&mut self, command: AgentCommand) -> Result<bool, AgentTransitionError>;
}

impl AgentSnapshotTransition for AgentSnapshot {
    fn transition(&mut self, command: AgentCommand) -> Result<bool, AgentTransitionError> {
        let decision = self.state.clone().decide(command)?;
        self.state = decision.next_state;
        Ok(decision.changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(id: &str) -> TurnId {
        TurnId::new(id).expect("valid Turn id")
    }

    #[test]
    fn active_agent_follows_declared_transition_path() {
        let queued = AgentState::idle()
            .decide(AgentCommand::Queue { turn_id: turn("1") })
            .expect("queue")
            .next_state;
        let running = queued
            .decide(AgentCommand::Start { turn_id: turn("1") })
            .expect("start")
            .next_state;
        let waiting = running
            .decide(AgentCommand::WaitForInteraction {
                turn_id: turn("1"),
                interaction_id: "ask-1".to_string(),
            })
            .expect("wait")
            .next_state;
        let settled = waiting
            .decide(AgentCommand::Settle { next_turn_id: None })
            .expect("settle")
            .next_state;

        assert!(settled.is_idle());
        assert!(
            settled
                .decide(AgentCommand::Close)
                .expect_err("idle cannot close directly")
                .to_string()
                .contains("does not accept")
        );
    }

    #[test]
    fn recover_faulted_accepts_only_typed_recoverable_faults() {
        let error = StateError {
            code: "runtime".to_string(),
            message: "failed".to_string(),
            retryable: true,
        };
        let recoverable = AgentState::Faulted(FaultedAgentState::classified(
            error.clone(),
            Some(turn("1")),
            AgentFaultClassification::RecoverableRuntime,
        ));
        assert!(
            recoverable
                .decide(AgentCommand::RecoverFaulted {
                    target: AgentRecoveryTarget::Idle,
                })
                .expect("recoverable fault")
                .next_state
                .is_idle()
        );

        let blocked = AgentState::Faulted(FaultedAgentState::classified(
            error,
            Some(turn("1")),
            AgentFaultClassification::AggregateCorruption,
        ));
        assert!(
            blocked
                .decide(AgentCommand::RecoverFaulted {
                    target: AgentRecoveryTarget::Idle,
                })
                .is_err()
        );
    }
}
