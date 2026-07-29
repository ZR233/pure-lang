use super::snapshot::studio_snapshot_inner;
use crate::api::studio::convert::runtime::bridge_recovery_cleanup_preview;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{BridgeRecoveryCleanupPreviewDto, BridgeStudioSnapshotResponse};
use anyhow::Result;

pub fn preview_recovery_issue_cleanup(issue_id: String) -> Result<BridgeRecoveryCleanupPreviewDto> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .preview_recovery_issue_cleanup(&issue_id)
            .await
            .map(bridge_recovery_cleanup_preview)
    })
}

pub fn preview_project_cleanup(project_id: String) -> Result<BridgeRecoveryCleanupPreviewDto> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .preview_project_cleanup(&project_id)
            .await
            .map(bridge_recovery_cleanup_preview)
    })
}

pub fn cleanup_recovery_issue(
    issue_id: String,
    expected_revision: String,
    selected_project_id: Option<String>,
    selected_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .cleanup_recovery_issue(&issue_id, &expected_revision)
            .await?;
        studio_snapshot_inner(bridge, selected_project_id, selected_session_id).await
    })
}

pub fn cleanup_project(
    project_id: String,
    expected_revision: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .cleanup_project(&project_id, &expected_revision)
            .await?;
        studio_snapshot_inner(bridge, selected_project_id, None).await
    })
}
