use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value};
use tokio::sync::{Mutex, OwnedMutexGuard};

mod responses_websocket;

pub(crate) use responses_websocket::ResponsesWebSocketConnection;

/// 单个模型会话的运行期 transport 状态。
///
/// 该值随 `AgentSession` 跨 turn 复用，但不进入 durable history。克隆同一
/// session 时共享物理连接；fork 和持久化恢复会创建新的 transport session，
/// 因而不同 agent/session 不会共用 Responses WebSocket continuation。
#[derive(Clone, Default)]
pub struct ModelSession {
    responses_websocket: Arc<Mutex<ResponsesWebSocketSession>>,
    responses_http_fallback_keys: Arc<RwLock<HashSet<u64>>>,
    orchestration: Arc<TransportOrchestrationCounters>,
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
}
