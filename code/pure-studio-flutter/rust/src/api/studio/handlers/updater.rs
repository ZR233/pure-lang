use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{
    BridgeStudioUpdateCheckDto, BridgeStudioUpdateDto, BridgeStudioUpdateEventDto,
};
use crate::frb_generated::StreamSink;
use anyhow::{Context, Result};
use pl_studio_runtime::{
    StudioUpdate, StudioUpdateCheck, StudioUpdateError, StudioUpdateErrorCode, StudioUpdateEvent,
    StudioUpdater,
};
use std::sync::OnceLock;

static UPDATER: OnceLock<StudioUpdater> = OnceLock::new();

pub fn check_studio_update(current_version: String) -> Result<BridgeStudioUpdateCheckDto> {
    let bridge = bridge()?;
    let result = bridge.block_on(updater()?.check(&current_version))?;
    Ok(match result {
        StudioUpdateCheck::UpToDate => BridgeStudioUpdateCheckDto::UpToDate,
        StudioUpdateCheck::Available(update) => BridgeStudioUpdateCheckDto::Available {
            update: bridge_update(update),
        },
    })
}

pub fn install_studio_update(
    update: BridgeStudioUpdateDto,
    sink: StreamSink<BridgeStudioUpdateEventDto>,
) -> Result<()> {
    let bridge = bridge()?;
    let updater = updater()?.clone();
    if bridge.block_on(bridge.studio.is_busy_for_update())? {
        let _ = sink.add(BridgeStudioUpdateEventDto::Failed {
            code: StudioUpdateErrorCode::RuntimeBusy.as_str().to_string(),
            message: "Studio runtime has an active turn or task".to_string(),
        });
        return Ok(());
    }
    let update = runtime_update(update);
    bridge.tokio.spawn(async move {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let forward = bridge.tokio.spawn(async move {
            while let Some(event) = progress_rx.recv().await {
                if sink.add(bridge_event(event)).is_err() {
                    break;
                }
            }
        });
        let progress_for_install = progress_tx.clone();
        let _ = updater
            .install_after(update, progress_for_install, || async {
                let stopped = bridge
                    .studio
                    .shutdown_runtime_if_idle()
                    .await
                    .map_err(runtime_error)?;
                if stopped.is_none() {
                    return Err(StudioUpdateError::new(
                        StudioUpdateErrorCode::RuntimeBusy,
                        "Studio runtime became busy before installer launch",
                    ));
                }
                Ok(())
            })
            .await;
        drop(progress_tx);
        let _ = forward.await;
    });
    Ok(())
}

fn updater() -> Result<&'static StudioUpdater> {
    if let Some(updater) = UPDATER.get() {
        return Ok(updater);
    }
    let candidate = StudioUpdater::new_default()?;
    let _ = UPDATER.set(candidate);
    UPDATER.get().context("Studio updater was not initialized")
}

fn bridge_update(update: StudioUpdate) -> BridgeStudioUpdateDto {
    BridgeStudioUpdateDto {
        version: update.version,
        published_at: update.published_at,
        notes_url: update.notes_url,
        installer_url: update.installer.url,
        installer_size: update.installer.size,
        installer_sha256: update.installer.sha256,
        installer_signature_url: update.installer.signature,
    }
}

fn runtime_update(update: BridgeStudioUpdateDto) -> StudioUpdate {
    StudioUpdate {
        version: update.version,
        published_at: update.published_at,
        notes_url: update.notes_url,
        installer: pl_studio_runtime::StudioUpdateAsset {
            url: update.installer_url,
            size: update.installer_size,
            sha256: update.installer_sha256,
            signature: update.installer_signature_url,
        },
    }
}

fn bridge_event(event: StudioUpdateEvent) -> BridgeStudioUpdateEventDto {
    match event {
        StudioUpdateEvent::Started { total } => BridgeStudioUpdateEventDto::Started { total },
        StudioUpdateEvent::Progress { downloaded, total } => {
            BridgeStudioUpdateEventDto::Progress { downloaded, total }
        }
        StudioUpdateEvent::Verifying => BridgeStudioUpdateEventDto::Verifying,
        StudioUpdateEvent::InstallerLaunched => BridgeStudioUpdateEventDto::InstallerLaunched,
        StudioUpdateEvent::Failed { code, message } => {
            BridgeStudioUpdateEventDto::Failed { code, message }
        }
    }
}

fn runtime_error(error: anyhow::Error) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::InstallerLaunchFailed,
        format!("failed to stop Studio runtime safely: {error:#}"),
    )
}
