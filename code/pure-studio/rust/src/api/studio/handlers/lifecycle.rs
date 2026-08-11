use super::snapshot::{
    bootstrap_studio_inner, ensure_project_recovery_available, studio_snapshot_from_projects_inner,
    studio_snapshot_inner,
};
use crate::api::studio::bridge_runtime::{BridgeLifecycle, BridgeRuntime, active_bridge, bridge};
use crate::api::studio::convert::runtime::runtime_snapshot;
use crate::api::studio::types::{BridgeError, BridgeStudioSnapshotResponse, RuntimeSnapshot};
use anyhow::Context;
use flutter_rust_bridge::frb;

use super::updater::cancel_all_update_operations;
// ── Runtime lifecycle ──

#[allow(unexpected_cfgs)]
#[frb(init)]
pub fn init_app() {
    crate::diagnostics::initialize();
}

pub async fn initialize_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = bridge().await?;
    let lifecycle = bridge.lifecycle.lock().await;
    match *lifecycle {
        BridgeLifecycle::Stopped | BridgeLifecycle::ShuttingDown => {
            Err(BridgeError::runtime_stopped())
        }
        BridgeLifecycle::Initialized | BridgeLifecycle::Started => {
            Ok(runtime_snapshot(bridge.studio.initialize_runtime().await?))
        }
    }
}

pub async fn start_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = bridge().await?;
    let mut lifecycle = bridge.lifecycle.lock().await;
    match *lifecycle {
        BridgeLifecycle::Stopped | BridgeLifecycle::ShuttingDown => {
            Err(BridgeError::runtime_stopped())
        }
        BridgeLifecycle::Initialized => {
            let snapshot = runtime_snapshot(bridge.studio.start_runtime().await?);
            *lifecycle = BridgeLifecycle::Started;
            Ok(snapshot)
        }
        BridgeLifecycle::Started => Ok(runtime_snapshot(bridge.studio.runtime_snapshot().await?)),
    }
}

pub async fn shutdown_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = bridge().await?;
    loop {
        let shutdown_complete = bridge.shutdown_complete.notified();
        let starts_shutdown = {
            let mut lifecycle = bridge.lifecycle.lock().await;
            match *lifecycle {
                BridgeLifecycle::Stopped => {
                    return Ok(runtime_snapshot(bridge.studio.runtime_snapshot().await?));
                }
                BridgeLifecycle::ShuttingDown => false,
                BridgeLifecycle::Initialized | BridgeLifecycle::Started => {
                    *lifecycle = BridgeLifecycle::ShuttingDown;
                    true
                }
            }
        };
        if starts_shutdown {
            break;
        }
        shutdown_complete.await;
    }

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
    *bridge.lifecycle.lock().await = BridgeLifecycle::Stopped;
    bridge.shutdown_complete.notify_waiters();
    Ok(runtime_snapshot(shutdown_result?))
}

pub(super) async fn shutdown_runtime_for_update(
    bridge: &'static BridgeRuntime,
) -> Result<bool, BridgeError> {
    let previous_lifecycle = loop {
        let shutdown_complete = bridge.shutdown_complete.notified();
        let lifecycle = {
            let mut lifecycle = bridge.lifecycle.lock().await;
            match *lifecycle {
                BridgeLifecycle::Stopped => return Ok(true),
                BridgeLifecycle::ShuttingDown => None,
                BridgeLifecycle::Initialized | BridgeLifecycle::Started => {
                    let previous = *lifecycle;
                    *lifecycle = BridgeLifecycle::ShuttingDown;
                    Some(previous)
                }
            }
        };
        if let Some(previous) = lifecycle {
            break previous;
        }
        shutdown_complete.await;
    };

    bridge.subscriptions.cancel_all().await;
    match bridge.studio.shutdown_runtime_if_idle().await {
        Ok(Some(_)) => {
            bridge.shutdown.cancel();
            *bridge.lifecycle.lock().await = BridgeLifecycle::Stopped;
            bridge.shutdown_complete.notify_waiters();
            tracing::info!("Studio runtime shutdown completed for update");
            crate::diagnostics::shutdown();
            Ok(true)
        }
        Ok(None) => {
            *bridge.lifecycle.lock().await = previous_lifecycle;
            bridge.shutdown_complete.notify_waiters();
            Ok(false)
        }
        Err(error) => {
            *bridge.lifecycle.lock().await = previous_lifecycle;
            bridge.shutdown_complete.notify_waiters();
            Err(error.into())
        }
    }
}

// ── Studio bootstrap ──

pub async fn bootstrap_studio() -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bootstrap_studio_inner(bridge).await?)
}

pub async fn open_project(path: String) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let project = bridge.studio.open_project(path).await?;
    bridge
        .studio
        .reconcile_lsp_runtime_for_project(&project.id)
        .await?;
    let _ = bridge.studio.ensure_project_threads(&project.id).await?;
    Ok(studio_snapshot_inner(bridge, Some(project.id), None).await?)
}

pub async fn select_project(
    project_id: String,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    ensure_project_recovery_available(bridge, &project_id)?;
    Ok(studio_snapshot_inner(bridge, Some(project_id), None).await?)
}

pub async fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .archive_project(&project_id)
        .await?
        .context("selected project not found")?;
    let projects = bridge.studio.list_projects().await?;
    let next_project_id = selected_project_id
        .filter(|id| id != &project_id && projects.iter().any(|project| project.id == *id))
        .or_else(|| projects.first().map(|project| project.id.clone()));
    Ok(studio_snapshot_from_projects_inner(bridge, projects, next_project_id, None).await?)
}
