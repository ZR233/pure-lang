use pl_core::AgentProgressStage;

#[cfg(test)]
use pl_core::AgentSnapshot;

#[cfg(test)]
pub(super) fn thread_status(snapshot: &AgentSnapshot) -> pl_protocol::ThreadStatus {
    super::super::repository::labels::thread_status(&snapshot.state)
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

pub(crate) fn progress_stage_from_label(label: &str) -> AgentProgressStage {
    match label {
        "exploring" => AgentProgressStage::Exploring,
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
    fn idle_agent_projects_to_idle_thread() {
        let snapshot = AgentSnapshot {
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
        };

        assert_eq!(thread_status(&snapshot), pl_protocol::ThreadStatus::Idle);
    }
}
