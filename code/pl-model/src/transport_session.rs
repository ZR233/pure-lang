use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
pub struct ModelTransportSession {
    responses_websocket: Arc<Mutex<ResponsesWebSocketSession>>,
    responses_http_fallback: Arc<AtomicBool>,
}

impl std::fmt::Debug for ModelTransportSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelTransportSession")
            .finish_non_exhaustive()
    }
}

impl ModelTransportSession {
    pub(crate) fn uses_responses_http_fallback(&self) -> bool {
        self.responses_http_fallback.load(Ordering::Relaxed)
    }

    pub(crate) async fn activate_responses_http_fallback(&self) -> bool {
        let activated = !self.responses_http_fallback.swap(true, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn responses_http_fallback_is_shared_only_within_one_model_session() {
        let session = ModelTransportSession::default();
        let clone = session.clone();

        assert!(session.activate_responses_http_fallback().await);
        assert!(clone.uses_responses_http_fallback());
        assert!(!clone.activate_responses_http_fallback().await);
        assert!(!ModelTransportSession::default().uses_responses_http_fallback());
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
