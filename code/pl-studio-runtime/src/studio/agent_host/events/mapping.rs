use crate::{
    StudioAgentProgressRuntime, StudioAgentState, StudioCancellingAgent, StudioClosedAgent,
    StudioClosingAgent, StudioFaultedAgent, StudioIdleAgent, StudioQueuedAgent, StudioRunningAgent,
    StudioWaitingInteractionAgent, StudioWaitingToolAgent,
};
use pl_core::{AgentProgressStage, AgentState};

#[cfg(test)]
use pl_core::AgentSnapshot;

#[cfg(test)]
pub(super) fn thread_status(snapshot: &AgentSnapshot) -> pl_protocol::ThreadStatus {
    super::super::repository::labels::thread_status(&snapshot.state)
}

pub(super) fn studio_agent_state(state: &AgentState) -> StudioAgentState {
    match state {
        AgentState::Idle(_) => StudioAgentState::Idle(StudioIdleAgent),
        AgentState::Queued(value) => {
            StudioAgentState::Queued(StudioQueuedAgent::new(value.turn_id().to_string()))
        }
        AgentState::Running(value) => {
            StudioAgentState::Running(StudioRunningAgent::new(value.turn_id().to_string()))
        }
        AgentState::WaitingTool(value) => {
            StudioAgentState::WaitingTool(StudioWaitingToolAgent::new(value.turn_id().to_string()))
        }
        AgentState::WaitingInteraction(value) => {
            StudioAgentState::WaitingInteraction(StudioWaitingInteractionAgent::new(
                value.turn_id().to_string(),
                value.interaction_id().to_string(),
            ))
        }
        AgentState::Cancelling(value) => {
            StudioAgentState::Cancelling(StudioCancellingAgent::new(value.turn_id().to_string()))
        }
        AgentState::Closing(_) => StudioAgentState::Closing(StudioClosingAgent),
        AgentState::Closed(_) => StudioAgentState::Closed(StudioClosedAgent),
        AgentState::Faulted(value) => StudioAgentState::Faulted(StudioFaultedAgent::new(
            value.error().clone(),
            value.turn_id().map(ToString::to_string),
        )),
    }
}

pub(crate) const fn progress_stage_label(stage: AgentProgressStage) -> &'static str {
    match stage {
        AgentProgressStage::Exploring => "exploring",
        AgentProgressStage::Implementing => "implementing",
        AgentProgressStage::Verifying => "verifying",
        AgentProgressStage::Blocked => "blocked",
        AgentProgressStage::ReadyForCompletion => "readyForCompletion",
        AgentProgressStage::ReadyForReview => "readyForReview",
    }
}

impl From<&pl_core::AgentProgressCheckpoint> for StudioAgentProgressRuntime {
    fn from(progress: &pl_core::AgentProgressCheckpoint) -> Self {
        Self {
            stage: progress_stage_label(progress.report.stage).to_string(),
            summary: progress.report.summary.clone(),
            next_step: progress.report.next_step.clone(),
            revision: progress.report.revision,
            updated_at: progress.updated_at,
        }
    }
}

pub(crate) fn progress_stage_from_label(label: &str) -> AgentProgressStage {
    match label {
        "implementing" => AgentProgressStage::Implementing,
        "verifying" => AgentProgressStage::Verifying,
        "blocked" => AgentProgressStage::Blocked,
        "readyForCompletion" => AgentProgressStage::ReadyForCompletion,
        "readyForReview" => AgentProgressStage::ReadyForReview,
        _ => AgentProgressStage::Exploring,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, ThreadId};

    #[test]
    fn budget_limited_turn_leaves_thread_idle_without_error() {
        let snapshot = snapshot_without_outcome();

        assert_eq!(thread_status(&snapshot), pl_protocol::ThreadStatus::Idle);
        assert_eq!(
            studio_agent_state(&snapshot.state),
            StudioAgentState::Idle(StudioIdleAgent)
        );
    }

    fn snapshot_without_outcome() -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: ThreadId::new("agent-1").expect("agent id"),
                parent_id: None,
                role: AgentRoleId::new("planner").expect("role id"),
                depth: 0,
            },
            state: AgentState::idle(),
            pending_inputs: 0,
            progress: None,
            last_turn: None,
            revision: 1,
            event_sequence: 1,
            updated_at: 7,
        }
    }
}
