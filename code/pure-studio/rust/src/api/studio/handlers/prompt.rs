use crate::api::studio::convert::interaction::resolve_interaction_response;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, BridgeInteractionResolution, BridgePlanConfirmationResolution,
    BridgeToolApprovalResolution, ResolveInteractionResponse, StopPromptResponse,
    SubmitPromptResponse,
};
use pl_protocol::{
    InteractionResolution, PlanConfirmationResolution, ToolApprovalResolution, UserInputAnswer,
};
use pl_studio_runtime::{StudioSubmitPromptOptions, StudioSubmitPromptRequest};
use std::collections::HashMap;
// ── Prompt / Interaction ──

pub async fn submit_prompt(
    session_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<SubmitPromptResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let response = bridge
        .studio
        .submit_prompt(StudioSubmitPromptRequest {
            session_id,
            prompt,
            attachment_ids,
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    Ok(SubmitPromptResponse {
        session_id: response.session_id,
        turn_id: response.turn_id,
        cursor: response.cursor,
    })
}

pub async fn stop_prompt(session_id: String) -> Result<StopPromptResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let response = bridge.studio.stop_prompt(session_id).await?;
    Ok(StopPromptResponse {
        session_id: response.session_id,
        stopped: response.stopped,
    })
}

pub async fn resolve_interaction(
    interaction_id: String,
    resolution: BridgeInteractionResolution,
) -> Result<ResolveInteractionResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let resolution = interaction_resolution(resolution)?;
    let response = bridge
        .studio
        .resolve_interaction(interaction_id, resolution)
        .await?;
    Ok(resolve_interaction_response(response))
}

fn interaction_resolution(
    resolution: BridgeInteractionResolution,
) -> Result<InteractionResolution, BridgeError> {
    Ok(match resolution {
        BridgeInteractionResolution::UserInput { answers } => {
            let mut mapped = HashMap::with_capacity(answers.len());
            for answer in answers {
                if mapped
                    .insert(
                        answer.question_id.clone(),
                        UserInputAnswer {
                            answers: answer.answers,
                        },
                    )
                    .is_some()
                {
                    return Err(BridgeError::invalid_argument(format!(
                        "duplicate answer for question {}",
                        answer.question_id
                    )));
                }
            }
            InteractionResolution::UserInput { answers: mapped }
        }
        BridgeInteractionResolution::ToolApproval { decision, reason } => {
            InteractionResolution::ToolApproval {
                decision: match decision {
                    BridgeToolApprovalResolution::Approved => ToolApprovalResolution::Approved,
                    BridgeToolApprovalResolution::Denied => ToolApprovalResolution::Denied,
                },
                reason,
            }
        }
        BridgeInteractionResolution::PlanConfirmation {
            decision,
            content,
            reason,
        } => InteractionResolution::PlanConfirmation {
            decision: match decision {
                BridgePlanConfirmationResolution::ImplementFreshContext => {
                    PlanConfirmationResolution::ImplementFreshContext
                }
                BridgePlanConfirmationResolution::ContinuePlanning => {
                    PlanConfirmationResolution::ContinuePlanning
                }
                BridgePlanConfirmationResolution::Dismiss => PlanConfirmationResolution::Dismiss,
            },
            content,
            reason,
        },
    })
}
