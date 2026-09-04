use serde::{Deserialize, Serialize};

use crate::TurnId;

/// 已由指定 Turn claim、尚未被 checkpoint 消费的 mailbox 输入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedMailboxState {
    turn_id: TurnId,
    checkpoint_seq: u64,
}

impl ClaimedMailboxState {
    pub fn new(turn_id: TurnId, checkpoint_seq: u64) -> Self {
        Self {
            turn_id,
            checkpoint_seq,
        }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    pub fn checkpoint_seq(&self) -> u64 {
        self.checkpoint_seq
    }
}
