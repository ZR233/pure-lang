//! Durable mailbox delivery state machine.

mod claimed;
mod consumed;
mod pending;

pub use claimed::ClaimedMailboxState;
pub use consumed::ConsumedMailboxState;
pub use pending::PendingMailboxState;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;

/// mailbox envelope 的唯一持久投递状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum MailboxDeliveryState {
    Pending(PendingMailboxState),
    Claimed(ClaimedMailboxState),
    Consumed(ConsumedMailboxState),
}

impl Default for MailboxDeliveryState {
    fn default() -> Self {
        Self::Pending(PendingMailboxState::new())
    }
}

impl MailboxDeliveryState {
    /// 计算 mailbox 投递状态转换。
    ///
    /// # Errors
    ///
    /// 当前状态不接受命令、Turn 不匹配或 checkpoint 倒退时返回转换错误。
    pub fn decide(
        self,
        command: MailboxCommand,
    ) -> Result<MailboxTransitionDecision, MailboxTransitionError> {
        let current = self.kind();
        match (self, command) {
            (Self::Pending(_), MailboxCommand::Claim { turn_id }) => {
                Ok(MailboxTransitionDecision::changed(Self::Claimed(
                    ClaimedMailboxState::new(turn_id, 0),
                )))
            }
            (Self::Claimed(state), MailboxCommand::Claim { turn_id })
                if state.turn_id() == &turn_id =>
            {
                Ok(MailboxTransitionDecision::unchanged(Self::Claimed(state)))
            }
            (
                Self::Claimed(state),
                MailboxCommand::Consume {
                    turn_id,
                    checkpoint_seq,
                },
            ) if state.turn_id() == &turn_id && checkpoint_seq >= state.checkpoint_seq() => {
                Ok(MailboxTransitionDecision::changed(Self::Consumed(
                    ConsumedMailboxState::new(turn_id, checkpoint_seq),
                )))
            }
            (
                Self::Consumed(state),
                MailboxCommand::Consume {
                    turn_id,
                    checkpoint_seq,
                },
            ) if state.turn_id() == &turn_id && state.checkpoint_seq() == checkpoint_seq => {
                Ok(MailboxTransitionDecision::unchanged(Self::Consumed(state)))
            }
            (Self::Claimed(_), MailboxCommand::Requeue) => Ok(MailboxTransitionDecision::changed(
                Self::Pending(PendingMailboxState::new()),
            )),
            (_, command) => Err(MailboxTransitionError::new(current, &command)),
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending(_))
    }

    pub fn is_claimed(&self) -> bool {
        matches!(self, Self::Claimed(_))
    }

    pub fn is_consumed(&self) -> bool {
        matches!(self, Self::Consumed(_))
    }

    pub fn turn_id(&self) -> Option<&TurnId> {
        match self {
            Self::Claimed(state) => Some(state.turn_id()),
            Self::Consumed(state) => Some(state.turn_id()),
            Self::Pending(_) => None,
        }
    }

    pub fn checkpoint_seq(&self) -> Option<u64> {
        match self {
            Self::Claimed(state) => Some(state.checkpoint_seq()),
            Self::Consumed(state) => Some(state.checkpoint_seq()),
            Self::Pending(_) => None,
        }
    }

    fn kind(&self) -> MailboxStateKind {
        match self {
            Self::Pending(_) => MailboxStateKind::Pending,
            Self::Claimed(_) => MailboxStateKind::Claimed,
            Self::Consumed(_) => MailboxStateKind::Consumed,
        }
    }
}

/// mailbox 投递命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxCommand {
    Claim {
        turn_id: TurnId,
    },
    Consume {
        turn_id: TurnId,
        checkpoint_seq: u64,
    },
    Requeue,
}

impl MailboxCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Claim { .. } => "claim",
            Self::Consume { .. } => "consume",
            Self::Requeue => "requeue",
        }
    }
}

/// mailbox 状态转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxTransitionDecision {
    pub next_state: MailboxDeliveryState,
    pub changed: bool,
}

impl MailboxTransitionDecision {
    fn changed(next_state: MailboxDeliveryState) -> Self {
        Self {
            next_state,
            changed: true,
        }
    }

    fn unchanged(next_state: MailboxDeliveryState) -> Self {
        Self {
            next_state,
            changed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MailboxStateKind {
    Pending,
    Claimed,
    Consumed,
}

/// 非法 mailbox 投递状态转换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxTransitionError {
    current: MailboxStateKind,
    command: &'static str,
}

impl MailboxTransitionError {
    fn new(current: MailboxStateKind, command: &MailboxCommand) -> Self {
        Self {
            current,
            command: command.name(),
        }
    }
}

impl fmt::Display for MailboxTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "mailbox state {:?} does not accept command {}",
            self.current, self.command
        )
    }
}

impl std::error::Error for MailboxTransitionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_requires_claim_before_consume() {
        let turn_id = TurnId::new("turn-1").unwrap();
        let pending = MailboxDeliveryState::default();
        assert!(
            pending
                .clone()
                .decide(MailboxCommand::Consume {
                    turn_id: turn_id.clone(),
                    checkpoint_seq: 1,
                })
                .is_err()
        );
        let claimed = pending
            .decide(MailboxCommand::Claim {
                turn_id: turn_id.clone(),
            })
            .unwrap()
            .next_state;
        let consumed = claimed
            .decide(MailboxCommand::Consume {
                turn_id,
                checkpoint_seq: 1,
            })
            .unwrap()
            .next_state;
        assert!(consumed.is_consumed());
    }
}
