use crate::api::studio::convert::thread_stream::bridge_interaction;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, BridgeInteractionRequest, BridgeInteractionResolution,
    BridgePlanConfirmationResolution, BridgeToolApprovalResolution, InterruptTurnResponse,
    StartTurnResponse, SteerTurnResponse,
};
use pl_protocol::{
    InteractionResolution, PlanConfirmationResolution, ToolApprovalResolution, UserInputAnswer,
};
use pl_studio_runtime::{StudioSubmitPromptOptions, StudioSubmitPromptRequest};
use std::collections::HashMap;
// ── Prompt / Interaction ──

pub async fn start_turn(
    thread_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<StartTurnResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let response = bridge
        .studio
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            prompt,
            attachment_ids,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await?;
    Ok(StartTurnResponse {
        thread_id: response.thread_id,
        turn_id: response.turn_id,
        revision: response.cursor,
    })
}

pub async fn steer_turn(
    thread_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<SteerTurnResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let response = bridge
        .studio
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            prompt,
            attachment_ids,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::SteerOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await?;
    Ok(SteerTurnResponse {
        thread_id: response.thread_id,
        turn_id: response.turn_id,
        revision: response.cursor,
    })
}

pub async fn interrupt_turn(
    thread_id: String,
    turn_id: String,
) -> Result<InterruptTurnResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let snapshot = bridge.studio.thread_snapshot(&thread_id).await?;
    let active_turn_id = snapshot.active_turn.as_ref().map(|turn| turn.id.as_str());
    if active_turn_id.is_some_and(|active| active != turn_id) {
        return Err(BridgeError::invalid_argument(
            "interruptTurn turnId does not match the active Turn",
        ));
    }
    let response = bridge.studio.stop_prompt(thread_id).await?;
    Ok(InterruptTurnResponse {
        thread_id: response.thread_id,
        turn_id,
        interrupted: response.stopped,
    })
}

pub async fn respond_interaction(
    interaction_id: String,
    resolution: BridgeInteractionResolution,
) -> Result<BridgeInteractionRequest, BridgeError> {
    let bridge = active_bridge().await?;
    let resolution = interaction_resolution(resolution)?;
    let response = bridge
        .studio
        .resolve_interaction(interaction_id, resolution)
        .await?;
    Ok(bridge_interaction(response.interaction)?)
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
