use pl_core::{
    ActiveKind, AgentActivityState, AgentLifecycleState, AgentProgressStage, AgentSnapshot,
    TurnOutcomeKind,
};

use crate::{PlanLifecycleState, StudioAgentActivity, StudioAgentProgressRuntime};

pub(super) fn thread_status(snapshot: &AgentSnapshot) -> pl_protocol::ThreadStatus {
    match snapshot.lifecycle {
        AgentLifecycleState::Closing | AgentLifecycleState::Closed => {
            pl_protocol::ThreadStatus::Closed
        }
        AgentLifecycleState::Faulted => pl_protocol::ThreadStatus::Failed,
        AgentLifecycleState::Active => match snapshot.activity {
            AgentActivityState::Queued | AgentActivityState::Active(ActiveKind::Running) => {
                pl_protocol::ThreadStatus::Running
            }
            AgentActivityState::Active(
                ActiveKind::WaitingTool | ActiveKind::WaitingInteraction,
            )
            | AgentActivityState::Cancelling => pl_protocol::ThreadStatus::Waiting,
            AgentActivityState::Idle => pl_protocol::ThreadStatus::Idle,
        },
    }
}

pub(super) fn error(snapshot: &AgentSnapshot) -> Option<String> {
    snapshot
        .last_turn
        .as_ref()
        .filter(|outcome| outcome.kind == TurnOutcomeKind::Failed)
        .and_then(|outcome| outcome.reason.clone())
}

pub(super) fn plan_terminal_projection(
    outcome: TurnOutcomeKind,
    reason: Option<String>,
) -> Option<(PlanLifecycleState, Option<String>)> {
    match outcome {
        TurnOutcomeKind::Completed => Some((PlanLifecycleState::Implemented, None)),
        TurnOutcomeKind::Cancelled | TurnOutcomeKind::Failed => {
            Some((PlanLifecycleState::ImplementationFailed, reason))
        }
        TurnOutcomeKind::BudgetLimited => None,
    }
}

pub(super) const fn lifecycle_label(lifecycle: AgentLifecycleState) -> &'static str {
    match lifecycle {
        AgentLifecycleState::Active => "active",
        AgentLifecycleState::Closing => "closing",
        AgentLifecycleState::Closed => "closed",
        AgentLifecycleState::Faulted => "faulted",
    }
}

pub(super) const fn studio_agent_activity(activity: AgentActivityState) -> StudioAgentActivity {
    match activity {
        AgentActivityState::Idle => StudioAgentActivity::Idle,
        AgentActivityState::Queued => StudioAgentActivity::Queued,
        AgentActivityState::Active(ActiveKind::Running) => StudioAgentActivity::ActiveRunning,
        AgentActivityState::Active(ActiveKind::WaitingTool) => {
            StudioAgentActivity::ActiveWaitingTool
        }
        AgentActivityState::Active(ActiveKind::WaitingInteraction) => {
            StudioAgentActivity::ActiveWaitingInteraction
        }
        AgentActivityState::Cancelling => StudioAgentActivity::Cancelling,
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
    use pl_core::{AgentIdentity, AgentRoleId, AgentTurnOutcome, ThreadId, TurnId};

    use super::*;

    #[test]
    fn budget_limited_turn_leaves_thread_idle_without_error() {
        let snapshot = snapshot_with_outcome(TurnOutcomeKind::BudgetLimited);

        assert_eq!(thread_status(&snapshot), pl_protocol::ThreadStatus::Idle);
        assert_eq!(error(&snapshot), None);
    }

    #[test]
    fn budget_limited_plan_keeps_implementing_lifecycle() {
        assert_eq!(
            plan_terminal_projection(
                TurnOutcomeKind::BudgetLimited,
                Some("budget reached".to_string()),
            ),
            None
        );
        assert_eq!(
            plan_terminal_projection(TurnOutcomeKind::Failed, Some("failed".to_string())),
            Some((
                PlanLifecycleState::ImplementationFailed,
                Some("failed".to_string()),
            ))
        );
    }

    fn snapshot_with_outcome(kind: TurnOutcomeKind) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: ThreadId::new("agent-1").expect("agent id"),
                parent_id: None,
                role: AgentRoleId::new("planner").expect("role id"),
                depth: 0,
            },
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Idle,
            active_turn_id: None,
            pending_inputs: 0,
            progress: None,
            last_turn: Some(AgentTurnOutcome {
                turn_id: TurnId::new("turn-1").expect("turn id"),
                thread_id: pl_core::ThreadId::new("session-1").expect("thread id"),
                kind,
                reason: Some("budget reached".to_string()),
                failure: None,
                budget_limit: None,
                rollover_compacted: false,
                rollover_compaction_error: None,
                usage: Default::default(),
                finished_at: 7,
            }),
            revision: 1,
            event_sequence: 1,
            updated_at: 7,
        }
    }
}
