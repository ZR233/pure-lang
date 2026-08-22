//! Turn aggregate and its canonical lifecycle state machine.

mod outcome;
mod state;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use outcome::{
    BudgetLimitedTurnOutcome, CancelledTurnOutcome, CompletedTurnOutcome, FailedTurnOutcome,
    TurnOutcome,
};
pub use state::{
    BudgetLimitedTurnState, CancelledTurnState, CompletedTurnState, FailedTurnState,
    QueuedTurnState, RunningTurnState, TurnState,
};

use crate::{BudgetLimitSnapshot, TurnFailure};

/// Thread 中一次由明确输入启动的执行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    pub thread_id: String,
    pub revision: u64,
    pub state: TurnState,
    pub updated_at: i64,
}

impl Turn {
    /// 创建排队中的 Turn。
    pub fn queued(id: impl Into<String>, thread_id: impl Into<String>, queued_at: i64) -> Self {
        Self {
            id: id.into(),
            thread_id: thread_id.into(),
            revision: 0,
            state: TurnState::Queued(QueuedTurnState::new(queued_at)),
            updated_at: queued_at,
        }
    }

    /// 根据领域命令计算下一状态，不执行任何 IO。
    ///
    /// # Errors
    ///
    /// 当前状态不接受该命令时返回 [`TurnTransitionError`]。
    pub fn decide(
        &self,
        command: TurnCommand,
    ) -> Result<TurnTransitionDecision, TurnTransitionError> {
        if command.turn_id() != self.id {
            return Err(TurnTransitionError::wrong_turn(self, &command));
        }
        if command.expected_revision() != self.revision {
            return Err(TurnTransitionError::stale_revision(self, &command));
        }
        self.state
            .clone()
            .decide(command)
            .map_err(|error| error.at(self))
    }

    /// 返回 Turn 开始执行的时间；排队期间可能为空。
    pub fn started_at(&self) -> Option<i64> {
        self.state.started_at()
    }

    /// 返回终态完成时间。
    pub fn completed_at(&self) -> Option<i64> {
        self.state.completed_at()
    }

    /// 返回运行阶段。
    pub fn phase(&self) -> Option<TurnPhase> {
        self.state.phase()
    }

    /// 返回失败终态携带的结构化失败。
    pub fn failure(&self) -> Option<&TurnFailure> {
        self.state.failure()
    }

    /// 应用已经决定的状态并更新时间。
    pub fn apply(&mut self, decision: TurnTransitionDecision, updated_at: i64) {
        if decision.changed {
            self.state = decision.next_state;
            self.revision = self.revision.saturating_add(1);
            self.updated_at = updated_at;
        }
    }
}

/// Turn 执行期间的可展示活动阶段。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnPhase {
    Preparing,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    Persisting,
}

/// Turn 正常完成的业务边界。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnCompletion {
    Normal,
    InteractionRequested,
}

/// Turn 被取消的强类型原因。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TurnCancellationCause {
    UserRequested,
    RuntimeShutdown,
    AgentClosed,
    Recovery,
    Coalesced { target_turn_id: String },
}

/// 预算终态后的上下文压缩结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TurnRolloverOutcome {
    NotAttempted,
    Succeeded,
    Failed { error: String },
}

/// 可以改变 Turn 生命周期的领域命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnCommand {
    Start {
        turn_id: String,
        expected_revision: u64,
        started_at: i64,
    },
    Advance {
        turn_id: String,
        expected_revision: u64,
        phase: TurnPhase,
    },
    Complete {
        turn_id: String,
        expected_revision: u64,
        completion: TurnCompletion,
        completed_at: i64,
    },
    Cancel {
        turn_id: String,
        expected_revision: u64,
        cause: TurnCancellationCause,
        completed_at: i64,
    },
    Fail {
        turn_id: String,
        expected_revision: u64,
        failure: TurnFailure,
        completed_at: i64,
    },
    LimitBudget {
        turn_id: String,
        expected_revision: u64,
        limit: BudgetLimitSnapshot,
        rollover: TurnRolloverOutcome,
        completed_at: i64,
    },
}

impl TurnCommand {
    fn turn_id(&self) -> &str {
        match self {
            Self::Start { turn_id, .. }
            | Self::Advance { turn_id, .. }
            | Self::Complete { turn_id, .. }
            | Self::Cancel { turn_id, .. }
            | Self::Fail { turn_id, .. }
            | Self::LimitBudget { turn_id, .. } => turn_id,
        }
    }

    fn expected_revision(&self) -> u64 {
        match self {
            Self::Start {
                expected_revision, ..
            }
            | Self::Advance {
                expected_revision, ..
            }
            | Self::Complete {
                expected_revision, ..
            }
            | Self::Cancel {
                expected_revision, ..
            }
            | Self::Fail {
                expected_revision, ..
            }
            | Self::LimitBudget {
                expected_revision, ..
            } => *expected_revision,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start",
            Self::Advance { .. } => "advance",
            Self::Complete { .. } => "complete",
            Self::Cancel { .. } => "cancel",
            Self::Fail { .. } => "fail",
            Self::LimitBudget { .. } => "limitBudget",
        }
    }
}

/// Turn 状态机的纯转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTransitionDecision {
    pub next_state: TurnState,
    pub changed: bool,
}

impl TurnTransitionDecision {
    fn state(current_state: &TurnState, next_state: TurnState) -> Self {
        Self {
            changed: *current_state != next_state,
            next_state,
        }
    }
}

/// 非法 Turn 状态转换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTransitionError {
    turn_id: String,
    current: state::TurnStateKind,
    command: &'static str,
    violation: TurnTransitionViolation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnTransitionViolation {
    IllegalTransition,
    WrongTurn { actual: String },
    StaleRevision { expected: u64, actual: u64 },
}

impl TurnTransitionError {
    fn new(current: state::TurnStateKind, command: &TurnCommand) -> Self {
        Self {
            turn_id: String::new(),
            current,
            command: command.name(),
            violation: TurnTransitionViolation::IllegalTransition,
        }
    }

    fn at(mut self, turn: &Turn) -> Self {
        self.turn_id = turn.id.clone();
        self
    }

    fn wrong_turn(turn: &Turn, command: &TurnCommand) -> Self {
        Self {
            turn_id: turn.id.clone(),
            current: turn.state.kind(),
            command: command.name(),
            violation: TurnTransitionViolation::WrongTurn {
                actual: command.turn_id().to_owned(),
            },
        }
    }

    fn stale_revision(turn: &Turn, command: &TurnCommand) -> Self {
        Self {
            turn_id: turn.id.clone(),
            current: turn.state.kind(),
            command: command.name(),
            violation: TurnTransitionViolation::StaleRevision {
                expected: turn.revision,
                actual: command.expected_revision(),
            },
        }
    }
}

impl fmt::Display for TurnTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.violation {
            TurnTransitionViolation::IllegalTransition => write!(
                formatter,
                "turn {}/{:?} does not accept command {}",
                self.turn_id, self.current, self.command
            ),
            TurnTransitionViolation::WrongTurn { actual } => write!(
                formatter,
                "turn {} rejected command {} for different turn {}",
                self.turn_id, self.command, actual
            ),
            TurnTransitionViolation::StaleRevision { expected, actual } => write!(
                formatter,
                "turn {} rejected command {} at revision {}; current revision is {}",
                self.turn_id, self.command, actual, expected
            ),
        }
    }
}

impl std::error::Error for TurnTransitionError {}

impl TurnState {
    fn decide(self, command: TurnCommand) -> Result<TurnTransitionDecision, TurnTransitionError> {
        let current = self.kind();
        let current_state = self.clone();
        match (self, command) {
            (Self::Queued(_), TurnCommand::Start { started_at, .. }) => {
                Ok(TurnTransitionDecision::state(
                    &current_state,
                    Self::Running(RunningTurnState::new(started_at, TurnPhase::Preparing)),
                ))
            }
            (Self::Running(state), TurnCommand::Advance { phase, .. }) => Ok(
                TurnTransitionDecision::state(&current_state, Self::Running(state.advance(phase))),
            ),
            (
                Self::Running(state),
                TurnCommand::Complete {
                    completion,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::Completed(CompletedTurnState::new(
                    Some(state.started_at()),
                    completed_at,
                    completion,
                )),
            )),
            (
                Self::Queued(state),
                TurnCommand::Cancel {
                    cause,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::Cancelled(CancelledTurnState::new(
                    None,
                    state.queued_at(),
                    completed_at,
                    cause,
                )),
            )),
            (
                Self::Running(state),
                TurnCommand::Cancel {
                    cause,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::Cancelled(CancelledTurnState::new(
                    Some(state.started_at()),
                    state.started_at(),
                    completed_at,
                    cause,
                )),
            )),
            (
                Self::Running(state),
                TurnCommand::Fail {
                    failure,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::Failed(FailedTurnState::new(
                    Some(state.started_at()),
                    completed_at,
                    failure,
                )),
            )),
            (
                Self::Queued(_),
                TurnCommand::Fail {
                    failure,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::Failed(FailedTurnState::new(None, completed_at, failure)),
            )),
            (
                Self::Running(state),
                TurnCommand::LimitBudget {
                    limit,
                    rollover,
                    completed_at,
                    ..
                },
            ) => Ok(TurnTransitionDecision::state(
                &current_state,
                Self::BudgetLimited(BudgetLimitedTurnState::new(
                    Some(state.started_at()),
                    completed_at,
                    limit,
                    rollover,
                )),
            )),
            (_, command) => Err(TurnTransitionError::new(current, &command)),
        }
    }
}

crate::impl_labeled_enum!(
    TurnPhase,
    "TurnPhase",
    [
        TurnPhase::Preparing => "preparing",
        TurnPhase::Thinking => "thinking",
        TurnPhase::Responding => "responding",
        TurnPhase::Planning => "planning",
        TurnPhase::RunningTool => "runningTool",
        TurnPhase::Persisting => "persisting",
    ]
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BudgetLimitKind, BudgetUsage, TurnFailureCategory};

    fn failure() -> TurnFailure {
        TurnFailure::permanent(TurnFailureCategory::Internal, "failed")
    }

    #[test]
    fn turn_state_machine_preserves_terminal_payloads() {
        let mut turn = Turn::queued("turn", "thread", 1);
        turn.apply(
            turn.decide(TurnCommand::Start {
                turn_id: "turn".to_string(),
                expected_revision: 0,
                started_at: 2,
            })
            .unwrap(),
            2,
        );
        turn.apply(
            turn.decide(TurnCommand::Advance {
                turn_id: "turn".to_string(),
                expected_revision: 1,
                phase: TurnPhase::Thinking,
            })
            .unwrap(),
            3,
        );
        turn.apply(
            turn.decide(TurnCommand::LimitBudget {
                turn_id: "turn".to_string(),
                expected_revision: 2,
                limit: BudgetLimitSnapshot {
                    kind: BudgetLimitKind::ModelStep,
                    usage: BudgetUsage {
                        model_steps: 10,
                        ..BudgetUsage::default()
                    },
                },
                rollover: TurnRolloverOutcome::Failed {
                    error: "compact failed".to_string(),
                },
                completed_at: 4,
            })
            .unwrap(),
            4,
        );

        assert!(matches!(turn.state, TurnState::BudgetLimited(_)));
        assert!(
            turn.decide(TurnCommand::Fail {
                turn_id: "turn".to_string(),
                expected_revision: 3,
                failure: failure(),
                completed_at: 5,
            })
            .is_err()
        );
    }

    #[test]
    fn turn_commands_validate_identity_revision_and_no_op_payloads() {
        let mut turn = Turn::queued("turn", "thread", 1);
        let wrong_turn = turn
            .decide(TurnCommand::Start {
                turn_id: "other".to_string(),
                expected_revision: 0,
                started_at: 2,
            })
            .expect_err("another Turn identity must be rejected");
        assert!(wrong_turn.to_string().contains("different turn"));

        let stale = turn
            .decide(TurnCommand::Start {
                turn_id: "turn".to_string(),
                expected_revision: 1,
                started_at: 2,
            })
            .expect_err("stale revision must be rejected");
        assert!(stale.to_string().contains("current revision is 0"));

        let started = turn
            .decide(TurnCommand::Start {
                turn_id: "turn".to_string(),
                expected_revision: 0,
                started_at: 2,
            })
            .expect("queued Turn should start");
        turn.apply(started, 2);
        let repeated_phase = turn
            .decide(TurnCommand::Advance {
                turn_id: "turn".to_string(),
                expected_revision: 1,
                phase: TurnPhase::Preparing,
            })
            .expect("repeating the current phase is a no-op");
        assert!(!repeated_phase.changed);
        turn.apply(repeated_phase, 3);
        assert_eq!(turn.revision, 1);
        assert_eq!(turn.updated_at, 2);
    }
}
