//! Studio 关机进度的精确阶段状态。

mod cancelling_turns;
mod flushing_persistence;
mod stopped;
mod stopping_agents;
mod stopping_lsp;
mod stopping_mcp;
mod stopping_subscriptions;

pub use cancelling_turns::CancellingTurnsProgress;
pub use flushing_persistence::FlushingPersistenceProgress;
pub use stopped::StoppedProgress;
pub use stopping_agents::StoppingAgentsProgress;
pub use stopping_lsp::StoppingLspProgress;
pub use stopping_mcp::StoppingMcpProgress;
pub use stopping_subscriptions::StoppingSubscriptionsProgress;

use serde::{Deserialize, Serialize};

/// Studio 关机的固定阶段序列；只有刷新持久化阶段携带 pending commit 数。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioShutdownProgress {
    StoppingSubscriptions(StoppingSubscriptionsProgress),
    CancellingTurns(CancellingTurnsProgress),
    FlushingPersistence(FlushingPersistenceProgress),
    StoppingAgents(StoppingAgentsProgress),
    StoppingMcp(StoppingMcpProgress),
    StoppingLsp(StoppingLspProgress),
    Stopped(StoppedProgress),
}

impl StudioShutdownProgress {
    /// 1-based 阶段序号；驱动验收按它断言顺序与完备性。
    pub fn index(self) -> u8 {
        match self {
            Self::StoppingSubscriptions(_) => 1,
            Self::CancellingTurns(_) => 2,
            Self::FlushingPersistence(_) => 3,
            Self::StoppingAgents(_) => 4,
            Self::StoppingMcp(_) => 5,
            Self::StoppingLsp(_) => 6,
            Self::Stopped(_) => 7,
        }
    }

    pub fn is_stopped(self) -> bool {
        matches!(self, Self::Stopped(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_flushing_state_accepts_pending_commits() {
        let state =
            StudioShutdownProgress::FlushingPersistence(FlushingPersistenceProgress::new(3));
        let encoded = serde_json::to_value(state).unwrap();
        assert_eq!(encoded["kind"], "flushingPersistence");
        assert_eq!(encoded["data"]["pendingCommits"], 3);
        assert!(
            serde_json::from_value::<StudioShutdownProgress>(serde_json::json!({
                "kind": "stoppingMcp",
                "data": { "pendingCommits": 3 }
            }))
            .is_err()
        );
    }
}
