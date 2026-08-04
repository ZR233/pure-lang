use anyhow::Result;

use crate::studio::{SessionHistoryPageRecord, StudioRuntime};

impl StudioRuntime {
    pub async fn load_session_history_page(
        &self,
        session_id: &str,
        before_turn_sequence: Option<i64>,
        limit: usize,
    ) -> Result<SessionHistoryPageRecord> {
        self.store
            .load_session_history_page(session_id, before_turn_sequence, limit)
            .await
    }
}
