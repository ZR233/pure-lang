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
    permit: Arc<tokio::sync::Semaphore>,
}

impl Default for SessionAdmission {
    fn default() -> Self {
        Self {
            closing: AtomicBool::new(false),
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
    /// Stops and joins physical transport tasks after the owner has drained model calls.
    /// Clones identify the same session. Closing seals admission before draining active calls.
    ///
    /// # Errors
    /// Reports transport task failure, preserving the session for a close retry.
    pub async fn close(&self) -> Result<(), pl_protocol::PureError> {
        self.admission.closing.store(true, Ordering::Release);
        let _lease = self
            .admission
            .permit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                pl_protocol::PureError::LlmError("model session admission closed".into())
            })?;
        let pending = self.closing_transport.lock().await.clone();
        let close = if let Some(pending) = pending {
            pending
        } else {
            let connection = self.lock_responses_websocket().await.connection.take();
            let close = async move {
                if let Some(mut connection) = connection {
                    connection.close().await.map_err(Arc::new)?;
                }
                Ok(())
            }
            .boxed()
            .shared();
            *self.closing_transport.lock().await = Some(close.clone());
            close
        };
        let result = close.await;
        self.closing_transport.lock().await.take();
        result.map_err(|error| {
            pl_protocol::PureError::LlmError(format!("model transport close failed: {error}"))
        })?;
        self.lock_responses_websocket().await.invalidate();
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
    pub(crate) last_request: Option<Map<String, Value>>,
    pub(crate) last_response_id: Option<String>,
    pub(crate) last_response_items: Vec<Value>,
}

impl ResponsesWebSocketSession {
    pub(crate) fn invalidate(&mut self) {
        self.connection_key = None;
        self.connection = None;
        self.last_request = None;
        self.last_response_id = None;
        self.last_response_items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn responses_http_fallback_is_shared_only_within_one_model_session() {
        let session = ModelSession::default();
        let clone = session.clone();

        assert!(session.activate_responses_http_fallback(11).await);
        assert!(clone.uses_responses_http_fallback(11));
        assert!(!clone.uses_responses_http_fallback(12));
        assert!(!clone.activate_responses_http_fallback(11).await);
        assert!(!ModelSession::default().uses_responses_http_fallback(11));
    }

    #[test]
    fn continuation_metrics_are_shared_and_snapshotable() {
        let session = ModelSession::default();
        let clone = session.clone();
        let before = session.orchestration_snapshot();

        clone.record_continuation_attempt();
        clone.record_continuation_invalid();

        let after = session.orchestration_snapshot();
        assert_eq!(
            after.continuation_attempts - before.continuation_attempts,
            1
        );
        assert_eq!(after.continuation_invalid - before.continuation_invalid, 1);
        assert_eq!(after.continuation_used - before.continuation_used, 0);
    }
    #[tokio::test]
    async fn closing_waits_for_active_invocation_and_seals_all_clones() {
        let session = ModelSession::default();
        let clone = session.clone();
        let lease = session.admit().await.unwrap();
        let closing = session.close();
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        assert!(clone.admit().await.is_err());
        drop(lease);
        closing.await.unwrap();
        assert!(session.admit().await.is_err());
        session.close().await.expect("repeated close");
        assert!(ModelSession::default().admit().await.is_ok());
    }

    #[tokio::test]
    async fn cloned_session_serializes_invocations_without_sharing_with_other_sessions() {
        let session = ModelSession::default();
        let peer = session.clone();
        let lease = session.admit().await.unwrap();
        let waiting = peer.admit();
        tokio::pin!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        let other = ModelSession::default();
        let independent = other.admit().await.unwrap();
        drop(independent);
        drop(lease);
        let next = waiting.await.unwrap();
        drop(next);
        session.close().await.unwrap();
    }
    #[tokio::test]
    async fn interrupted_close_resumes_the_same_owned_transport_cleanup() {
        let session = ModelSession::default();
        let starts = Arc::new(AtomicU64::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let task_starts = starts.clone();
        let task_release = release.clone();
        let cleanup = async move {
            task_starts.fetch_add(1, Ordering::SeqCst);
            task_release.notified().await;
            Ok(())
        }
        .boxed()
        .shared();
        *session.closing_transport.lock().await = Some(cleanup);
        {
            let first = session.close();
            tokio::pin!(first);
            assert!(futures::poll!(&mut first).is_pending());
            assert_eq!(starts.load(Ordering::SeqCst), 1);
        }
        assert!(session.admit().await.is_err());
        release.notify_one();
        session.close().await.expect("resume cleanup");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(session.closing_transport.lock().await.is_none());
    }
}
