use crate::api::studio::bridge_runtime::{
    BridgeRuntime, active_bridge, install_bridge_runtime, installed_bridge,
};
use crate::api::studio::convert::runtime::runtime_snapshot;
use crate::api::studio::types::{BridgeError, ProjectDto, RuntimeSnapshot};
use anyhow::Context;
use flutter_rust_bridge::frb;

use super::updater::cancel_all_update_operations;
// ── Runtime lifecycle ──

#[allow(unexpected_cfgs)]
#[frb(init)]
pub fn init_app() {
    crate::diagnostics::initialize();
}

pub async fn start_studio_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = install_bridge_runtime().await?;
    Ok(runtime_snapshot(bridge.studio.start_runtime().await?))
}

pub async fn shutdown_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = installed_bridge()?;
    bridge.shutdown.cancel();
    cancel_all_update_operations().await;
    bridge.subscriptions.cancel_all().await;
    let shutdown_result = bridge.studio.shutdown_runtime().await;
    if let Err(error) = &shutdown_result {
        tracing::error!(
            error_bytes = error.to_string().len(),
            "Studio runtime shutdown failed"
        );
    } else {
        tracing::info!("Studio runtime shutdown completed");
    }
    crate::diagnostics::shutdown();
    Ok(runtime_snapshot(shutdown_result?))
}

pub(super) async fn shutdown_runtime_for_update(
    bridge: &'static BridgeRuntime,
) -> Result<bool, BridgeError> {
    bridge.subscriptions.cancel_all().await;
    match bridge.studio.shutdown_runtime_if_idle().await {
        Ok(Some(_)) => {
            bridge.shutdown.cancel();
            tracing::info!("Studio runtime shutdown completed for update");
            crate::diagnostics::shutdown();
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

// ── Studio commands ──

pub async fn open_project(path: String) -> Result<ProjectDto, BridgeError> {
    let bridge = active_bridge().await?;
    let project = bridge.studio.open_project(path).await?;
    Ok(project.into())
}

pub async fn activate_project(project_id: String) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    bridge.studio.activate_project(&project_id).await?;
    Ok(())
}

pub async fn archive_project(project_id: String) -> Result<Option<ProjectDto>, BridgeError> {
    let bridge = active_bridge().await?;
    let archived = bridge
        .studio
        .archive_project(&project_id)
        .await?
        .context("selected project not found")?;
    Ok(Some(archived.into()))
}
