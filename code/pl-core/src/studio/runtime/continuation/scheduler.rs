use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::studio::ids::new_id;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContinuationClaim {
    pub(crate) claim_id: String,
    pub(crate) request: ContinuationRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionTurnState {
    Active,
    Idle,
}

#[derive(Clone, Default)]
pub(crate) struct ContinuationScheduler {
    state: Arc<Mutex<SchedulerState>>,
}

impl ContinuationScheduler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn request(
        &self,
        request: ContinuationRequest,
        session_turn_state: SessionTurnState,
    ) -> Option<ContinuationClaim> {
        let mut scheduler = self.state.lock().await;
        if !scheduler.enabled {
            return None;
        }
        let state = scheduler
            .sessions
            .entry(request.session_id.clone())
            .or_default();
        if request.reason == ContinuationReason::Recovery
            && state
                .active
                .as_ref()
                .is_some_and(|active| active.claim.request == request)
        {
            return None;
        }
        state.pending = Some(request);
        if session_turn_state == SessionTurnState::Active || state.active.is_some() {
            return None;
        }
        claim_pending(state)
    }

    pub(crate) async fn turn_removed(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<ContinuationClaim> {
        let mut scheduler = self.state.lock().await;
        if !scheduler.enabled {
            return None;
        }
        let state = scheduler.sessions.get_mut(session_id)?;
        match state.active.as_mut() {
            Some(active) if active.bound_turn_id.as_deref() == Some(turn_id) => {
                state.active = None;
            }
            Some(active) if active.bound_turn_id.is_some() => return None,
            Some(_) if state.pending.is_some() => {
                state.active = None;
            }
            Some(active) => {
                active.removed_turn_id = Some(turn_id.to_string());
                return None;
            }
            None => {}
        }
        let launch = claim_pending(state);
        if state.active.is_none() && state.pending.is_none() {
            scheduler.sessions.remove(session_id);
        }
        launch
    }

    pub(crate) async fn claim_if_idle(&self, session_id: &str) -> Option<ContinuationClaim> {
        let mut scheduler = self.state.lock().await;
        if !scheduler.enabled {
            return None;
        }
        let state = scheduler.sessions.get_mut(session_id)?;
        if state.active.is_some() {
            return None;
        }
        claim_pending(state)
    }

    pub(crate) async fn defer(&self, claim: ContinuationClaim) {
        let mut scheduler = self.state.lock().await;
        if !scheduler.enabled {
            return;
        }
        let Some(state) = scheduler.sessions.get_mut(&claim.request.session_id) else {
            return;
        };
        if state
            .active
            .as_ref()
            .map(|active| active.claim.claim_id.as_str())
            != Some(claim.claim_id.as_str())
        {
            return;
        }
        state.active = None;
        if state.pending.is_none() {
            state.pending = Some(claim.request);
        }
    }

    pub(crate) async fn bind_turn(
        &self,
        claim: &ContinuationClaim,
        turn_id: &str,
    ) -> Option<ContinuationClaim> {
        let mut scheduler = self.state.lock().await;
        let session_id = &claim.request.session_id;
        let state = scheduler.sessions.get_mut(session_id)?;
        let active = state.active.as_mut()?;
        if active.claim.claim_id != claim.claim_id {
            return None;
        }
        active.bound_turn_id = Some(turn_id.to_string());
        if active.removed_turn_id.as_deref() != Some(turn_id) {
            return None;
        }
        state.active = None;
        let launch = claim_pending(state);
        if state.active.is_none() && state.pending.is_none() {
            scheduler.sessions.remove(session_id);
        }
        launch
    }

    pub(crate) async fn cancel_claim(&self, claim: &ContinuationClaim) -> bool {
        let mut scheduler = self.state.lock().await;
        let Some(state) = scheduler.sessions.get_mut(&claim.request.session_id) else {
            return false;
        };
        if state
            .active
            .as_ref()
            .map(|active| active.claim.claim_id.as_str())
            != Some(claim.claim_id.as_str())
        {
            return false;
        }
        scheduler.sessions.remove(&claim.request.session_id);
        true
    }

    pub(crate) async fn pause_and_clear(&self) {
        let mut scheduler = self.state.lock().await;
        scheduler.enabled = false;
        scheduler.sessions.clear();
    }

    pub(crate) async fn resume(&self) {
        self.state.lock().await.enabled = true;
    }

    #[cfg(test)]
    pub(crate) async fn has_session(&self, session_id: &str) -> bool {
        self.state.lock().await.sessions.contains_key(session_id)
    }
}

struct SchedulerState {
    enabled: bool,
    sessions: HashMap<String, SessionContinuation>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            enabled: true,
            sessions: HashMap::new(),
        }
    }
}

#[derive(Default)]
struct SessionContinuation {
    pending: Option<ContinuationRequest>,
    active: Option<ActiveClaim>,
}

struct ActiveClaim {
    claim: ContinuationClaim,
    bound_turn_id: Option<String>,
    removed_turn_id: Option<String>,
}

fn claim_pending(state: &mut SessionContinuation) -> Option<ContinuationClaim> {
    let request = state.pending.take()?;
    let claim = ContinuationClaim {
        claim_id: new_id("continuation-claim"),
        request,
    };
    state.active = Some(ActiveClaim {
        claim: claim.clone(),
        bound_turn_id: None,
        removed_turn_id: None,
    });
    Some(claim)
}
