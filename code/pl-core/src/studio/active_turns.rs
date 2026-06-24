use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, bail};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::studio::StudioRuntimeState;

#[derive(Clone)]
pub(super) struct StudioActiveTurns {
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    runtime_state: StudioRuntimeState,
}

impl StudioActiveTurns {
    pub(super) fn new(runtime_state: StudioRuntimeState) -> Self {
        Self {
            tokens: Arc::new(Mutex::new(HashMap::new())),
            runtime_state,
        }
    }

    pub(super) async fn insert(
        &self,
        session_id: String,
        turn_id: String,
        token: CancellationToken,
    ) -> Result<()> {
        let mut tokens = self.tokens.lock().await;
        if tokens.contains_key(&session_id) {
            bail!("session already has an active turn");
        }
        tokens.insert(session_id.clone(), token);
        drop(tokens);
        let _ = self.runtime_state.mark_active_turn(session_id, turn_id);
        Ok(())
    }

    pub(super) async fn token(&self, session_id: &str) -> Option<CancellationToken> {
        self.tokens.lock().await.get(session_id).cloned()
    }

    pub(super) async fn contains(&self, session_id: &str) -> bool {
        self.tokens.lock().await.contains_key(session_id)
    }

    pub(super) async fn contains_any<'a>(
        &self,
        session_ids: impl IntoIterator<Item = &'a String>,
    ) -> bool {
        let tokens = self.tokens.lock().await;
        session_ids
            .into_iter()
            .any(|session_id| tokens.contains_key(session_id))
    }

    pub(super) async fn remove(&self, session_id: &str) {
        self.tokens.lock().await.remove(session_id);
        let _ = self.runtime_state.clear_active_turn(session_id);
    }

    pub(super) async fn cancel_all_and_clear(&self) {
        let session_ids = {
            let mut tokens = self.tokens.lock().await;
            for token in tokens.values() {
                token.cancel();
            }
            let session_ids = tokens.keys().cloned().collect::<Vec<_>>();
            tokens.clear();
            session_ids
        };
        for session_id in session_ids {
            let _ = self.runtime_state.clear_active_turn(&session_id);
        }
    }
}
