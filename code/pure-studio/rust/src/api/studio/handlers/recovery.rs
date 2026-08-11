use super::snapshot::studio_snapshot_inner;
use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::{
    bridge_recovery_cleanup_preview, bridge_task_recovery_preview, bridge_task_recovery_result,
    task_recovery_request,
};
use crate::api::studio::types::{
    BridgeError, BridgeRecoveryCleanupPreviewDto, BridgeStudioSnapshotResponse,
    BridgeTaskRecoveryPreviewDto, BridgeTaskRecoveryRequestDto, BridgeTaskRecoveryResultDto,
};

pub async fn preview_task_recovery(
    root_thread_id: String,
) -> Result<BridgeTaskRecoveryPreviewDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .preview_task_recovery(&root_thread_id)
        .await
        .map(bridge_task_recovery_preview)?)
}

pub async fn apply_task_recovery(
    request: BridgeTaskRecoveryRequestDto,
) -> Result<BridgeTaskRecoveryResultDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .apply_task_recovery(task_recovery_request(request))
        .await
        .map(bridge_task_recovery_result)?)
}

pub async fn preview_recovery_issue_cleanup(
    issue_id: String,
) -> Result<BridgeRecoveryCleanupPreviewDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .preview_recovery_issue_cleanup(&issue_id)
        .await
        .map(bridge_recovery_cleanup_preview)?)
}

pub async fn preview_project_cleanup(
    project_id: String,
) -> Result<BridgeRecoveryCleanupPreviewDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .preview_project_cleanup(&project_id)
        .await
        .map(bridge_recovery_cleanup_preview)?)
}

pub async fn cleanup_recovery_issue(
    issue_id: String,
    expected_revision: String,
    selected_project_id: Option<String>,
    selected_thread_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .cleanup_recovery_issue(&issue_id, &expected_revision)
        .await?;
    Ok(studio_snapshot_inner(bridge, selected_project_id, selected_thread_id).await?)
}

pub async fn retry_recovery_issue(
    issue_id: String,
    selected_project_id: Option<String>,
    selected_thread_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    bridge.studio.retry_recovery_issue(&issue_id).await?;
    Ok(studio_snapshot_inner(bridge, selected_project_id, selected_thread_id).await?)
}

pub async fn cleanup_project(
    project_id: String,
    expected_revision: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .cleanup_project(&project_id, &expected_revision)
        .await?;
    Ok(studio_snapshot_inner(bridge, selected_project_id, None).await?)
}
