use crate::api::studio::convert::interaction::resolve_interaction_response;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{
    ResolveInteractionResponse, StopPromptResponse, SubmitPromptResponse,
};
use anyhow::{Context, Result};
use pl_protocol::InteractionResolution;
use pl_studio_runtime::{StudioSubmitPromptOptions, StudioSubmitPromptRequest};
// ── Prompt / Interaction ──

pub fn submit_prompt(
    session_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<SubmitPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
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
    })
}

pub fn stop_prompt(session_id: String) -> Result<StopPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge.studio.stop_prompt(session_id).await?;
        Ok(StopPromptResponse {
            session_id: response.session_id,
            stopped: response.stopped,
        })
    })
}

pub fn resolve_interaction(
    interaction_id: String,
    resolution_json: String,
) -> Result<ResolveInteractionResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let resolution: InteractionResolution = serde_json::from_str(&resolution_json)
            .context("invalid interaction resolution json")?;
        let response = bridge
            .studio
            .resolve_interaction(interaction_id, resolution)
            .await?;
        Ok(resolve_interaction_response(response))
    })
}
