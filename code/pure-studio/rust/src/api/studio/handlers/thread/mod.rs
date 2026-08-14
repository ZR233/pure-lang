use anyhow::Context;

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::records::thread_from_record;
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::{BridgeError, BridgeThread, BridgeThreadMode};
use pl_studio_runtime::StudioMode;

/// Creates and selects a Simple-mode root Thread for a Project.
///
/// # Errors
///
/// Returns an error when the Project does not exist or the canonical snapshot cannot be read.
pub async fn create_thread(
    project_id: String,
    title: Option<String>,
) -> Result<BridgeThread, BridgeError> {
    let bridge = active_bridge().await?;
    let title = title.unwrap_or_else(|| "New Session".to_string());
    let thread = bridge.studio.create_thread(&project_id, &title).await?;
    Ok(bridge_thread(thread_from_record(thread)))
}

/// Archives a root Thread and selects the next available Thread.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or its tree is active.
pub async fn archive_thread(thread_id: String) -> Result<BridgeThread, BridgeError> {
    let bridge = active_bridge().await?;
    let archived = bridge
        .studio
        .archive_thread(thread_id)
        .await?
        .context("selected Thread not found")?;
    Ok(bridge_thread(thread_from_record(archived)))
}

/// Changes the selected root Thread between Simple and Task mode.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or has an active Task.
pub async fn set_thread_mode(thread_id: String, mode: BridgeThreadMode) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    let mode = match mode {
        BridgeThreadMode::Simple => StudioMode::Simple,
        BridgeThreadMode::Task => StudioMode::Task,
    };
    bridge.studio.set_thread_mode(&thread_id, mode).await?;
    Ok(())
}
