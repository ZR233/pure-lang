//! Studio Agent 目录直接暴露的 canonical 生命周期投影。

mod cancelling;
mod closed;
mod closing;
mod faulted;
mod idle;
mod queued;
mod running;
mod waiting_interaction;
mod waiting_tool;

pub use cancelling::StudioCancellingAgent;
pub use closed::StudioClosedAgent;
pub use closing::StudioClosingAgent;
pub use faulted::StudioFaultedAgent;
pub use idle::StudioIdleAgent;
pub use queued::StudioQueuedAgent;
pub use running::StudioRunningAgent;
pub use waiting_interaction::StudioWaitingInteractionAgent;
pub use waiting_tool::StudioWaitingToolAgent;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioAgentState {
    Idle(StudioIdleAgent),
    Queued(StudioQueuedAgent),
    Running(StudioRunningAgent),
    WaitingTool(StudioWaitingToolAgent),
    WaitingInteraction(StudioWaitingInteractionAgent),
    Cancelling(StudioCancellingAgent),
    Closing(StudioClosingAgent),
    Closed(StudioClosedAgent),
    Faulted(StudioFaultedAgent),
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn agent_states_round_trip_with_exact_payloads() {
        let states = [
            StudioAgentState::Idle(StudioIdleAgent),
            StudioAgentState::Queued(StudioQueuedAgent::new("turn-1")),
            StudioAgentState::Running(StudioRunningAgent::new("turn-1")),
            StudioAgentState::WaitingTool(StudioWaitingToolAgent::new("turn-1")),
            StudioAgentState::WaitingInteraction(StudioWaitingInteractionAgent::new(
                "turn-1",
                "interaction-1",
            )),
            StudioAgentState::Cancelling(StudioCancellingAgent::new("turn-1")),
            StudioAgentState::Closing(StudioClosingAgent),
            StudioAgentState::Closed(StudioClosedAgent),
            StudioAgentState::Faulted(StudioFaultedAgent::new(
                pl_protocol::StateError {
                    code: "agentRuntimeFault".to_string(),
                    message: "runtime failure".to_string(),
                    retryable: false,
                },
                Some("turn-1".to_string()),
            )),
        ];

        for state in states {
            let json = serde_json::to_string(&state).expect("serialize Agent state");
            let restored = serde_json::from_str(&json).expect("deserialize Agent state");
            assert_eq!(state, restored);
        }
    }

    #[test]
    fn agent_state_rejects_flattened_legacy_axes() {
        let legacy = serde_json::json!({
            "status": "running",
            "lifecycle": "active",
            "activity": "activeRunning",
            "activeTurnId": "turn-1"
        });

        assert!(serde_json::from_value::<StudioAgentState>(legacy).is_err());
    }
}
