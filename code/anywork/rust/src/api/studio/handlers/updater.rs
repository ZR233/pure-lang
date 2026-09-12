use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::bridge_update_state;
use crate::api::studio::types::{BridgeError, BridgeUpdaterStateSnapshot};
use crate::frb_generated::StreamSink;
use anyhow::Result;
use pl_studio_runtime::{StudioUpdateCancellation, StudioUpdateError, StudioUpdateErrorCode};
use std::sync::{Arc, OnceLock, Weak};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use super::lifecycle::shutdown_runtime_for_update;

static UPDATE_OPERATIONS: OnceLock<Mutex<Vec<Weak<BridgeStudioUpdateOperationInner>>>> =
    OnceLock::new();

pub struct BridgeStudioUpdateOperation {
    inner: Arc<BridgeStudioUpdateOperationInner>,
}

struct BridgeStudioUpdateOperationInner {
    cancellation: StudioUpdateCancellation,
    task: Mutex<Option<JoinHandle<()>>>,
    sink_task: Mutex<Option<JoinHandle<()>>>,
    progress_receiver: Mutex<Option<mpsc::Receiver<BridgeUpdaterStateSnapshot>>>,
}

impl BridgeStudioUpdateOperation {
    pub async fn progress_stream(
        &self,
        sink: StreamSink<BridgeUpdaterStateSnapshot>,
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

pub async fn check_studio_update() -> Result<BridgeUpdaterStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.check_studio_update().await?;
    Ok(bridge_update_state(state))
}

pub async fn read_studio_update_state() -> Result<BridgeUpdaterStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_update_state(bridge.studio.read_update_state().await))
}

pub async fn install_studio_update(
    expected_revision: u64,
    version: String,
) -> Result<BridgeStudioUpdateOperation, BridgeError> {
    let bridge = active_bridge().await?;
    if bridge.studio.is_busy_for_update().await? {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::RuntimeBusy,
            "Studio runtime has an active turn or task",
        )
        .into());
    }
    let update = bridge
        .studio
        .verified_studio_update(expected_revision, &version)
        .await?;
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
            while let Some(state) = progress_rx.recv().await {
                if bridge_progress_tx
                    .send(bridge_update_state(state))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let progress_for_install = progress_tx.clone();
        let _ = bridge
            .studio
            .install_studio_update_after(update, progress_for_install, cancellation, || async {
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

fn runtime_error(error: BridgeError) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::InstallerLaunchFailed,
        format!(
            "failed to stop Studio runtime safely: {} ({})",
            error.message, error.correlation_id
        ),
    )
}
