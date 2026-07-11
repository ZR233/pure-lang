use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContinuationReason {
    AgentTerminal,
    MergeConflict,
    ReviewReturned,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContinuationRequest {
    pub(crate) task_run_id: String,
    pub(crate) session_id: String,
    pub(crate) reason: ContinuationReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionTurnState {
    Active,
    Idle,
}

#[derive(Clone, Default)]
pub(crate) struct ContinuationScheduler {
    sessions: Arc<Mutex<HashMap<String, SessionContinuation>>>,
}

impl ContinuationScheduler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn request(
        &self,
        request: ContinuationRequest,
        session_turn_state: SessionTurnState,
    ) -> Option<ContinuationRequest> {
        let mut sessions = self.sessions.lock().await;
        let state = sessions.entry(request.session_id.clone()).or_default();
        if request.reason == ContinuationReason::Recovery && state.active.as_ref() == Some(&request)
        {
            return None;
        }
        state.pending = Some(request);
        if session_turn_state == SessionTurnState::Active || state.active.is_some() {
            return None;
        }
        claim_pending(state)
    }

    pub(crate) async fn turn_removed(&self, session_id: &str) -> Option<ContinuationRequest> {
        let mut sessions = self.sessions.lock().await;
        let state = sessions.get_mut(session_id)?;
        state.active = None;
        let launch = claim_pending(state);
        if state.active.is_none() && state.pending.is_none() {
            sessions.remove(session_id);
        }
        launch
    }

    pub(crate) async fn defer(&self, request: ContinuationRequest) {
        let mut sessions = self.sessions.lock().await;
        let state = sessions.entry(request.session_id.clone()).or_default();
        state.active = None;
        state.pending = Some(request);
    }

    pub(crate) async fn cancel_session(&self, session_id: &str) {
        self.sessions.lock().await.remove(session_id);
    }
}

#[derive(Default)]
struct SessionContinuation {
    pending: Option<ContinuationRequest>,
    active: Option<ContinuationRequest>,
}

fn claim_pending(state: &mut SessionContinuation) -> Option<ContinuationRequest> {
    let request = state.pending.take()?;
    state.active = Some(request.clone());
    Some(request)
}
