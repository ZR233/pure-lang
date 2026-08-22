use serde::{Deserialize, Serialize};

/// 尚未归属执行 Turn 的 mailbox 输入。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingMailboxState;

impl PendingMailboxState {
    pub fn new() -> Self {
        Self
    }
}
