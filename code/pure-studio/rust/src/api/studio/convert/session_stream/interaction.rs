use anyhow::Result;
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionResolution,
    InteractionStatus, PlanConfirmationResolution, ToolApprovalResolution, UserQuestion,
};

use crate::api::studio::types::*;

pub(super) fn interaction(value: InteractionRequest) -> Result<BridgeInteractionRequest> {
    Ok(BridgeInteractionRequest {
        interaction_id: value.interaction_id,
        kind: match value.kind {
            InteractionKind::UserInput => BridgeInteractionKind::UserInput,
            InteractionKind::ToolApproval => BridgeInteractionKind::ToolApproval,
            InteractionKind::PlanConfirmation => BridgeInteractionKind::PlanConfirmation,
        },
        status: match value.status {
            InteractionStatus::Pending => BridgeInteractionStatus::Pending,
            InteractionStatus::Resolved => BridgeInteractionStatus::Resolved,
            InteractionStatus::Cancelled => BridgeInteractionStatus::Cancelled,
            InteractionStatus::Expired => BridgeInteractionStatus::Expired,
        },
        scope: BridgeInteractionScope {
            session_id: value.scope.session_id,
            turn_id: value.scope.turn_id,
            item_id: value.scope.item_id,
            tool_id: value.scope.tool_id,
            agent_path: value.scope.agent_path,
        },
        payload: payload(value.payload)?,
        created_at: value.created_at,
        updated_at: value.updated_at,
        resolved_at: value.resolved_at,
        resolution: value.resolution.map(resolution),
    })
}

fn payload(value: InteractionPayload) -> Result<BridgeInteractionPayload> {
    Ok(match value {
        InteractionPayload::UserInput { questions } => BridgeInteractionPayload::UserInput {
            questions: questions.into_iter().map(question).collect(),
        },
        InteractionPayload::ToolApproval {
            name,
            arguments,
            working_directory,
            parent_agent_id,
        } => BridgeInteractionPayload::ToolApproval {
            name,
            arguments_json: serde_json::to_string(&arguments)?,
            working_directory,
            parent_agent_id,
        },
        InteractionPayload::PlanConfirmation { plan_id, content } => {
            BridgeInteractionPayload::PlanConfirmation { plan_id, content }
        }
    })
}

fn resolution(value: InteractionResolution) -> BridgeInteractionResolution {
    match value {
        InteractionResolution::UserInput { answers } => {
            let mut answers = answers
                .into_iter()
                .map(|(question_id, answer)| BridgeUserInputAnswer {
                    question_id,
                    answers: answer.answers,
                })
                .collect::<Vec<_>>();
            answers.sort_by(|left, right| left.question_id.cmp(&right.question_id));
            BridgeInteractionResolution::UserInput { answers }
        }
        InteractionResolution::ToolApproval { decision, reason } => {
            BridgeInteractionResolution::ToolApproval {
                decision: match decision {
                    ToolApprovalResolution::Approved => BridgeToolApprovalResolution::Approved,
                    ToolApprovalResolution::Denied => BridgeToolApprovalResolution::Denied,
                },
                reason,
            }
        }
        InteractionResolution::PlanConfirmation {
            decision,
            content,
            reason,
        } => BridgeInteractionResolution::PlanConfirmation {
            decision: match decision {
                PlanConfirmationResolution::ImplementFreshContext => {
                    BridgePlanConfirmationResolution::ImplementFreshContext
                }
                PlanConfirmationResolution::ContinuePlanning => {
                    BridgePlanConfirmationResolution::ContinuePlanning
                }
                PlanConfirmationResolution::Dismiss => BridgePlanConfirmationResolution::Dismiss,
            },
            content,
            reason,
        },
    }
}

fn question(value: UserQuestion) -> BridgeUserQuestion {
    BridgeUserQuestion {
        id: value.id,
        header: value.header,
        question: value.question,
        is_other: value.is_other,
        is_secret: value.is_secret,
        options: value.options.map(|options| {
            options
                .into_iter()
                .map(|option| BridgeUserQuestionOption {
                    label: option.label,
                    description: option.description,
                })
                .collect()
        }),
    }
}
