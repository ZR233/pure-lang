use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;

/// 已随指定 checkpoint 原子提交的 mailbox 输入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedMailboxState {
    turn_id: TurnId,
    checkpoint_seq: u64,
}

impl ConsumedMailboxState {
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
