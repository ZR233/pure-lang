use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, bail};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

use crate::studio::StudioRuntimeState;

#[derive(Debug)]
pub(super) struct SessionAlreadyHasActiveTurn;

impl std::fmt::Display for SessionAlreadyHasActiveTurn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("session already has an active turn")
    }
}

impl std::error::Error for SessionAlreadyHasActiveTurn {}

#[derive(Clone)]
pub(super) struct StudioActiveTurns {
    turns: Arc<Mutex<HashMap<String, ActiveTurn>>>,
    active_count: watch::Sender<usize>,
    runtime_state: StudioRuntimeState,
}

struct ActiveTurn {
    turn_id: String,
    token: CancellationToken,
}

impl StudioActiveTurns {
    pub(super) fn new(runtime_state: StudioRuntimeState) -> Self {
        let (active_count, _) = watch::channel(0);
        Self {
            turns: Arc::new(Mutex::new(HashMap::new())),
            active_count,
            runtime_state,
        }
    }

    pub(super) async fn insert(
        &self,
        session_id: String,
        turn_id: String,
        token: CancellationToken,
    ) -> Result<()> {
        let mut turns = self.turns.lock().await;
        if !matches!(
            self.runtime_state.snapshot().status,
            crate::studio::StudioRuntimeStatus::Ready
        ) {
            bail!("Studio runtime is not ready");
        }
        if turns.contains_key(&session_id) {
            return Err(SessionAlreadyHasActiveTurn.into());
        }
        turns.insert(
            session_id.clone(),
            ActiveTurn {
                turn_id: turn_id.clone(),
                token,
            },
        );
        self.active_count.send_replace(turns.len());
        drop(turns);
        let _ = self.runtime_state.mark_active_turn(session_id, turn_id);
        Ok(())
    }

    pub(super) async fn token(&self, session_id: &str) -> Option<CancellationToken> {
        self.turns
            .lock()
            .await
            .get(session_id)
            .map(|turn| turn.token.clone())
    }

    pub(super) async fn contains(&self, session_id: &str) -> bool {
        self.turns.lock().await.contains_key(session_id)
    }

    pub(super) async fn contains_exact(&self, session_id: &str, turn_id: &str) -> bool {
        self.turns
            .lock()
            .await
            .get(session_id)
            .is_some_and(|turn| turn.turn_id == turn_id)
    }

    pub(super) async fn contains_any<'a>(
        &self,
        session_ids: impl IntoIterator<Item = &'a String>,
    ) -> bool {
        let turns = self.turns.lock().await;
        session_ids
            .into_iter()
            .any(|session_id| turns.contains_key(session_id))
    }

    pub(super) async fn remove(&self, session_id: &str, turn_id: &str) -> bool {
        let mut turns = self.turns.lock().await;
        if turns.get(session_id).map(|turn| turn.turn_id.as_str()) != Some(turn_id) {
            return false;
        }
        turns.remove(session_id);
        self.active_count.send_replace(turns.len());
        drop(turns);
        let _ = self.runtime_state.clear_active_turn(session_id, turn_id);
        true
    }

    pub(super) async fn cancel_all(&self) {
        let turns = self.turns.lock().await;
        for turn in turns.values() {
            turn.token.cancel();
        }
    }

    pub(super) async fn wait_until_empty(&self) {
        let mut active_count = self.active_count.subscribe();
        loop {
            if *active_count.borrow() == 0 {
                return;
            }
            if active_count.changed().await.is_err() {
                return;
            }
        }
    }
}
