use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, BridgeStudioUpdateCheckDto, BridgeStudioUpdateDto, BridgeStudioUpdateEventDto,
};
use crate::frb_generated::StreamSink;
use anyhow::{Context, Result};
use pl_studio_runtime::{
    StudioUpdate, StudioUpdateCancellation, StudioUpdateCheck, StudioUpdateError,
    StudioUpdateErrorCode, StudioUpdateEvent, StudioUpdater,
};
use std::sync::{Arc, OnceLock, Weak};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use super::lifecycle::shutdown_runtime_for_update;

static UPDATER: OnceLock<StudioUpdater> = OnceLock::new();
static UPDATE_OPERATIONS: OnceLock<Mutex<Vec<Weak<BridgeStudioUpdateOperationInner>>>> =
    OnceLock::new();

pub struct BridgeStudioUpdateOperation {
    inner: Arc<BridgeStudioUpdateOperationInner>,
}

struct BridgeStudioUpdateOperationInner {
    cancellation: StudioUpdateCancellation,
    task: Mutex<Option<JoinHandle<()>>>,
    sink_task: Mutex<Option<JoinHandle<()>>>,
    progress_receiver: Mutex<Option<mpsc::Receiver<BridgeStudioUpdateEventDto>>>,
}

impl BridgeStudioUpdateOperation {
    pub async fn progress_stream(
        &self,
        sink: StreamSink<BridgeStudioUpdateEventDto>,
    ) -> Result<(), BridgeError> {
        let mut receiver = self
            .inner
            .progress_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("update progress stream can only be opened once")
            })?;
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if sink.add(event).is_err() {
                    let _ = inner.cancellation.cancel();
                    break;
                }
            }
        });
        *self.inner.sink_task.lock().await = Some(task);
        Ok(())
    }

    pub async fn cancel(&self) -> Result<(), BridgeError> {
        self.inner.cancellation.cancel()?;
        self.inner.wait().await;
        Ok(())
    }
}

impl Drop for BridgeStudioUpdateOperation {
    fn drop(&mut self) {
        let _ = self.inner.cancellation.cancel();
    }
}

impl BridgeStudioUpdateOperationInner {
    async fn wait(&self) {
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
        if let Some(task) = self.sink_task.lock().await.take() {
            let _ = task.await;
        }
    }
}

pub async fn check_studio_update(
    current_version: String,
) -> Result<BridgeStudioUpdateCheckDto, BridgeError> {
    let _bridge = active_bridge().await?;
    let result = updater()?.check(&current_version).await?;
    Ok(match result {
        StudioUpdateCheck::UpToDate => BridgeStudioUpdateCheckDto::UpToDate,
        StudioUpdateCheck::Available(update) => BridgeStudioUpdateCheckDto::Available {
            update: bridge_update(update),
        },
    })
}

pub async fn install_studio_update(
    update: BridgeStudioUpdateDto,
) -> Result<BridgeStudioUpdateOperation, BridgeError> {
    let bridge = active_bridge().await?;
    let updater = updater()?.clone();
    if bridge.studio.is_busy_for_update().await? {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::RuntimeBusy,
            "Studio runtime has an active turn or task",
        )
        .into());
    }
    let update = runtime_update(update);
    let cancellation = StudioUpdateCancellation::new();
    let (bridge_progress_tx, bridge_progress_rx) = mpsc::channel(64);
    let inner = Arc::new(BridgeStudioUpdateOperationInner {
        cancellation: cancellation.clone(),
        task: Mutex::new(None),
        sink_task: Mutex::new(None),
        progress_receiver: Mutex::new(Some(bridge_progress_rx)),
    });
    let task = tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let forward = tokio::spawn(async move {
            while let Some(event) = progress_rx.recv().await {
                if bridge_progress_tx.send(bridge_event(event)).await.is_err() {
                    break;
                }
            }
        });
        let progress_for_install = progress_tx.clone();
        let _ = updater
            .install_after(update, progress_for_install, cancellation, || async {
                if !shutdown_runtime_for_update(bridge)
                    .await
                    .map_err(runtime_error)?
                {
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
    *inner.task.lock().await = Some(task);
    update_operations()
        .lock()
        .await
        .push(Arc::downgrade(&inner));
    Ok(BridgeStudioUpdateOperation { inner })
}

pub(crate) async fn cancel_all_update_operations() {
    let operations = {
        let mut registry = update_operations().lock().await;
        registry
            .drain(..)
            .filter_map(|operation| operation.upgrade())
            .collect::<Vec<_>>()
    };
    for operation in operations {
        let _ = operation.cancellation.cancel();
        operation.wait().await;
    }
}

fn update_operations() -> &'static Mutex<Vec<Weak<BridgeStudioUpdateOperationInner>>> {
    UPDATE_OPERATIONS.get_or_init(|| Mutex::new(Vec::new()))
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

fn runtime_error(error: BridgeError) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::InstallerLaunchFailed,
        format!(
            "failed to stop Studio runtime safely: {} ({})",
            error.message, error.correlation_id
        ),
    )
}
