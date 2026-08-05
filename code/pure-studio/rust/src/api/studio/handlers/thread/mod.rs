use anyhow::Context;

use super::snapshot::studio_snapshot_inner;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{BridgeError, BridgeStudioSnapshotResponse, BridgeThreadMode};
use pl_studio_runtime::StudioMode;

/// Creates and selects a Simple-mode root Thread for a Project.
///
/// # Errors
///
/// Returns an error when the Project does not exist or the canonical snapshot cannot be read.
pub async fn create_thread(
    project_id: String,
    title: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let title = title.unwrap_or_else(|| "New Session".to_string());
    let thread = bridge.studio.create_thread(&project_id, &title).await?;
    Ok(studio_snapshot_inner(bridge, Some(project_id), Some(thread.id)).await?)
}

/// Archives a root Thread and selects the next available Thread.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or its tree is active.
pub async fn archive_thread(
    thread_id: String,
    selected_thread_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let archived = bridge
        .studio
        .archive_thread(thread_id)
        .await?
        .context("selected Thread not found")?;
    Ok(studio_snapshot_inner(bridge, Some(archived.project_id), selected_thread_id).await?)
}

/// Changes the selected root Thread between Simple and Task mode.
///
/// # Errors
///
/// Returns an error when the Thread does not exist, is a child Thread, or has an active Task.
pub async fn set_thread_mode(
    thread_id: String,
    mode: BridgeThreadMode,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mode = match mode {
        BridgeThreadMode::Simple => StudioMode::Simple,
        BridgeThreadMode::Task => StudioMode::Task,
    };
    bridge.studio.set_thread_mode(&thread_id, mode).await?;
    let project_id = bridge
        .studio
        .store()
        .read_thread(&thread_id)
        .await?
        .context("Thread not found after changing mode")?
        .project_id;
    Ok(studio_snapshot_inner(bridge, Some(project_id), Some(thread_id)).await?)
}
