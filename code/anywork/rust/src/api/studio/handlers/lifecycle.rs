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
    publish_startup_stage(pl_studio_runtime::StudioStartupStage::OpeningStorage);
    let bridge = match install_bridge_runtime().await {
        Ok(bridge) => bridge,
        Err(error) => {
            publish_startup_stage(pl_studio_runtime::StudioStartupStage::Failed);
            return Err(error.into());
        }
    };
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
        crate::diagnostics::shutdown();
    }
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

/// Changes a Project display name, preserving its path and connection.
pub async fn rename_project(project_id: String, name: String) -> Result<ProjectDto, BridgeError> {
    let bridge = super::super::bridge_runtime::active_bridge().await?;
    Ok(bridge
        .studio
        .rename_project(&project_id, &name)
        .await?
        .into())
}

static STARTUP: std::sync::LazyLock<
    tokio::sync::watch::Sender<super::super::types::BridgeStartupStage>,
> = std::sync::LazyLock::new(|| {
    tokio::sync::watch::channel(super::super::types::BridgeStartupStage::OpeningStorage).0
});

pub(crate) fn publish_startup_stage(stage: pl_studio_runtime::StudioStartupStage) {
    use super::super::types::BridgeStartupStage as Target;
    use pl_studio_runtime::StudioStartupStage as Source;
    STARTUP.send_replace(match stage {
        Source::OpeningStorage => Target::OpeningStorage,
        Source::LoadingConfiguration => Target::LoadingConfiguration,
        Source::ReadingProjects => Target::ReadingProjects,
        Source::PreparingResources => Target::PreparingResources,
        Source::Ready => Target::Ready,
        Source::Failed => Target::Failed,
    });
}

#[frb(sync)]
pub fn read_startup_stage() -> super::super::types::BridgeStartupStage {
    *STARTUP.borrow()
}

pub async fn read_recovery_state()
-> Result<super::super::types::BridgeRecoveryStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(super::super::convert::runtime::bridge_recovery_state(
        bridge.studio.read_recovery_state().state,
    ))
}

pub async fn retry_recovery()
-> Result<super::super::types::BridgeRecoveryStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(super::super::convert::runtime::bridge_recovery_state(
        bridge.studio.retry_recovery().await?.state,
    ))
}
