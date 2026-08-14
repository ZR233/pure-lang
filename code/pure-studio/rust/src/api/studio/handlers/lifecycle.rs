use super::snapshot::ensure_project_recovery_available;
use crate::api::studio::bridge_runtime::{
    BridgeLifecycle, BridgeRuntime, active_bridge, install_bridge_runtime, installed_bridge,
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
    let mut lifecycle = bridge.lifecycle.lock().await;
    match *lifecycle {
        BridgeLifecycle::Stopped | BridgeLifecycle::ShuttingDown => {
            Err(BridgeError::runtime_stopped())
        }
        BridgeLifecycle::Initialized => {
            let _ = bridge.studio.initialize_runtime().await?;
            let snapshot = runtime_snapshot(bridge.studio.start_runtime().await?);
            *lifecycle = BridgeLifecycle::Started;
            Ok(snapshot)
        }
        BridgeLifecycle::Started => Ok(runtime_snapshot(bridge.studio.runtime_snapshot().await?)),
    }
}

pub async fn shutdown_runtime() -> Result<RuntimeSnapshot, BridgeError> {
    let bridge = installed_bridge()?;
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

// ── Studio commands ──

pub async fn open_project(path: String) -> Result<ProjectDto, BridgeError> {
    let bridge = active_bridge().await?;
    let project = bridge.studio.open_project(path).await?;
    Ok(project.into())
}

pub async fn activate_project(project_id: String) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    ensure_project_recovery_available(bridge, &project_id)?;
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
