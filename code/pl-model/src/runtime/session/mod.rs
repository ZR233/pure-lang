use futures::FutureExt;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde_json::{Map, Value};
use tokio::sync::{Mutex, OwnedMutexGuard};

mod responses_websocket;

pub(crate) use responses_websocket::ResponsesWebSocketConnection;

/// 单个模型会话的运行期 transport 状态。
///
/// 该值由所属 Thread 跨 turn 复用，但不进入 durable history。克隆同一
/// session 时共享物理连接；fork 和持久化恢复会创建新的 transport session，
/// 因而不同 agent/session 不会共用 Responses WebSocket continuation。
#[derive(Clone, Default)]
pub struct ModelSession {
    pub(super) uploaded_files:
        Arc<Mutex<std::collections::HashMap<(u64, String), super::attachments::UploadState>>>,
    responses_websocket: Arc<Mutex<ResponsesWebSocketSession>>,
    admission: Arc<SessionAdmission>,
    closing_transport: Arc<Mutex<Option<TransportClose>>>,
    responses_http_fallback_keys: Arc<RwLock<HashSet<u64>>>,
    orchestration: Arc<TransportOrchestrationCounters>,
}

type TransportClose = futures::future::Shared<
    futures::future::BoxFuture<'static, Result<(), Arc<pl_protocol::PureError>>>,
>;

struct SessionAdmission {
    closing: AtomicBool,
    cancellation: tokio_util::sync::CancellationToken,
    permit: Arc<tokio::sync::Semaphore>,
}

impl Default for SessionAdmission {
    fn default() -> Self {
        Self {
            closing: AtomicBool::new(false),
            cancellation: tokio_util::sync::CancellationToken::new(),
            permit: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TransportOrchestrationSnapshot {
    pub(crate) continuation_attempts: u64,
    pub(crate) continuation_used: u64,
    pub(crate) continuation_invalid: u64,
}

#[derive(Default)]
struct TransportOrchestrationCounters {
    continuation_attempts: AtomicU64,
    continuation_used: AtomicU64,
    continuation_invalid: AtomicU64,
}

impl std::fmt::Debug for ModelSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelSession")
            .finish_non_exhaustive()
    }
}

impl ModelSession {
    /// Cancels active model calls and joins physical transport tasks.
    /// Clones identify the same session. Closing seals admission before draining active calls.
    ///
    /// # Errors
    /// Reports transport task failure, preserving the session for a close retry.
    pub async fn close(&self) -> Result<(), pl_protocol::PureError> {
        self.admission.closing.store(true, Ordering::Release);
        self.admission.cancellation.cancel();
        let _lease = self
            .admission
            .permit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                pl_protocol::PureError::LlmError("model session admission closed".into())
            })?;
        let mut pending = self.closing_transport.lock().await;
        let close = if let Some(pending) = pending.as_ref() {
            pending.clone()
        } else {
            let mut transport = self.lock_responses_websocket().await;
            transport.invalidate();
            let connections = std::mem::take(&mut transport.retired);
            let close = async move {
                let mut failure = None;
                for mut connection in connections {
                    if let Err(error) = connection.close().await {
                        failure.get_or_insert_with(|| Arc::new(error));
                    }
                }
                failure.map_or(Ok(()), Err)
            }
            .boxed()
            .shared();
            *pending = Some(close.clone());
            close
        };
        drop(pending);
        let result = close.await;
        self.closing_transport.lock().await.take();
        result.map_err(|error| {
            pl_protocol::PureError::LlmError(format!("model transport close failed: {error}"))
        })?;
        self.lock_responses_websocket().await.invalidate();
        self.uploaded_files.lock().await.clear();
        self.responses_http_fallback_keys
            .write()
            .map_err(|_| {
                pl_protocol::PureError::ConfigError(
                    "model session fallback state is poisoned".into(),
                )
            })?
            .clear();
        Ok(())
    }

    /// Resets physical continuation without changing session fallback policy.
    pub(crate) async fn reset_transport(&self) -> Result<(), pl_protocol::PureError> {
        let _lease = self.admit().await?;
        let mut transport = self.lock_responses_websocket().await;
        transport.invalidate();
        transport.reap_retired().await
    }

    pub(crate) async fn admit(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, pl_protocol::PureError> {
        if self.admission.closing.load(Ordering::Acquire) {
            return Err(pl_protocol::PureError::LlmError(
                "model session is closing".into(),
            ));
        }
        let lease = self
            .admission
            .permit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                pl_protocol::PureError::LlmError("model session admission closed".into())
            })?;
        if self.admission.closing.load(Ordering::Acquire) {
            return Err(pl_protocol::PureError::LlmError(
                "model session is closing".into(),
            ));
        }
        Ok(lease)
    }

    pub(crate) fn closing_token(&self) -> tokio_util::sync::CancellationToken {
        self.admission.cancellation.clone()
    }

    pub(crate) fn orchestration_snapshot(&self) -> TransportOrchestrationSnapshot {
        TransportOrchestrationSnapshot {
            continuation_attempts: self
                .orchestration
                .continuation_attempts
                .load(Ordering::Relaxed),
            continuation_used: self.orchestration.continuation_used.load(Ordering::Relaxed),
            continuation_invalid: self
                .orchestration
                .continuation_invalid
                .load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_continuation_attempt(&self) {
        self.orchestration
            .continuation_attempts
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_continuation_used(&self) {
        self.orchestration
            .continuation_used
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_continuation_invalid(&self) {
        self.orchestration
            .continuation_invalid
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn uses_responses_http_fallback(&self, connection_key: u64) -> bool {
        self.responses_http_fallback_keys
            .read()
            .is_ok_and(|keys| keys.contains(&connection_key))
    }

    pub(crate) async fn activate_responses_http_fallback(&self, connection_key: u64) -> bool {
        let activated = self
            .responses_http_fallback_keys
            .write()
            .is_ok_and(|mut keys| keys.insert(connection_key));
        if activated {
            self.lock_responses_websocket().await.invalidate();
        }
        activated
    }

    pub(crate) async fn lock_responses_websocket(
        &self,
    ) -> OwnedMutexGuard<ResponsesWebSocketSession> {
        Arc::clone(&self.responses_websocket).lock_owned().await
    }
}

#[derive(Default)]
pub(crate) struct ResponsesWebSocketSession {
    pub(crate) connection_key: Option<u64>,
    pub(crate) connection: Option<ResponsesWebSocketConnection>,
    retired: Vec<ResponsesWebSocketConnection>,
    pub(crate) last_request: Option<Map<String, Value>>,
    pub(crate) last_response_id: Option<String>,
    pub(crate) last_response_items: Vec<Value>,
}

impl ResponsesWebSocketSession {
    pub(crate) fn invalidate(&mut self) {
        self.connection_key = None;
        if let Some(connection) = self.connection.take() {
            connection.retire();
            self.retired.push(connection);
        }
        self.last_request = None;
        self.last_response_id = None;
        self.last_response_items.clear();
    }

    pub(crate) async fn reap_retired(&mut self) -> Result<(), pl_protocol::PureError> {
        while let Some(connection) = self.retired.last_mut() {
            // Keep the handle in session state until the await is complete:
            // cancellation must not detach a task that still needs joining.
            let result = connection.close().await;
            self.retired.pop();
            result?;
        }
        Ok(())
    }
}
