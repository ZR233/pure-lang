use anyhow::Context;

use super::snapshot::studio_snapshot_inner;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{BridgeError, BridgeStudioSnapshotResponse, BridgeThreadMode};
use pl_studio_runtime::StudioMode;

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
