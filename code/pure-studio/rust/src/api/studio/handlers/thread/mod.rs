use anyhow::Context;

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::records::thread_from_record;
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::{
    ArchiveThreadResult, BridgeError, BridgeThreadMode, StartNewThreadResponse, StartTurnResponse,
};
use pl_studio_runtime::{StudioMode, StudioStartNewThreadRequest, StudioSubmitPromptOptions};

fn studio_mode(mode: BridgeThreadMode) -> StudioMode {
    match mode {
        BridgeThreadMode::Simple => StudioMode::Simple,
        BridgeThreadMode::Task => StudioMode::Task,
    }
}

/// Creates a root Thread with the requested mode and accepts its first Turn.
///
/// # Errors
///
/// Returns an error when the Project does not exist, the prompt is empty, or the Turn is rejected.
pub async fn start_new_thread(
    project_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
    mode: BridgeThreadMode,
) -> Result<StartNewThreadResponse, BridgeError> {
    let mode = studio_mode(mode);
    let bridge = active_bridge().await?;
    let response = bridge
        .studio
        .start_new_thread(StudioStartNewThreadRequest {
            project_id,
            title: "New Session".to_string(),
            prompt,
            attachment_ids,
            mode,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await?;
    Ok(StartNewThreadResponse {
        thread: bridge_thread(thread_from_record(response.thread)),
        receipt: StartTurnResponse {
            thread_id: response.submission.thread_id,
            turn_id: response.submission.turn_id,
            revision: response.submission.cursor,
        },
    })
}

/// Archives a root Thread and selects the next available Thread.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or its tree is active.
pub async fn archive_thread(thread_id: String) -> Result<ArchiveThreadResult, BridgeError> {
    let bridge = active_bridge().await?;
    let archived = bridge
        .studio
        .archive_thread(thread_id)
        .await?
        .context("selected Thread not found")?;
    Ok(ArchiveThreadResult {
        archived_root_id: archived.archived_root_id,
        removed_thread_ids: archived.removed_thread_ids,
        next_root: archived
            .next_root
            .map(thread_from_record)
            .map(bridge_thread),
    })
}

/// Changes the selected root Thread between Simple and Task mode.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or has an active Task.
pub async fn set_thread_mode(thread_id: String, mode: BridgeThreadMode) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .set_thread_mode(&thread_id, studio_mode(mode))
        .await?;
    Ok(())
}
