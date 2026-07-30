use pl_protocol::{InteractionKind, InteractionRequest, InteractionStatus, SessionTurnActivity};

pub(super) fn turn_activity_for_interaction(
    interaction: &InteractionRequest,
) -> Option<SessionTurnActivity> {
    match interaction.status {
        InteractionStatus::Pending => Some(match interaction.kind {
            InteractionKind::ToolApproval => SessionTurnActivity::WaitingForApproval,
            InteractionKind::UserInput => SessionTurnActivity::WaitingForUserInput,
            InteractionKind::PlanConfirmation => SessionTurnActivity::WaitingForPlanConfirmation,
        }),
        InteractionStatus::Resolved => Some(SessionTurnActivity::Thinking),
        InteractionStatus::Cancelled | InteractionStatus::Expired => None,
    }
}
