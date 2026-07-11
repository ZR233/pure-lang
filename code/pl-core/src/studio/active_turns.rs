use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
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
    runtime_state: StudioRuntimeState,
}

struct ActiveTurn {
    turn_id: String,
    token: CancellationToken,
}

impl StudioActiveTurns {
    pub(super) fn new(runtime_state: StudioRuntimeState) -> Self {
        Self {
            turns: Arc::new(Mutex::new(HashMap::new())),
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
        drop(turns);
        let _ = self.runtime_state.clear_active_turn(session_id, turn_id);
        true
    }

    pub(super) async fn cancel_all_and_clear(&self) {
        let active_turns = {
            let mut turns = self.turns.lock().await;
            for turn in turns.values() {
                turn.token.cancel();
            }
            let active_turns = turns
                .iter()
                .map(|(session_id, turn)| (session_id.clone(), turn.turn_id.clone()))
                .collect::<Vec<_>>();
            turns.clear();
            active_turns
        };
        for (session_id, turn_id) in active_turns {
            let _ = self.runtime_state.clear_active_turn(&session_id, &turn_id);
        }
    }
}
