use super::snapshot::{
    bootstrap_studio_inner, studio_snapshot_from_projects_inner, studio_snapshot_inner,
};
use crate::api::studio::convert::runtime::runtime_snapshot;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{BridgeStudioSnapshotResponse, RuntimeSnapshot};
use anyhow::{Context, Result};
// ── Runtime lifecycle ──

pub fn initialize_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .initialize_runtime()
            .await
            .map(runtime_snapshot)
    })
}

pub fn start_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.start_runtime().await.map(runtime_snapshot) })
}

pub fn shutdown_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.shutdown_runtime().await.map(runtime_snapshot) })
}

// ── Studio bootstrap ──

pub fn bootstrap_studio() -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { bootstrap_studio_inner(bridge).await })
}

pub fn open_project(path: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let project = bridge.studio.open_project(path).await?;
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let _ = bridge.studio.ensure_project_sessions(&project.id).await?;
        studio_snapshot_inner(bridge, Some(project.id), None).await
    })
}

pub fn select_project(project_id: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { studio_snapshot_inner(bridge, Some(project_id), None).await })
}

pub fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .archive_project(&project_id)
            .await?
            .context("selected project not found")?;
        let projects = bridge.studio.list_projects().await?;
        let next_project_id = selected_project_id
            .filter(|id| id != &project_id && projects.iter().any(|project| project.id == *id))
            .or_else(|| projects.first().map(|project| project.id.clone()));
        studio_snapshot_from_projects_inner(bridge, projects, next_project_id, None).await
    })
}
